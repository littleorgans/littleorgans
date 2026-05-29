mod common;
use common::{LOCAL_UID, TestDaemon, handle_spawn, headless_spawn_request, local_context};
use lilo_session_core::RpcResponse;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::time::{Duration, Instant};

fn pid_alive(pid: u32) -> bool {
    // Signal 0 performs an existence/permission check without delivering a signal.
    i32::try_from(pid)
        .ok()
        .is_some_and(|pid| kill(Pid::from_raw(pid), None).is_ok())
}

#[tokio::test]
async fn daemon_drain_reaps_spawned_shim() {
    // Regression: a spawned `lilo __runtime-shim` (and its runtime child) must not
    // outlive the daemon. Before the fix these accumulated as live orphans
    // across every test run. Draining SIGTERMs the shim, which forwards
    // SIGTERM to its runtime child and reaps it, tearing down the subtree.
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let request = headless_spawn_request("engineer", daemon.dir.path().display().to_string());
    let RpcResponse::Spawned { response } = handle_spawn(&daemon, context, request).await.response
    else {
        panic!("expected spawn response");
    };
    let runtime_pid = response.session.runtime_pid;
    assert!(runtime_pid > 0, "spawn should report a runtime pid");
    assert!(
        pid_alive(runtime_pid),
        "runtime child {runtime_pid} should be alive after spawn"
    );

    daemon.runtime.drain_shims();

    let deadline = Instant::now() + Duration::from_secs(5);
    while pid_alive(runtime_pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !pid_alive(runtime_pid),
        "runtime child {runtime_pid} survived daemon drain"
    );
}
