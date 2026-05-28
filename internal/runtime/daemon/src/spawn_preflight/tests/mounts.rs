use std::path::{Path, PathBuf};

use crate::backend::RuntimeBackends;

#[tokio::test]
async fn docker_path_shaped_env_without_mount_is_rejected() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", "/host/path"));

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("missing path mount should fail");

    assert_eq!(
        error.to_string(),
        "path-shaped env CLAUDE_CONFIG_DIR=/host/path is not covered by a declared Docker mount target; add --mount /host/path:/host/path:ro"
    );
}

#[tokio::test]
async fn docker_path_shaped_env_with_same_destination_mount_is_accepted() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let config = path_text(&layout.config);
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", config.clone()));
    request.mounts.push(bind_mount(&layout.config, &config));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "same destination mount should pass",
    )
    .await;
}

#[tokio::test]
async fn docker_path_shaped_env_accepts_subtree_mount_coverage() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let target = path_text(&layout.config);
    let env_value = format!("{target}/subdir");
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", env_value));
    request.mounts.push(bind_mount(&layout.config, &target));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "subtree mount should pass",
    )
    .await;
}

#[tokio::test]
async fn docker_spawn_without_path_shaped_envs_or_mounts_is_accepted() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CODE_OAUTH_TOKEN", "token"));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "spawn without path shaped envs should pass",
    )
    .await;
}

#[tokio::test]
async fn docker_path_shaped_env_relative_value_is_rejected() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", "./local"));

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("relative path env should fail");

    assert_eq!(
        error.to_string(),
        "path-shaped env CLAUDE_CONFIG_DIR=./local must be an absolute container path"
    );
}

#[tokio::test]
async fn docker_duplicate_mount_targets_are_rejected_after_normalization() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let second_config = layout.temp.path().join("config-two");
    std::fs::create_dir_all(&second_config).expect("config two");
    let mut request = docker_request(&layout.cwd);
    request.mounts = vec![
        bind_mount(&layout.config, "/config"),
        bind_mount(&second_config, "/config/."),
    ];

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("duplicate targets should fail");

    assert_eq!(
        error.to_string(),
        "docker mount target /config is declared more than once"
    );
}

#[tokio::test]
async fn docker_mount_source_descendant_of_cwd_is_rejected() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let nested = layout.cwd.join("config");
    std::fs::create_dir_all(&nested).expect("nested config");
    let mut request = docker_request(&layout.cwd);
    request.mounts.push(bind_mount(&nested, "/config"));

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("overlapping source should fail");

    assert!(
        error.to_string().contains("overlaps the cwd auto-mount source"),
        "{error}"
    );
}

#[tokio::test]
async fn docker_mount_source_equal_to_cwd_suppresses_cwd_auto_mount() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request.mounts.push(bind_mount(&layout.cwd, "/project"));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "cwd cover should pass",
    )
    .await;

    let launch = RuntimeBackends::new(state.config())
        .prepare_launch(&request, launch_spec(&request.cwd))
        .expect("prepare launch");

    assert!(!launch.argv.contains(&cwd_auto_mount_arg(&request.cwd)));
    assert_eq!(workdir(&launch.argv), "/project");
}

#[tokio::test]
async fn docker_mount_source_ancestor_of_cwd_remaps_workdir() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let root = layout.temp.path().join("repo");
    let cwd = root.join("littleorgans");
    std::fs::create_dir_all(&cwd).expect("cwd");
    let mut request = docker_request(&cwd);
    request.mounts.push(bind_mount(&root, "/workspace"));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "ancestor cover should pass",
    )
    .await;

    let launch = RuntimeBackends::new(state.config())
        .prepare_launch(&request, launch_spec(&request.cwd))
        .expect("prepare launch");

    assert!(!launch.argv.contains(&cwd_auto_mount_arg(&request.cwd)));
    assert_eq!(workdir(&launch.argv), "/workspace/littleorgans");
}

#[tokio::test]
async fn docker_multiple_cwd_covers_with_equal_precedence_are_rejected() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request.mounts = vec![
        bind_mount(&layout.cwd, "/one"),
        bind_mount(&layout.cwd, "/two"),
    ];

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("equal precedence covers should fail");

    assert!(
        error
            .to_string()
            .contains("multiple docker mount sources cover spawn cwd"),
        "{error}"
    );
}

#[tokio::test]
async fn docker_mount_target_overlapping_cwd_auto_mount_is_rejected() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let target = layout.cwd.canonicalize().expect("canonical cwd").join("config");
    let mut request = docker_request(&layout.cwd);
    request.mounts.push(bind_mount(&layout.config, &path_text(&target)));

    let error = check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect_err("overlapping target should fail");

    assert!(
        error.to_string().contains("overlaps the cwd auto-mount target"),
        "{error}"
    );
}

