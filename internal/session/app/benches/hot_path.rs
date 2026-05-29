use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use lilo_session_core::{MailCheckRequest, RpcResponse, Selector, SessionRpc};
use uuid::Uuid;

#[path = "../tests/common/mod.rs"]
mod common;
use common::OrPanic as _;

const MAIL_CHECK_BUDGET: Duration = Duration::from_millis(50);
const RPC_BUDGET: Duration = Duration::from_millis(5);

fn hot_path_benches(c: &mut Criterion) {
    let runtime_dir = fake_runtime_dir();
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_dir.path());
    let session_id = spawn_bench_agent(&daemon);
    let runtime = tokio::runtime::Runtime::new().or_panic("tokio runtime starts");

    assert_budget("sm mail check cold-start", MAIL_CHECK_BUDGET, || {
        run_mail_check(&daemon, &session_id);
    });
    assert_budget("daemon RPC round-trip", RPC_BUDGET, || {
        run_rpc_round_trip(&runtime, &daemon, session_id);
    });

    c.bench_function("sm mail check cold-start", |bench| {
        bench.iter(|| black_box(run_mail_check(&daemon, &session_id)));
    });
    c.bench_function("daemon RPC round-trip", |bench| {
        bench.iter(|| black_box(run_rpc_round_trip(&runtime, &daemon, session_id)));
    });
}

fn assert_budget<F>(name: &str, budget: Duration, mut run: F)
where
    F: FnMut(),
{
    let mut samples = (0..5)
        .map(|_| {
            let started = Instant::now();
            run();
            started.elapsed()
        })
        .collect::<Vec<_>>();
    samples.sort();
    let median = samples[samples.len() / 2];
    assert!(
        median <= budget,
        "{name} median {median:?} exceeds budget {budget:?}"
    );
}

fn run_mail_check(daemon: &common::DaemonFixture, session_id: &Uuid) -> usize {
    let output = daemon
        .command()
        .args(["mail", "check", "--selector", &session_id.to_string()])
        .output()
        .or_panic("sm mail check runs");
    assert!(
        output.status.success(),
        "sm mail check failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).or_panic("stdout is utf8");
    assert_eq!(stdout.trim(), "0 unread");
    0
}

fn run_rpc_round_trip(
    runtime: &tokio::runtime::Runtime,
    daemon: &common::DaemonFixture,
    session_id: Uuid,
) -> usize {
    let request = SessionRpc::MailCheck {
        request: MailCheckRequest {
            selector: Selector::Id { id: session_id },
        },
    };
    let endpoint = lilo_paths::DaemonEndpoint::unix_socket(daemon.socket_path());
    let response = runtime
        .block_on(lilo_session_daemon::send_request(&endpoint, &request))
        .or_panic("daemon RPC succeeds");
    match response {
        RpcResponse::MailChecked { response } => response.unread,
        other => panic!("unexpected daemon response: {other:?}"),
    }
}

fn spawn_bench_agent(daemon: &common::DaemonFixture) -> Uuid {
    let output = daemon
        .command()
        .args(["run", "codex", "--role", "bench", "--dir", "bench"])
        .output()
        .or_panic("sm run starts");
    assert!(
        output.status.success(),
        "sm run failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).or_panic("stdout is utf8");
    let id = stdout
        .split_whitespace()
        .next()
        .or_panic("session id is printed");
    Uuid::parse_str(id).or_panic("session id is a uuid")
}

fn fake_runtime_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().or_panic("fake runtime dir creates");
    let runtime = dir.path().join("codex");
    fs::write(
        &runtime,
        "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile true; do sleep 1; done\n",
    )
    .or_panic("fake runtime writes");
    let mut permissions = fs::metadata(&runtime)
        .or_panic("fake runtime metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&runtime, permissions).or_panic("fake runtime executable");
    dir
}

criterion_group!(benches, hot_path_benches);
criterion_main!(benches);
