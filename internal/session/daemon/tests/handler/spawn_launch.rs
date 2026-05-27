use crate::common::{
    LOCAL_UID, OrPanic as _, TestDaemon, launch_env, local_context, spawn_test_session,
};
use lilo_session_core::{
    IsolationPolicy, MountSpec, Namespace, RpcResponse, RuntimeKind, SessionRpc, SpawnRequest,
};

#[tokio::test]
pub(crate) async fn spawn_launch_fields_reach_spawn_driver() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let shell_resume = lilo_rm_core::ShellResume {
        argv: vec!["/bin/zsh".to_string()],
        env: vec![launch_env("TERM", "xterm-256color")],
        cwd: daemon.dir.path().to_path_buf(),
    };
    let isolation = IsolationPolicy::Docker(lilo_rm_core::IsolationProfile::default());
    let image = Some("runtime-matters-claude:local".to_string());
    let mounts = vec![MountSpec {
        source: "/host/config".into(),
        target: "/container/config".into(),
        read_only: true,
    }];

    let spawned = daemon
        .state
        .handle(
            context,
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: daemon.dir.path().display().to_string(),
                    dir: None,
                    namespace: None,
                    target: "tmux:test:0.0".to_string(),
                    agent_config: None,
                    isolation: isolation.clone(),
                    image: image.clone(),
                    env: vec![
                        launch_env("HOME", "/Users/tester"),
                        launch_env("PATH", "/opt/node/bin:/usr/bin"),
                    ],
                    mounts: mounts.clone(),
                    shell_resume: Some(shell_resume.clone()),
                    labels: Vec::new(),
                    force: true,
                }),
            },
        )
        .await;

    let RpcResponse::Spawned { .. } = spawned.response else {
        panic!("expected spawn response");
    };
    let launch = daemon.driver.launches().pop().or_panic("driver saw launch");
    assert_eq!(launch.isolation, isolation);
    assert_eq!(launch.image, image);
    assert_eq!(launch.mounts, mounts);
    assert!(launch.force);
    assert!(launch.env.contains(&launch_env("HOME", "/Users/tester")));
    assert!(
        launch
            .env
            .contains(&launch_env("PATH", "/opt/node/bin:/usr/bin"))
    );
    assert_eq!(launch.shell_resume, Some(shell_resume));
}

#[tokio::test]
pub(crate) async fn spawn_launch_cwd_is_request_workspace() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let session = spawn_test_session(&daemon, &local_context(), "pm").await;
    let launch = daemon.driver.launches().pop().or_panic("driver saw launch");
    assert_eq!(launch.cwd, daemon.dir.path());
    assert!(!launch.force);
    assert_eq!(session.namespace, Namespace::default());
    assert_eq!(session.dir, daemon.dir.path());
}
