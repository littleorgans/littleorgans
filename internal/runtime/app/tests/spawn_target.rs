#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use std::io::BufReader;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use common::{
    FAKE_RUNTIME_READY, RtmHarness, headless_spawn_command, headless_spawn_request_with_env,
    output_stderr, output_stdout, spawn_ok, spawn_output_ok, wait_for_log, wait_for_log_contains,
    wait_for_status,
};
use lilo_rm_core::{
    ErrorCode, IsolationPolicy, IsolationProfile, LaunchEnv, MountSpec, NudgeFailureReason,
    NudgeOutcome, NudgePayload, NudgeRequest, NudgeResponse, RuntimeResponse, RuntimeRpc,
    SpawnRequest, ValidateTargetOutcome, ValidateTargetPayload, ValidateTargetRequest,
    ValidateTargetResponse, read_json_line_blocking, write_json_line_blocking,
};
use lilo_wire::LilodRpc;
use serde_json::{Value, json};
use uuid::Uuid;

#[test]
fn explicit_headless_spawn_records_no_tmux_pane_and_rejects_nudge() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    let status = harness.status_format(&session_id, "json");
    assert!(status.status.success(), "status failed: {status:?}");
    let lifecycles: Value = serde_json::from_str(&output_stdout(status)).expect("status json");
    assert_eq!(lifecycles[0]["tmux_pane"], Value::Null);

    let response = request_raw(
        &harness,
        &RuntimeRpc::Nudge {
            request: NudgeRequest {
                session_id: session_id.parse().expect("session id"),
                content: "headless".to_owned(),
            },
        },
    );
    assert_eq!(
        response,
        RuntimeResponse::Nudge(NudgePayload {
            response: NudgeResponse {
                delivered: false,
                outcome: NudgeOutcome::Unsupported(NudgeFailureReason::HeadlessLifecycle),
            },
        })
    );

    let nudge = harness.nudge(&session_id, "headless");
    assert!(!nudge.status.success(), "nudge succeeded: {nudge:?}");
    let stderr = output_stderr(nudge);
    assert!(
        stderr.contains(&format!(
            "nudge unsupported; reason=headless_lifecycle session_id={session_id}"
        )),
        "{stderr}"
    );
}

#[test]
fn missing_session_nudge_uses_structured_error_code() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7();

    let response = request_raw(
        &harness,
        &RuntimeRpc::Nudge {
            request: NudgeRequest {
                session_id,
                content: "missing".to_owned(),
            },
        },
    );
    let RuntimeResponse::Error(payload) = response else {
        panic!("unexpected missing-session response: {response:?}");
    };
    assert_eq!(payload.code, ErrorCode::SessionNotFound);
    assert!(
        payload
            .message
            .contains(&format!("session {session_id} not found"))
    );
}

#[test]
fn headless_spawn_pipes_stdout_and_stderr_to_session_logs() {
    let harness = RtmHarness::start_outside_tmux();
    let session_id = Uuid::now_v7();
    let response = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(lilo_runtime_app::shared::request(
            harness.socket_path(),
            RuntimeRpc::Spawn {
                request: headless_spawn_request_with_env(
                    session_id,
                    harness.rtm_home(),
                    vec![LaunchEnv::new("RTM_TEST_STDIO_SENTINELS", "1")],
                ),
            },
        ))
        .expect("headless spawn");

    let RuntimeResponse::Spawned(payload) = response else {
        panic!("unexpected spawn response: {response:?}");
    };
    let log_dir = payload.log_dir.expect("headless log dir");
    let stdout_path = payload.stdout_path.expect("headless stdout path");
    let stderr_path = payload.stderr_path.expect("headless stderr path");
    assert_eq!(payload.lifecycle.tmux_pane, None);
    assert_eq!(
        log_dir,
        harness
            .rtm_home()
            .join("logs")
            .join("runtimes")
            .join(session_id.to_string())
    );
    assert_eq!(stdout_path, log_dir.join("stdout.log"));
    assert_eq!(stderr_path, log_dir.join("stderr.log"));
    wait_for_log(stdout_path, "HELLO\n");
    wait_for_log(stderr_path, "WORLD\n");
}

#[test]
fn headless_spawn_cwd_flag_overrides_caller_cwd() {
    let harness = RtmHarness::start_outside_tmux();
    let session_id = Uuid::now_v7().to_string();
    let caller_cwd = harness.rtm_home();
    let runtime_cwd = caller_cwd.join("runtime-cwd");
    std::fs::create_dir_all(&runtime_cwd).expect("runtime cwd");
    std::fs::write(runtime_cwd.join(".rtm-print-cwd"), "").expect("cwd marker");
    let runtime_cwd = std::fs::canonicalize(runtime_cwd).expect("canonical runtime cwd");

    let output = harness
        .spawn_command(&session_id, "claude", "headless", true)
        .arg("--cwd")
        .arg(&runtime_cwd)
        .current_dir(caller_cwd)
        .output()
        .expect("spawn client");
    spawn_output_ok(output, "claude");

    wait_for_log(
        caller_cwd
            .join("logs")
            .join("runtimes")
            .join(&session_id)
            .join("stdout.log"),
        &format!("{FAKE_RUNTIME_READY} {}\n", runtime_cwd.display()),
    );
}

