use std::ffi::OsString;
use std::path::PathBuf;

use crate::settings::Settings;

/// Operator state root override.
pub const LILO_HOME: &str = "LILO_HOME";
/// Operator daemon socket override.
pub const LILO_SOCKET_PATH: &str = "LILO_SOCKET_PATH";
/// Operator logging filter.
pub const LILO_LOG: &str = "LILO_LOG";
/// Operator logging formatter override.
pub const LILO_LOG_FORMAT: &str = "LILO_LOG_FORMAT";
/// Operator default Docker image.
pub const LILO_DOCKER_IMAGE: &str = "LILO_DOCKER_IMAGE";
/// Operator escape hatch for root-user Docker images.
pub const LILO_DOCKER_ALLOW_ROOT_IMAGE_USER: &str = "LILO_DOCKER_ALLOW_ROOT_IMAGE_USER";
/// Operator escape hatch for Docker arm64 manifest checks.
pub const LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE: &str = "LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE";
/// Operator liveness probe sweep override in milliseconds.
pub const LILO_PROBE_SWEEP_INTERVAL_MS: &str = "LILO_PROBE_SWEEP_INTERVAL_MS";
/// Operator resume poll interval override in milliseconds.
pub const LILO_RESUME_POLL_INTERVAL_MS: &str = "LILO_RESUME_POLL_INTERVAL_MS";
/// Operator resume gap threshold override in milliseconds.
pub const LILO_RESUME_GAP_THRESHOLD_MS: &str = "LILO_RESUME_GAP_THRESHOLD_MS";
/// Operator tmux server label override.
pub const LILO_TMUX_SERVER_LABEL: &str = "LILO_TMUX_SERVER_LABEL";
/// Operator Postgres database connection string.
pub const LILO_DATABASE_URL: &str = "LILO_DATABASE_URL";
/// Operator Compose host port for the local Postgres service (Compose-only).
pub const LILO_DATABASE_DOCKER_PORT: &str = "LILO_DATABASE_DOCKER_PORT";
/// Agent identity namespace prefix.
pub const LILO_AGENT_PREFIX: &str = "LILO_AGENT_";
/// Agent session id injected into child processes.
pub const LILO_AGENT_SESSION_ID: &str = "LILO_AGENT_SESSION_ID";
/// Agent runtime kind injected into child processes.
pub const LILO_AGENT_RUNTIME: &str = "LILO_AGENT_RUNTIME";
/// Agent role injected into child processes.
pub const LILO_AGENT_ROLE: &str = "LILO_AGENT_ROLE";
/// Agent workspace injected into child processes.
pub const LILO_AGENT_WORKSPACE: &str = "LILO_AGENT_WORKSPACE";
/// Build-time CLI version.
pub const LILO_CLI_VERSION: &str = "LILO_CLI_VERSION";
/// Build-time git sha.
pub const LILO_GIT_SHA: &str = "LILO_GIT_SHA";
/// Build-time git sha inclusion toggle.
pub const LILO_VERSION_INCLUDE_GIT_SHA: &str = "LILO_VERSION_INCLUDE_GIT_SHA";
/// GitHub token passthrough.
pub const LILO_GITHUB_PAT: &str = "LILO_GITHUB_PAT";
/// Local development binary override.
pub const LILO_DEV_BIN: &str = "LILO_DEV_BIN";
/// Test sentinel for allowed env passthrough.
pub const LILO_TEST_ALLOWED_SENTINEL: &str = "LILO_TEST_ALLOWED_SENTINEL";
/// Test sentinel for non-UTF-8 env values.
pub const LILO_TEST_BAD_BYTES: &str = "LILO_TEST_BAD_BYTES";
/// Test benchmark binary override.
pub const LILO_TEST_BENCH_BIN: &str = "LILO_TEST_BENCH_BIN";
/// Test benchmark sample-count override.
pub const LILO_TEST_BENCH_SAMPLES: &str = "LILO_TEST_BENCH_SAMPLES";
/// Test binary override.
pub const LILO_TEST_BIN: &str = "LILO_TEST_BIN";
/// Test admin/maintenance Postgres URL for test database provisioning.
pub const LILO_TEST_DATABASE_URL: &str = "LILO_TEST_DATABASE_URL";
/// Test Docker end-to-end toggle.
pub const LILO_TEST_E2E_DOCKER: &str = "LILO_TEST_E2E_DOCKER";
/// Test env passthrough sentinel.
pub const LILO_TEST_ENV: &str = "LILO_TEST_ENV";
/// Test namespace-binding fault injection toggle.
pub const LILO_TEST_FAULT_NAMESPACE_BINDING_CLEAR: &str = "LILO_TEST_FAULT_NAMESPACE_BINDING_CLEAR";
/// Test missing-env dynamic-name prefix.
pub const LILO_TEST_MISSING_PREFIX: &str = "LILO_TEST_MISSING_";
/// Test fake-runtime print-env toggle.
pub const LILO_TEST_PRINT_ENV: &str = "LILO_TEST_PRINT_ENV";
/// Test sentinel for inherited env clearing.
pub const LILO_TEST_PRE_EXISTING_SENTINEL: &str = "LILO_TEST_PRE_EXISTING_SENTINEL";
/// Test quote escaping sentinel.
pub const LILO_TEST_QUOTE: &str = "LILO_TEST_QUOTE";
/// Test shell-resume sentinel.
pub const LILO_TEST_SHELL_RESUME_SENTINEL: &str = "LILO_TEST_SHELL_RESUME_SENTINEL";
/// Test stdio sentinel.
pub const LILO_TEST_STDIO_SENTINELS: &str = "LILO_TEST_STDIO_SENTINELS";
/// Test TUI exit-window sentinel.
pub const LILO_TEST_TUI_EXIT_WINDOW: &str = "LILO_TEST_TUI_EXIT_WINDOW";

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    non_empty_env(name).map(PathBuf::from)
}

/// Operator Postgres connection string (`LILO_DATABASE_URL`). Empty is unset.
pub fn database_url() -> Option<String> {
    non_empty_string(LILO_DATABASE_URL)
}

/// Admin/maintenance Postgres URL used to provision test databases
/// (`LILO_TEST_DATABASE_URL`). Empty is unset.
pub fn test_database_url() -> Option<String> {
    non_empty_string(LILO_TEST_DATABASE_URL)
}

/// Resolve the operator database URL: `LILO_DATABASE_URL` over
/// `settings.database.url`. Returns `None` when neither is set.
pub fn resolve_database_url(settings: &Settings) -> Option<String> {
    database_url().or_else(|| settings.database.url.clone())
}

/// Resolve the test/admin database URL: `LILO_TEST_DATABASE_URL` over
/// `LILO_DATABASE_URL` over `settings.database.test_url` over
/// `settings.database.url`. Returns `None` when nothing is set.
pub fn resolve_test_database_url(settings: &Settings) -> Option<String> {
    test_database_url()
        .or_else(database_url)
        .or_else(|| settings.database.test_url.clone())
        .or_else(|| settings.database.url.clone())
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

fn non_empty_string(name: &str) -> Option<String> {
    non_empty_env(name).map(|value| value.to_string_lossy().into_owned())
}
