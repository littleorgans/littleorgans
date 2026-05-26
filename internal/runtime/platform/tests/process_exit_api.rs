use lilo_runtime_platform::process_exit::{ProcessExitWatcher, watch_process_exit};
use tokio::sync::oneshot;

#[test]
fn process_exit_watcher_api_is_platform_neutral() {
    let _: fn(u32) -> anyhow::Result<(ProcessExitWatcher, oneshot::Receiver<()>)> =
        watch_process_exit;
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn linux_process_exit_watcher_fires_for_child_exit() {
    let mut child = tokio::process::Command::new("/bin/sh")
        .arg("-c")
        .arg("sleep 0.1")
        .spawn()
        .expect("spawn child");
    let pid = child.id().expect("child pid");
    let (_watcher, exit_rx) = watch_process_exit(pid).expect("watch child");

    tokio::time::timeout(std::time::Duration::from_secs(2), exit_rx)
        .await
        .expect("watcher timed out")
        .expect("watcher sender dropped");
    child.wait().await.expect("reap child");
}