#[test]
fn headless_spawn_env_flag_forwards_caller_explicit_duplicate_and_empty_values() {
    let harness = RtmHarness::start_outside_tmux();
    let session_id = Uuid::now_v7().to_string();
    let token = format!("token-{}", Uuid::now_v7().simple());
    let output = harness
        .spawn_command(&session_id, "claude", "headless", true)
        .arg("--env")
        .arg("RTM_TEST_PRINT_ENV=1")
        .arg("--env")
        .arg("CLAUDE_CODE_OAUTH_TOKEN")
        .arg("--env")
        .arg("CLAUDE_CODE_EXPLICIT=literal")
        .arg("--env")
        .arg("CLAUDE_CODE_DUP=first")
        .arg("--env")
        .arg("CLAUDE_CODE_DUP=second")
        .arg("--env")
        .arg("CLAUDE_CODE_EMPTY=")
        .env_clear()
        .env("LILO_SOCKET_PATH", harness.socket_path())
        .env("CLAUDE_CODE_OAUTH_TOKEN", &token)
        .env("HOME", "/host/home")
        .env("USER", "host-user")
        .env("SHELL", "/host/shell")
        .output()
        .expect("spawn client");
    spawn_output_ok(output, "claude");

    let stdout_path = harness
        .rtm_home()
        .join("logs")
        .join("runtimes")
        .join(&session_id)
        .join("stdout.log");
    let stdout = wait_for_log_contains(&stdout_path, &format!("CLAUDE_CODE_OAUTH_TOKEN={token}\n"));
    assert!(
        stdout.contains("CLAUDE_CODE_EXPLICIT=literal\n"),
        "{stdout}"
    );
    assert!(stdout.contains("CLAUDE_CODE_DUP=second\n"), "{stdout}");
    assert!(!stdout.contains("CLAUDE_CODE_DUP=first\n"), "{stdout}");
    assert!(stdout.contains("CLAUDE_CODE_EMPTY=\n"), "{stdout}");
}

#[test]
fn docker_spawn_mount_flag_forwards_mounts_in_declaration_order() {
    let session_id = Uuid::now_v7();
    let mut command = spawn_capture_command(session_id);
    command
        .arg("--isolation")
        .arg("docker")
        .arg("--mount")
        .arg("/host/path:/container/path")
        .arg("--mount")
        .arg("/host/readonly:/container/readonly:ro")
        .arg("--mount")
        .arg("./local-config:/container/local:rw");

    let (request, _output) = capture_spawn_request(command);

    assert_eq!(request.session_id, session_id);
    assert_eq!(
        request.isolation,
        IsolationPolicy::Docker(IsolationProfile { name: None })
    );
    assert_eq!(
        request.mounts,
        vec![
            MountSpec {
                source: "/host/path".into(),
                target: "/container/path".into(),
                read_only: true,
            },
            MountSpec {
                source: "/host/readonly".into(),
                target: "/container/readonly".into(),
                read_only: true,
            },
            MountSpec {
                source: "./local-config".into(),
                target: "/container/local".into(),
                read_only: false,
            },
        ]
    );
}

#[test]
fn docker_spawn_mount_flag_expands_leading_tilde_source() {
    let session_id = Uuid::now_v7();
    let mut command = spawn_capture_command(session_id);
    command
        .arg("--isolation")
        .arg("docker")
        .arg("--mount")
        .arg("~/foo:/container/path")
        .env_clear()
        .env("HOME", "/host/home");

    let (request, _output) = capture_spawn_request(command);

    assert_eq!(
        request.mounts,
        vec![MountSpec {
            source: "/host/home/foo".into(),
            target: "/container/path".into(),
            read_only: true,
        }]
    );
}

