use anyhow::{Context, Result, bail};
use clap::Args;
use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{
    IsolationPolicy, LaunchEnv, MountSpec, RuntimeKind, RuntimeResponse, SpawnRequest, SpawnTarget,
    ensure_mounts_allowed_for_isolation, upsert_launch_env,
};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::cli::output;

#[derive(Debug, Args)]
pub struct SpawnArgs {
    #[command(flatten)]
    output: output::OutputArgs,
    #[arg(long)]
    runtime: RuntimeKind,
    #[arg(long)]
    session_id: Uuid,
    #[arg(long, value_name = "headless|tmux:SESSION:WINDOW.PANE")]
    target: SpawnTarget,
    #[arg(long, default_value_t = IsolationPolicy::Host, value_name = "host|docker[:PROFILE]")]
    isolation: IsolationPolicy,
    #[arg(long, value_name = "IMAGE")]
    image: Option<String>,
    #[arg(long, value_name = "PATH")]
    cwd: Option<PathBuf>,
    #[arg(long = "env", value_name = "KEY[=VALUE]")]
    env: Vec<String>,
    #[arg(
        long = "mount",
        value_name = "HOST:CONTAINER[:ro|:rw]",
        value_parser = |s: &str| s.parse::<MountSpec>(),
        help = "Docker-only bind mount; defaults to :ro and rejects --isolation host"
    )]
    mounts: Vec<MountSpec>,
    /// Pre-empt a live runtime that already occupies the requested tmux pane.
    ///
    /// Does not override session id reuse conflicts.
    #[arg(long)]
    force: bool,
}

pub async fn run(args: SpawnArgs) -> Result<()> {
    let SpawnArgs {
        output,
        runtime,
        session_id,
        target,
        isolation,
        image,
        cwd,
        env,
        mounts,
        force,
    } = args;
    ensure_mounts_allowed_for_isolation(&isolation, &mounts)?;
    let cwd = spawn_cwd(cwd)?;
    let socket_path = crate::shared::socket_path()?;
    let env = spawn_env(&isolation, env)?;
    let shell_resume = target
        .tmux_address()
        .map(|_| lilo_rm_core::capture_shell_resume(cwd.clone()));
    let payload = RuntimeClient::new(socket_path)
        .spawn(SpawnRequest {
            session_id,
            runtime,
            isolation,
            image,
            env,
            mounts,
            cwd,
            target,
            force,
            shell_resume,
        })
        .await?;

    output::emit(&output, &RuntimeResponse::Spawned(payload))?;
    Ok(())
}

fn spawn_env(isolation: &IsolationPolicy, overrides: Vec<String>) -> Result<Vec<LaunchEnv>> {
    let mut env = match isolation {
        IsolationPolicy::Host => lilo_rm_core::capture_caller_env(),
        IsolationPolicy::Docker(_) => Vec::new(),
    };
    for value in overrides {
        upsert_launch_env(&mut env, parse_spawn_env(value)?);
    }
    Ok(env)
}

fn parse_spawn_env(value: String) -> Result<LaunchEnv> {
    if let Some((key, explicit_value)) = value.split_once('=') {
        return spawn_env_entry(key, explicit_value);
    }

    if value.is_empty() {
        bail!("spawn env key cannot be empty");
    }
    let caller_value = std::env::var_os(&value)
        .ok_or_else(|| anyhow::anyhow!("spawn env {value} is not set in caller environment"))?;
    Ok(LaunchEnv::new(
        value,
        caller_value.to_string_lossy().into_owned(),
    ))
}

fn spawn_env_entry(key: &str, value: &str) -> Result<LaunchEnv> {
    if key.is_empty() {
        bail!("spawn env key cannot be empty");
    }
    Ok(LaunchEnv::new(key, value))
}

fn spawn_cwd(cwd: Option<PathBuf>) -> Result<PathBuf> {
    let Some(path) = cwd else {
        return lilo_rm_core::capture_caller_cwd().context("failed to capture caller cwd");
    };
    let caller_cwd = lilo_rm_core::capture_caller_cwd().context("failed to capture caller cwd")?;
    let resolved = resolve_caller_path(&caller_cwd, &path);
    let canonical = std::fs::canonicalize(&resolved)
        .with_context(|| format!("spawn cwd does not exist: {}", resolved.display()))?;
    if !canonical.is_dir() {
        bail!("spawn cwd is not a directory: {}", canonical.display());
    }
    Ok(canonical)
}

fn resolve_caller_path(caller_cwd: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        caller_cwd.join(path)
    }
}