#[tokio::test]
async fn docker_container_paths_are_normalized_without_host_filesystem_lookup() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", "/missing/root/child/../config"));
    request.mounts.push(bind_mount(&layout.config, "/missing/root"));

    assert_preflight_passes(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
        "container target comparison should be lexical",
    )
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn docker_cwd_cover_uses_canonical_cwd_for_argv() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let real_root = layout.temp.path().join("real-root");
    let real_cwd = real_root.join("project");
    std::fs::create_dir_all(&real_cwd).expect("real cwd");
    let link_root = layout.temp.path().join("link-root");
    std::os::unix::fs::symlink(&real_root, &link_root).expect("symlink root");
    let mut request = docker_request(&link_root.join("project"));
    request.mounts.push(bind_mount(&real_root, "/workspace"));

    check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect("preflight");

    let canonical_cwd = real_cwd.canonicalize().expect("canonical cwd");
    assert_eq!(request.cwd, canonical_cwd);

    let launch = RuntimeBackends::new(state.config())
        .prepare_launch(&request, launch_spec(&request.cwd))
        .expect("prepare launch");

    assert!(!launch.argv.contains(&cwd_auto_mount_arg(&canonical_cwd)));
    assert_eq!(workdir(&launch.argv), "/workspace/project");
}

#[cfg(unix)]
#[tokio::test]
async fn docker_mount_sources_are_canonicalized_before_docker_argv() {
    let layout = MountLayout::new();
    let state = test_state().await;
    let link = layout.cwd.join("config-link");
    std::os::unix::fs::symlink(&layout.config, &link).expect("symlink");
    let mut request = docker_request(&layout.cwd);
    request
        .env
        .push(LaunchEnv::new("CLAUDE_CONFIG_DIR", "/container/config"));
    request
        .mounts
        .push(bind_mount(Path::new("config-link"), "/container/config"));

    check_with_docker_inspector(
        &state,
        &mut request,
        &FakeDockerInspector::available_non_root(),
    )
    .await
    .expect("preflight");

    let canonical = layout.config.canonicalize().expect("canonical config");
    assert_eq!(request.mounts[0].source, canonical);

    let launch = RuntimeBackends::new(state.config())
        .prepare_launch(&request, launch_spec(&layout.cwd))
        .expect("prepare launch");
    let expected = format!(
        "type=bind,source={},target=/container/config,readonly",
        canonical.display()
    );

    assert!(launch.argv.contains(&expected), "{:?}", launch.argv);
}

#[tokio::test]
async fn host_isolation_mounts_do_not_reject_direct_rpc_requests() {
    let state = test_state().await;
    let mut request = headless_request(Uuid::now_v7(), false);
    request.env.push(LaunchEnv::new("CLAUDE_CONFIG_DIR", "/host/path"));
    request.mounts.push(MountSpec {
        source: "missing-source".into(),
        target: "/host/path".into(),
        read_only: true,
    });

    let response = check(&state, &mut request)
        .await
        .expect("host isolation should not reject mounts");

    assert_no_preflight_conflict(response.as_ref());
}

struct MountLayout {
    temp: tempfile::TempDir,
    cwd: PathBuf,
    config: PathBuf,
}

impl MountLayout {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("tempdir");
        let cwd = temp.path().join("work");
        let config = temp.path().join("config");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(&config).expect("config");
        Self { temp, cwd, config }
    }
}

fn docker_request(cwd: &Path) -> SpawnRequest {
    let mut request = headless_request(Uuid::now_v7(), false);
    request.isolation = docker_profile(None);
    request.cwd = cwd.to_path_buf();
    request
}

fn bind_mount(source: &Path, target: &str) -> MountSpec {
    MountSpec {
        source: source.into(),
        target: target.into(),
        read_only: true,
    }
}

fn launch_spec(cwd: &Path) -> LaunchSpec {
    LaunchSpec {
        argv: vec!["claude".to_owned()],
        env: Vec::new(),
        cwd: cwd.to_path_buf(),
        shell_resume: None,
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn cwd_auto_mount_arg(cwd: &Path) -> String {
    let cwd = path_text(cwd);
    format!("type=bind,src={cwd},dst={cwd}")
}

fn workdir(argv: &[String]) -> &str {
    let index = argv
        .iter()
        .position(|arg| arg == "--workdir")
        .expect("workdir flag");
    &argv[index + 1]
}

async fn assert_preflight_passes(
    state: &Arc<ServerState>,
    request: &mut SpawnRequest,
    docker: &FakeDockerInspector,
    message: &'static str,
) {
    let response = check_with_docker_inspector(state, request, docker)
        .await
        .expect(message);

    assert_no_preflight_conflict(response.as_ref());
}

fn assert_no_preflight_conflict(response: Option<&RuntimeResponse>) {
    assert!(response.is_none(), "preflight returned conflict");
}
