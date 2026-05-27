use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use std::str::FromStr;

use lilo_rm_core::{IsolationPolicy, MountSpec, SpawnTarget};
use lilo_session_core::{
    Label, Namespace, RpcResponse, SessionRpc, SmEndpoint, SpawnRequest,
    agent_config_uses_home_prefix, is_agent_config_path_like, normalize_agent_config_request,
};

use crate::cli::cli_def::{RunArgs, SessionCreateArgs};
use crate::cli::namespace_resolver::resolve_namespace_dir;
use crate::cli::output::print_session_line;

pub async fn run(args: RunArgs) -> Result<()> {
    let isolation = args.isolation.unwrap_or_default();
    reject_host_mounts(&isolation, &args.mounts)?;
    spawn_session(
        args.session,
        args.target,
        args.force,
        isolation,
        args.image,
        args.mounts,
    )
    .await
}

pub async fn create_session(args: SessionCreateArgs) -> Result<()> {
    spawn_session(
        args,
        "headless".to_string(),
        false,
        IsolationPolicy::default(),
        None,
        Vec::new(),
    )
    .await
}

async fn spawn_session(
    args: SessionCreateArgs,
    target: String,
    force: bool,
    isolation: IsolationPolicy,
    image: Option<String>,
    mounts: Vec<MountSpec>,
) -> Result<()> {
    let spawn_location = resolve_spawn_location(args.dir.as_ref(), args.namespace.clone())?;
    let agent_config = normalize_cli_agent_config(args.agent_config.as_deref())?;
    let endpoint = SmEndpoint::from_env()?;
    let env = lilo_rm_core::capture_caller_env();
    let spawn_target = SpawnTarget::from_str(&target).ok();
    let shell_resume = if spawn_target
        .as_ref()
        .and_then(SpawnTarget::tmux_address)
        .is_some()
    {
        Some(lilo_rm_core::capture_shell_resume(
            lilo_rm_core::capture_caller_cwd()?,
        ))
    } else {
        None
    };
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &SessionRpc::Spawn {
            request: Box::new(SpawnRequest {
                runtime: args.runtime,
                role: args.role,
                workspace: spawn_location.dir.clone(),
                dir: Some(spawn_location.dir),
                namespace: Some(spawn_location.namespace),
                target,
                agent_config,
                isolation,
                image,
                env,
                mounts,
                shell_resume,
                labels: args
                    .labels
                    .iter()
                    .map(|label| Label::from_str(label))
                    .collect::<Result<Vec<_>, _>>()?,
                force,
            }),
        },
    )
    .await?;

    match response {
        RpcResponse::Spawned { response } => {
            print_session_line(&response.session, false);
            Ok(())
        }
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
}

fn reject_host_mounts(isolation: &IsolationPolicy, mounts: &[MountSpec]) -> Result<()> {
    if isolation.is_host() && !mounts.is_empty() {
        bail!("--mount is docker-only and cannot be used with --isolation host");
    }
    Ok(())
}

fn normalize_cli_agent_config(agent_config: Option<&str>) -> Result<Option<String>> {
    let Some(agent_config) = agent_config else {
        return Ok(None);
    };
    if !is_agent_config_path_like(agent_config) {
        return Ok(Some(agent_config.to_string()));
    }
    let cwd = lilo_rm_core::capture_caller_cwd()
        .context("cannot read current directory to resolve --agent-config")?;
    let home = home_for_agent_config(agent_config)?;
    Ok(Some(normalize_agent_config_request(
        agent_config,
        &cwd,
        home.as_deref(),
    )))
}