#[test]
fn docker_spawn_env_flag_reaches_container_and_runtime() {
    let harness = RtmHarness::start_outside_tmux();
    let session_id = Uuid::now_v7();
    let token = format!("token-{}", Uuid::now_v7().simple());
    let output = harness
        .spawn_command(&session_id.to_string(), "claude", "headless", true)
        .arg("--isolation")
        .arg("docker")
        .arg("--image")
        .arg("runtime-matters-agent:latest")
        .arg("--env")
        .arg("RTM_TEST_PRINT_ENV=1")
        .arg("--env")
        .arg("CLAUDE_CODE_OAUTH_TOKEN")
        .arg("--env")
        .arg("CLAUDE_CODE_DUP=first")
        .arg("--env")
        .arg("CLAUDE_CODE_DUP=second")
        .arg("--env")
        .arg("CLAUDE_CODE_EMPTY=")
        .env_clear()
        .env("LILO_SOCKET_PATH", harness.socket_path())
        .env("CLAUDE_CODE_OAUTH_TOKEN", &token)
        .env("HOME", "/host/home")
        .env("USER", "host-user")
        .env("SHELL", "/host/shell")
        .output()
        .expect("spawn client");
    spawn_output_ok(output, "claude");

    let env = common::wait_until(Duration::from_secs(5), || {
        let env = common::docker::container_env(&harness, session_id);
        env.contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={token}"))
            .then_some(env)
    })
    .unwrap_or_else(|| panic!("container env never contained CLAUDE_CODE_OAUTH_TOKEN"));
    assert!(
        env.contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={token}")),
        "{env:?}"
    );
    assert!(
        env.contains(&"CLAUDE_CODE_DUP=second".to_owned()),
        "{env:?}"
    );
    assert!(
        !env.contains(&"CLAUDE_CODE_DUP=first".to_owned()),
        "{env:?}"
    );
    assert!(env.contains(&"CLAUDE_CODE_EMPTY=".to_owned()), "{env:?}");
    assert!(!env.contains(&"HOME=/host/home".to_owned()), "{env:?}");
    assert!(!env.contains(&"USER=host-user".to_owned()), "{env:?}");
    assert!(!env.contains(&"SHELL=/host/shell".to_owned()), "{env:?}");
    let output = common::wait_until(Duration::from_secs(5), || {
        let output = common::docker::container_output(&harness, session_id);
        output
            .contains(&format!("CLAUDE_CODE_OAUTH_TOKEN={token}\n"))
            .then_some(output)
    })
    .unwrap_or_else(|| panic!("container runtime output never contained CLAUDE_CODE_OAUTH_TOKEN"));
    assert!(output.contains("CLAUDE_CODE_DUP=second\n"), "{output}");
    assert!(!output.contains("CLAUDE_CODE_DUP=first\n"), "{output}");
    assert!(output.contains("CLAUDE_CODE_EMPTY=\n"), "{output}");
}

#[test]
fn docker_spawn_image_flag_overrides_daemon_default() {
    let harness = RtmHarness::start_with_docker_image("daemon-default:latest");
    let session_id = Uuid::now_v7();
    let output = harness
        .spawn_command(&session_id.to_string(), "claude", "headless", true)
        .arg("--isolation")
        .arg("docker")
        .arg("--image")
        .arg("ghcr.io/org/runtime-matters-claude:1.2.3")
        .output()
        .expect("spawn client");
    spawn_output_ok(output, "claude");

    let image = common::wait_until(Duration::from_secs(5), || {
        let image = common::docker::container_image(&harness, session_id);
        image
            .contains("ghcr.io/org/runtime-matters-claude:1.2.3")
            .then_some(image)
    })
    .unwrap_or_else(|| panic!("container image was not recorded"));
    assert_eq!(image.trim(), "ghcr.io/org/runtime-matters-claude:1.2.3");
}

#[test]
fn spawn_rejects_live_and_terminal_session_id_reuse() {
    let harness = RtmHarness::start();
    let session_id = Uuid::now_v7().to_string();
    spawn_ok(&harness, &session_id, "claude");

    let live_conflict = harness.spawn_runtime(&session_id, "claude");
    assert_spawn_conflict(live_conflict, "SessionId", &session_id, "Running");

    let forced_live_conflict = harness
        .spawn_command(&session_id, "claude", "headless", true)
        .arg("--force")
        .output()
        .expect("forced spawn client");
    assert_spawn_conflict(forced_live_conflict, "SessionId", &session_id, "Running");

    let kill = harness.kill(&session_id, "kill", 0);
    assert!(kill.status.success(), "kill failed: {kill:?}");
    wait_for_status(&harness, &session_id, "state=Exited");

    let terminal_conflict = harness.spawn_runtime(&session_id, "claude");
    assert_spawn_conflict(terminal_conflict, "SessionId", &session_id, "Exited");
}

#[test]
fn validate_target_rpc_reports_headless_and_parse_outcomes() {
    let harness = RtmHarness::start();

    assert_eq!(
        validate_target(&harness, "headless"),
        RuntimeResponse::ValidateTarget(ValidateTargetPayload {
            response: ValidateTargetResponse::valid(),
        })
    );

    let RuntimeResponse::ValidateTarget(payload) = validate_target(&harness, "tmux:not-a-pane")
    else {
        panic!("expected validate target response");
    };
    let response = payload.response;
    assert!(!response.valid);
    assert!(matches!(
        response.outcome,
        ValidateTargetOutcome::InvalidTarget { .. }
    ));

    assert_eq!(
        validate_target(&harness, "ssh:remote"),
        RuntimeResponse::ValidateTarget(ValidateTargetPayload {
            response: ValidateTargetResponse::unsupported_target("ssh:remote"),
        })
    );
}

