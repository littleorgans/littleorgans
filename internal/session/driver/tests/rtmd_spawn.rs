mod common;

use std::path::PathBuf;

use common::OrPanic as _;
use lilo_rm_core::{
    IsolationPolicy, LaunchEnv, Lifecycle, LifecycleState, LostEvidence, MountSpec, RuntimeEvent,
    RuntimeKind, RuntimeResponse, RuntimeRpc, ShellResume, SpawnRequest, SpawnedPayload,
    read_json_line, write_json_line,
};
use lilo_session_core::RuntimeKind as SmRuntimeKind;
use lilo_session_driver::{RtmdDriver, SpawnDriver, SpawnLaunch};
use tokio::io::BufReader;
use tokio::net::UnixListener;
use uuid::Uuid;

#[tokio::test]
async fn rtmd_spawn_forwards_env_shell_resume_and_force_enabled() {
    rtmd_spawn_forwards_env_shell_resume_and_force(true).await;
}

#[tokio::test]
async fn rtmd_spawn_forwards_force_disabled() {
    rtmd_spawn_forwards_env_shell_resume_and_force(false).await;
}

async fn rtmd_spawn_forwards_env_shell_resume_and_force(force: bool) {
    let session_id = Uuid::now_v7();
    let tempdir = tempfile::tempdir().or_panic("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = UnixListener::bind(&socket_path).or_panic("bind test socket");
    let driver = RtmdDriver::new(socket_path);
    let shell_resume = ShellResume {
        argv: vec!["/bin/zsh".to_string()],
        env: vec![LaunchEnv::new("TERM", "xterm-256color")],
        cwd: PathBuf::from("/tmp/session"),
    };
    let isolation = IsolationPolicy::Docker(lilo_rm_core::IsolationProfile::default());
    let image = Some("runtime-matters-claude:local".to_string());
    let mounts = vec![MountSpec {
        source: "/host/config".into(),
        target: "/container/config".into(),
        read_only: true,
    }];

    let server = tokio::spawn({
        let shell_resume = shell_resume.clone();
        let isolation = isolation.clone();
        let image = image.clone();
        let mounts = mounts.clone();
        async move {
            let _tempdir = tempdir;
            let (stream, _) = listener.accept().await.or_panic("accept client");
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let rpc: RuntimeRpc = read_json_line(&mut reader).await.or_panic("read rpc");
            let RuntimeRpc::Spawn { request } = rpc else {
                panic!("expected spawn rpc");
            };
            assert_eq!(request.env, vec![LaunchEnv::new("HOME", "/Users/tester")]);
            assert_eq!(request.isolation, isolation);
            assert_eq!(request.image, image);
            assert_eq!(request.mounts, mounts);
            assert_eq!(request.shell_resume, Some(shell_resume));
            assert_eq!(request.force, force);
            write_json_line(
                &mut write_half,
                &RuntimeResponse::Spawned(spawned(&request)),
            )
            .await
            .or_panic("write response");
        }
    });

    driver
        .spawn(
            &session_id.to_string(),
            &SpawnLaunch {
                runtime: SmRuntimeKind::Claude,
                isolation,
                image,
                cwd: PathBuf::from("/tmp/session"),
                target: "headless".to_string(),
                env: vec![LaunchEnv::new("HOME", "/Users/tester")],
                mounts,
                shell_resume: Some(shell_resume),
                force,
            },
        )
        .await
        .or_panic("spawn delegates to rtmd");
    server.await.or_panic("server task");
}

fn spawned(request: &SpawnRequest) -> SpawnedPayload {
    let lifecycle = Lifecycle {
        session_id: request.session_id,
        runtime: RuntimeKind::Claude,
        isolation: IsolationPolicy::default(),
        state: LifecycleState::Running,
        shim_pid: None,
        runtime_pid: Some(42),
        start_time: None,
        tmux_pane: None,
        log_availability: None,
    };
    SpawnedPayload {
        lifecycle,
        event: RuntimeEvent::Lost {
            session_id: request.session_id,
            evidence: LostEvidence::PidNotAlive,
        },
        log_dir: None,
        stdout_path: None,
        stderr_path: None,
    }
}