fn home_for_agent_config(agent_config: &str) -> Result<Option<PathBuf>> {
    if !agent_config_uses_home_prefix(agent_config) {
        return Ok(None);
    }
    let Some(home) = std::env::var_os("HOME") else {
        bail!("HOME is required to expand --agent-config path {agent_config}");
    };
    Ok(Some(PathBuf::from(home)))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpawnLocation {
    namespace: Namespace,
    dir: String,
}

fn resolve_spawn_location(
    dir: Option<&PathBuf>,
    namespace: Option<Namespace>,
) -> Result<SpawnLocation> {
    let start_dir = match dir {
        Some(dir) => dir.clone(),
        None => {
            std::env::current_dir().context("cannot read current directory to resolve --dir")?
        }
    };
    let (namespace, canonical_dir) = resolve_namespace_dir(&start_dir, namespace)?.into_pair();

    Ok(SpawnLocation {
        namespace,
        dir: canonical_dir.display().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use crate::test_support::OrPanic as _;
    use std::sync::{Mutex, OnceLock};

    use super::*;

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn explicit_namespace_uses_dir_as_canonical_spawn_dir() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).or_panic("project dir");
        let namespace = Namespace::new("alpha").or_panic("namespace");

        let resolved =
            resolve_spawn_location(Some(&project), Some(namespace.clone())).or_panic("resolves");

        assert_eq!(resolved.namespace, namespace);
        assert_eq!(
            PathBuf::from(resolved.dir),
            std::fs::canonicalize(project).or_panic("canonical project")
        );
    }

    #[test]
    fn marker_from_dir_is_ignored_and_dir_is_canonicalized() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let project = root.path().join("project");
        let child = project.join("child");
        std::fs::create_dir_all(project.join(".sm")).or_panic("marker parent");
        std::fs::create_dir_all(&child).or_panic("child dir");
        std::fs::write(project.join(".sm").join("namespace"), "marker").or_panic("marker");

        let resolved = with_isolated_namespace_env(root.path().join("sm-home"), || {
            resolve_spawn_location(Some(&child), None).or_panic("resolves dir")
        });

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            PathBuf::from(resolved.dir),
            std::fs::canonicalize(child).or_panic("canonical child")
        );
    }

    #[test]
    fn marker_from_cwd_is_ignored_and_cwd_is_canonicalized() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let project = root.path().join("project");
        let child = project.join("child");
        std::fs::create_dir_all(project.join(".sm")).or_panic("marker parent");
        std::fs::create_dir_all(&child).or_panic("child dir");
        std::fs::write(project.join(".sm").join("namespace"), "cwd-marker").or_panic("marker");
        let saved = std::env::current_dir().or_panic("cwd");
        let resolved = with_isolated_namespace_env(root.path().join("sm-home"), || {
            std::env::set_current_dir(&child).or_panic("chdir");
            let resolved = resolve_spawn_location(None, None).or_panic("resolves cwd");
            std::env::set_current_dir(saved).or_panic("restore cwd");
            resolved
        });
        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            PathBuf::from(resolved.dir),
            std::fs::canonicalize(child).or_panic("canonical child")
        );
    }

    #[test]
    fn falls_back_to_default_namespace() {
        let project = tempfile::tempdir().or_panic("tempdir");

        let resolved = with_isolated_namespace_env(project.path().join("sm-home"), || {
            let project = project.path().to_path_buf();
            resolve_spawn_location(Some(&project), None).or_panic("resolves default")
        });

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            PathBuf::from(resolved.dir),
            std::fs::canonicalize(project.path()).or_panic("canonical project")
        );
    }

    // Rust 2024 marks process env mutation unsafe. The lock keeps these env
    // changes scoped and serial for namespace resolution tests.
    #[allow(unsafe_code)]
    fn with_isolated_namespace_env<T>(lilo_home: PathBuf, test: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .or_panic("env lock");
        let original_lilo_home = std::env::var_os("LILO_HOME");

        // SAFETY: ENV_LOCK serializes namespace tests, so no other thread reads
        // or writes LILO_HOME concurrently; originals are restored
        // before the guard drops.
        unsafe {
            std::env::set_var("LILO_HOME", lilo_home);
        }
        let result = test();
        restore_env("LILO_HOME", original_lilo_home);
        result
    }

    #[allow(unsafe_code)]
    fn restore_env(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            // SAFETY: called from inside `with_isolated_namespace_env` while
            // ENV_LOCK is held; no other thread can race on this variable.
            Some(value) => unsafe { std::env::set_var(name, value) },
            // SAFETY: see above; ENV_LOCK held for the entire restore window.
            None => unsafe { std::env::remove_var(name) },
        }
    }
}
