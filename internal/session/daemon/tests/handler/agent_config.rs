use std::ffi::OsString;
use std::path::Path;

use crate::common::{LOCAL_UID, OrPanic as _, TestDaemon, launch_env, local_context};
use lilo_session_core::{
    IsolationPolicy, ListRequest, RpcResponse, RuntimeKind, Selector, SessionRpc, SpawnRequest,
};

#[tokio::test]
pub(crate) async fn agent_config_env_reaches_spawn_driver() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let config = daemon.dir.path().join("agent.toml");
    std::fs::write(
        &config,
        "claude_config_dir = \"/tmp/demo-claude\"\n[env]\nHELIOY_AGENT_NAME = \"demo\"\n",
    )
    .or_panic("agent config writes");

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
                    target: "headless".to_string(),
                    agent_config: Some(config.display().to_string()),
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
        )
        .await;

    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    assert_eq!(
        response.session.agent_config,
        Some(config.display().to_string())
    );
    let launch = daemon.driver.launches().pop().or_panic("driver saw launch");
    assert!(
        launch
            .env
            .contains(&launch_env("CLAUDE_CONFIG_DIR", "/tmp/demo-claude"))
    );
    assert!(
        launch
            .env
            .contains(&launch_env("HELIOY_AGENT_NAME", "demo"))
    );
    assert_eq!(launch.isolation, IsolationPolicy::Host);
    assert_eq!(launch.image, None);
    assert!(launch.mounts.is_empty());
    assert!(launch.env.contains(&launch_env(
        "HELIOY_SESSION_ID",
        &response.session.id.to_string()
    )));
}

#[tokio::test]
pub(crate) async fn named_agent_config_persists_resolved_path() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let home = tempfile::tempdir().or_panic("home tempdir creates");
    let config_dir = home.path().join(".agm").join("demo-agent");
    std::fs::create_dir_all(&config_dir).or_panic("agent config dir creates");
    let config = config_dir.join("agent.toml");
    std::fs::write(&config, "[env]\nHELIOY_AGENT_NAME = \"demo\"\n")
        .or_panic("agent config writes");
    let _home = set_home_for_test(home.path());

    let spawned = daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "pm".to_string(),
                    workspace: daemon.dir.path().display().to_string(),
                    dir: None,
                    namespace: None,
                    target: "headless".to_string(),
                    agent_config: Some("demo-agent".to_string()),
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: vec![launch_env("HOME", "/Users/tester")],
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
        )
        .await;

    let RpcResponse::Spawned { response } = spawned.response else {
        panic!("expected spawn response");
    };
    let expected_path = config.display().to_string();
    assert_eq!(response.session.agent_config, Some(expected_path.clone()));
    assert_ne!(response.session.agent_config.as_deref(), Some("demo-agent"));

    let listed = daemon
        .state
        .handle(
            context,
            SessionRpc::List {
                request: ListRequest {
                    selector: Some(Selector::Id {
                        id: response.session.id,
                    }),
                },
            },
        )
        .await;
    let RpcResponse::Listed { response } = listed.response else {
        panic!("expected list response");
    };

    assert_eq!(response.sessions.len(), 1);
    assert_eq!(response.sessions[0].agent_config, Some(expected_path));
}

pub(crate) struct HomeEnvGuard {
    original: Option<OsString>,
}

// Rust 2024 marks process env mutation unsafe. This guard keeps it scoped to
// the named agent config test and restores the original value on drop.
#[allow(unsafe_code)]
impl Drop for HomeEnvGuard {
    fn drop(&mut self) {
        match self.original.take() {
            // SAFETY: HomeEnvGuard is only constructed by `set_home_for_test`,
            // which is invoked from #[tokio::test] bodies that do not spawn
            // other threads touching HOME; the guard restores the original
            // value before any subsequent test reads it.
            Some(value) => unsafe { std::env::set_var("HOME", value) },
            // SAFETY: see above; HOME was unset originally, so removing on
            // drop returns the process env to its pre-test state.
            None => unsafe { std::env::remove_var("HOME") },
        }
    }
}

#[allow(unsafe_code)]
pub(crate) fn set_home_for_test(home: &Path) -> HomeEnvGuard {
    let guard = HomeEnvGuard {
        original: std::env::var_os("HOME"),
    };
    // SAFETY: invoked from #[tokio::test] bodies; no other thread mutates
    // HOME concurrently and `HomeEnvGuard::drop` restores the original
    // value before the test returns.
    unsafe {
        std::env::set_var("HOME", home.as_os_str());
    }
    guard
}
