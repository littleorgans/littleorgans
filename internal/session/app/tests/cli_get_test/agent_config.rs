use crate::common::{self, OrPanic as _};
use crate::{assert_success, canonical_display, first_field, get_session_json, stderr};

#[test]
pub(crate) fn run_agent_config_paths_are_canonicalized_from_caller_context() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let caller = daemon.dir.path().join("caller");
    let workspace = daemon.dir.path().join("workspace");
    let home = daemon.dir.path().join("caller-home");
    std::fs::create_dir_all(&caller).or_panic("caller dir");
    std::fs::create_dir_all(&workspace).or_panic("workspace dir");
    std::fs::create_dir_all(&home).or_panic("home dir");
    let config = caller.join("agent.toml");
    std::fs::write(&config, "[env]\nHELIOY_AGENT_NAME = \"cli\"\n").or_panic("agent config");

    let run = daemon
        .command()
        .current_dir(&caller)
        .env("HOME", &home)
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &workspace.display().to_string(),
            "--agent-config",
            "./agent.toml",
            "--detach",
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &run);

    let session = get_session_json(&daemon, &first_field(&run.stdout));
    assert_eq!(session["agent_config"], canonical_display(&config));

    let missing = daemon
        .command()
        .current_dir(&caller)
        .env("HOME", &home)
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &workspace.display().to_string(),
            "--agent-config",
            "~/missing.toml",
            "--detach",
        ])
        .output()
        .or_panic("sm run executes");
    assert!(!missing.status.success());
    assert!(stderr(&missing).contains(&home.join("missing.toml").display().to_string()));
}

#[test]
pub(crate) fn run_missing_named_agent_config_surfaces_resolved_path() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let workspace = daemon.dir.path().join("workspace");
    std::fs::create_dir_all(&workspace).or_panic("workspace dir");

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "x",
            "--dir",
            &workspace.display().to_string(),
            "--agent-config",
            "does-not-exist",
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!run.status.success());
    let stderr = stderr(&run);
    assert!(stderr.contains("agent config not found: does-not-exist"));
    assert!(stderr.contains("looked for"));
    assert!(stderr.contains("config/session/agents/does-not-exist/agent.toml"));
    assert!(!stderr.contains(".agm"));
}
