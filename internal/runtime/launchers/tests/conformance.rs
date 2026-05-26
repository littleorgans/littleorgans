use lilo_rm_core::{
    HeadlessSpawnTarget, IsolationPolicy, RuntimeKind, RuntimeLauncher, SpawnRequest, SpawnTarget,
};
use uuid::Uuid;

#[test]
fn registered_launchers_return_argv_and_env() {
    for launcher in lilo_runtime_launchers::registered_launchers() {
        assert_launcher_conforms(launcher);
    }
}

#[test]
fn unknown_runtime_kind_is_not_registered() {
    let Err(error) =
        lilo_runtime_launchers::dispatch(&RuntimeKind::Other("nonexistent".to_owned()))
    else {
        panic!("unknown launcher resolved");
    };

    assert_eq!(
        error.to_string(),
        "no launcher registered for runtime kind: nonexistent"
    );
}

fn assert_launcher_conforms(launcher: &'static dyn RuntimeLauncher) {
    let request = SpawnRequest {
        session_id: Uuid::now_v7(),
        runtime: launcher.kind(),
        isolation: IsolationPolicy::default(),
        image: None,
        env: Vec::new(),
        mounts: Vec::new(),
        cwd: lilo_rm_core::launcher_probe_cwd(),
        target: SpawnTarget::Headless(HeadlessSpawnTarget {}),
        force: false,
        shell_resume: None,
    };

    let argv = launcher.argv(&request).expect("argv");
    assert!(!argv.is_empty(), "argv should not be empty");
    assert_eq!(argv.len(), 1, "launcher argv shape changed");
    assert_eq!(
        std::path::Path::new(&argv[0]).file_name(),
        Some(std::ffi::OsStr::new(expected_binary(&request.runtime))),
        "launcher command should resolve the expected binary"
    );

    let env = launcher.env(&request).expect("env");
    assert!(!env.is_empty(), "env should not be empty");
    assert!(
        env.iter().any(|entry| entry.key == "HELIOY_SESSION_ID"
            && entry.value == request.session_id.to_string()),
        "HELIOY_SESSION_ID should be present"
    );
    assert!(
        env.iter().any(
            |entry| entry.key == "HELIOY_RUNTIME" && entry.value == request.runtime.to_string()
        ),
        "HELIOY_RUNTIME should be present"
    );
}

fn expected_binary(runtime: &RuntimeKind) -> &'static str {
    match runtime {
        RuntimeKind::Claude => "claude",
        RuntimeKind::Codex => "codex",
        RuntimeKind::Other(_) => panic!("unexpected unknown launcher"),
    }
}