#[test]
fn validate_target_cli_reports_json_and_human_outcomes() {
    let harness = RtmHarness::start();

    let json_output = harness.cli(&["validate-target", "headless"]);
    assert!(
        json_output.status.success(),
        "validate-target json failed: {json_output:?}"
    );
    assert_eq!(
        serde_json::from_str::<Value>(&output_stdout(json_output)).expect("validate target json"),
        json!({
            "valid": true,
            "outcome": {
                "kind": "valid",
            },
        })
    );

    let human_output = harness.cli(&["validate-target", "garbage", "--format", "human"]);
    assert!(
        !human_output.status.success(),
        "invalid validate-target succeeded: {human_output:?}"
    );
    assert_eq!(
        output_stdout(human_output),
        "garbage: invalid (InvalidTarget)\n"
    );
}

#[test]
fn validate_target_rpc_checks_tmux_liveness_when_available() {
    let Some(tmux_session) = common::tmux::TmuxSession::start("rtm-validate-target") else {
        eprintln!("skipping tmux validate target test because tmux is unavailable");
        return;
    };
    let harness = RtmHarness::start();
    let target = format!("tmux:{}", tmux_session.pane());

    assert_eq!(
        validate_target(&harness, &target),
        RuntimeResponse::ValidateTarget(ValidateTargetPayload {
            response: ValidateTargetResponse::valid(),
        })
    );

    let address = tmux_session.pane();
    tmux_session.kill();

    assert_eq!(
        validate_target(&harness, &target),
        RuntimeResponse::ValidateTarget(ValidateTargetPayload {
            response: ValidateTargetResponse::tmux_pane_dead(
                address.parse().expect("tmux address"),
            ),
        })
    );
}

fn validate_target(harness: &RtmHarness, target: &str) -> RuntimeResponse {
    request_raw(
        harness,
        &RuntimeRpc::ValidateTarget {
            request: ValidateTargetRequest {
                target: target.to_owned(),
            },
        },
    )
}

fn assert_spawn_conflict(
    output: std::process::Output,
    kind: &str,
    session_id: &str,
    identity: &str,
) {
    assert!(!output.status.success(), "spawn unexpectedly succeeded");
    let stderr = output_stderr(output);
    assert!(stderr.contains("spawn conflict"), "{stderr}");
    assert!(stderr.contains(kind), "{stderr}");
    assert!(stderr.contains(session_id), "{stderr}");
    assert!(stderr.contains(identity), "{stderr}");
}

fn spawn_capture_command(session_id: Uuid) -> Command {
    headless_spawn_command(session_id)
}

fn capture_spawn_request(mut command: Command) -> (SpawnRequest, std::process::Output) {
    let temp = tempfile::TempDir::new().expect("temp dir");
    let socket_path = temp.path().join("rtm.sock");
    let listener = UnixListener::bind(&socket_path).expect("bind capture socket");
    listener
        .set_nonblocking(true)
        .expect("nonblocking capture socket");
    let handle = thread::spawn(move || accept_spawn_request(&listener));

    let output = command
        .env("LILO_SOCKET_PATH", &socket_path)
        .output()
        .expect("spawn client");
    let request = handle.join().expect("capture thread");
    (request, output)
}

fn accept_spawn_request(listener: &UnixListener) -> SpawnRequest {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => return read_spawn_request(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(
                    Instant::now() <= deadline,
                    "rtm did not connect to capture socket"
                );
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("accept capture socket: {error}"),
        }
    }
}

fn read_spawn_request(stream: UnixStream) -> SpawnRequest {
    let mut reader = BufReader::new(stream);
    let rpc: LilodRpc = read_json_line_blocking(&mut reader).expect("read captured request");
    write_json_line_blocking(
        reader.get_mut(),
        &RuntimeResponse::error(ErrorCode::SpawnConflict, "captured spawn request"),
    )
    .expect("write captured response");
    let LilodRpc::Runtime(rpc) = rpc else {
        panic!("unexpected captured substrate rpc: {rpc:?}");
    };
    let RuntimeRpc::Spawn { request } = rpc else {
        panic!("unexpected captured rpc: {rpc:?}");
    };
    request
}

fn request_raw(harness: &RtmHarness, rpc: &RuntimeRpc) -> RuntimeResponse {
    let mut stream = UnixStream::connect(harness.socket_path()).expect("connect daemon");
    write_json_line_blocking(&mut stream, &LilodRpc::Runtime(rpc.clone())).expect("write request");
    let mut reader = BufReader::new(stream);
    read_json_line_blocking(&mut reader).expect("read response")
}
