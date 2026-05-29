use crate::error::RuntimeFailure;
use anyhow::{Context, Result};
use lilo_rm_core::{
    LaunchEnv, LaunchSpec, RuntimeResponse, RuntimeRpc, ShimExit, ShimLaunchRequest, ShimReady,
    SpawnRequest, SpawnTarget, TmuxAddress, TmuxSpawnTarget, read_json_line,
    read_json_line_blocking, write_json_line, write_json_line_blocking,
};
use lilo_wire::LilodRpc;
use std::io::BufReader as StdBufReader;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::{AsyncRead, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::process::Command;

use crate::server::DaemonConfig;

pub struct HeadlessLogPaths {
    pub log_dir: PathBuf,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
}

pub async fn launch_shim(
    config: &DaemonConfig,
    request: &SpawnRequest,
) -> Result<Option<HeadlessLogPaths>> {
    let env = shim_env(config)?;
    match &request.target {
        SpawnTarget::Tmux(target) => launch_tmux_shim(config, request, target, &env)
            .await
            .map(|()| None),
        SpawnTarget::Headless(_) => launch_headless_shim(config, request, &env).await.map(Some),
    }
}

async fn launch_tmux_shim(
    config: &DaemonConfig,
    request: &SpawnRequest,
    target: &TmuxSpawnTarget,
    env: &[LaunchEnv],
) -> Result<()> {
    let argv = shim_argv(config, request);
    let tmux_server_label = config.tmux_server_label.as_deref();
    match lilo_runtime_platform::tmux::TmuxGateway::respawn_pane(
        tmux_server_label,
        &target.address,
        &argv,
        env,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) => {
            Err(classify_tmux_respawn_error(tmux_server_label, &target.address, error).await)
        }
    }
}

async fn classify_tmux_respawn_error(
    server_label: Option<&str>,
    address: &TmuxAddress,
    error: anyhow::Error,
) -> anyhow::Error {
    match lilo_runtime_platform::tmux::TmuxGateway::is_alive(server_label, address).await {
        Ok(false) => RuntimeFailure::tmux_pane_dead(address.clone()),
        Ok(true) | Err(_) => error.context(format!("failed to respawn tmux pane {address}")),
    }
}

async fn launch_headless_shim(
    config: &DaemonConfig,
    request: &SpawnRequest,
    env: &[LaunchEnv],
) -> Result<HeadlessLogPaths> {
    let paths = config.session_log_paths(request.session_id);
    tokio::fs::create_dir_all(&paths.log_dir)
        .await
        .with_context(|| format!("failed to create log directory {}", paths.log_dir.display()))?;
    let stdout = File::create(&paths.stdout_path)
        .await
        .context("failed to create headless stdout log")?;
    let stderr = File::create(&paths.stderr_path)
        .await
        .context("failed to create headless stderr log")?;

    let mut command = Command::new(&config.shim_path);
    command
        .arg("__shim")
        .arg("--session-id")
        .arg(request.session_id.to_string());
    for entry in env {
        command.env(&entry.key, &entry.value);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn shim {}", config.shim_path.display()))?;
    let child_stdout = child
        .stdout
        .take()
        .context("headless shim stdout missing")?;
    let child_stderr = child
        .stderr
        .take()
        .context("headless shim stderr missing")?;
    spawn_log_copy(request.session_id, "stdout", child_stdout, stdout);
    spawn_log_copy(request.session_id, "stderr", child_stderr, stderr);
    Ok(paths)
}

fn spawn_log_copy<R>(session_id: uuid::Uuid, stream: &'static str, reader: R, file: File)
where
    R: AsyncRead + Send + Unpin + 'static,
{
    tokio::spawn(async move {
        if let Err(error) = copy_log_stream(reader, file).await {
            tracing::warn!(%error, %session_id, stream, "headless log copy failed");
        }
    });
}

async fn copy_log_stream<R>(mut reader: R, mut file: File) -> Result<()>
where
    R: AsyncRead + Unpin,
{
    tokio::io::copy(&mut reader, &mut file).await?;
    file.flush().await?;
    Ok(())
}

fn shim_argv(config: &DaemonConfig, request: &SpawnRequest) -> Vec<String> {
    vec![
        config.shim_path.to_string_lossy().into_owned(),
        "__shim".to_owned(),
        "--session-id".to_owned(),
        request.session_id.to_string(),
    ]
}

/// Build the bootstrap env handed to the shim before it phones home for the
/// real `LaunchSpec`. By contract, this is the **only** env handed to tmux via
/// `respawn-pane -e ...` and the only env the shim inherits at startup. The
/// runtime's actual env arrives over the post-spawn UDS handoff
/// (`ShimLaunch` -> `LaunchSpec.env`) and the shim applies it after
/// `env_clear()` in `rtm-cli/src/cli/shim.rs::runtime_command`.
///
/// Adding entries here is a deliberate widening of the bootstrap surface and
/// must be paired with a documented justification.
fn shim_env(config: &DaemonConfig) -> Result<Vec<LaunchEnv>> {
    let socket_path = config
        .socket_path()
        .context("headless shim transport requires a Unix socket endpoint")?;
    Ok(vec![LaunchEnv {
        key: "LILO_SOCKET_PATH".to_owned(),
        value: socket_path.to_string_lossy().into_owned(),
    }])
}

pub async fn request_launch(
    socket_path: &std::path::Path,
    request: ShimLaunchRequest,
) -> Result<LaunchSpec> {
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(
        &mut write_half,
        &LilodRpc::Runtime(RuntimeRpc::ShimLaunch { request }),
    )
    .await?;

    let mut reader = BufReader::new(read_half);
    launch_from_response(read_json_line(&mut reader).await?)
}

pub fn request_launch_blocking(
    socket_path: &std::path::Path,
    request: ShimLaunchRequest,
) -> Result<LaunchSpec> {
    let mut stream = StdUnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    write_json_line_blocking(
        &mut stream,
        &LilodRpc::Runtime(RuntimeRpc::ShimLaunch { request }),
    )?;

    let mut reader = StdBufReader::new(stream);
    launch_from_response(read_json_line_blocking(&mut reader)?)
}

pub async fn send_ready(socket_path: &std::path::Path, ready: ShimReady) -> Result<()> {
    send_shim_rpc(socket_path, RuntimeRpc::ShimReady { ready }, "ShimReady").await
}

pub fn send_ready_blocking(socket_path: &std::path::Path, ready: ShimReady) -> Result<()> {
    send_shim_rpc_blocking(socket_path, &RuntimeRpc::ShimReady { ready }, "ShimReady")
}

pub async fn send_exit(socket_path: &std::path::Path, exit: ShimExit) -> Result<()> {
    send_shim_rpc(socket_path, RuntimeRpc::ShimExit { exit }, "ShimExit").await
}

pub fn send_exit_blocking(socket_path: &std::path::Path, exit: ShimExit) -> Result<()> {
    send_shim_rpc_blocking(socket_path, &RuntimeRpc::ShimExit { exit }, "ShimExit")
}

async fn send_shim_rpc(
    socket_path: &std::path::Path,
    rpc: RuntimeRpc,
    label: &'static str,
) -> Result<()> {
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &LilodRpc::Runtime(rpc)).await?;

    let mut reader = BufReader::new(read_half);
    ack_from_response(read_json_line(&mut reader).await?, label)
}

fn send_shim_rpc_blocking(
    socket_path: &std::path::Path,
    rpc: &RuntimeRpc,
    label: &'static str,
) -> Result<()> {
    let mut stream = StdUnixStream::connect(socket_path)
        .with_context(|| format!("failed to connect to {}", socket_path.display()))?;
    write_json_line_blocking(&mut stream, &LilodRpc::Runtime(rpc.clone()))?;

    let mut reader = StdBufReader::new(stream);
    ack_from_response(read_json_line_blocking(&mut reader)?, label)
}

fn launch_from_response(response: RuntimeResponse) -> Result<LaunchSpec> {
    match response {
        RuntimeResponse::ShimLaunch(payload) => Ok(payload.launch),
        RuntimeResponse::Error(payload) => anyhow::bail!(payload.message),
        response => anyhow::bail!("unexpected ShimLaunch response: {response:?}"),
    }
}

fn ack_from_response(response: RuntimeResponse, label: &'static str) -> Result<()> {
    match response {
        RuntimeResponse::Ack => Ok(()),
        RuntimeResponse::Error(payload) => anyhow::bail!(payload.message),
        response => anyhow::bail!("unexpected {label} response: {response:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{RpcErrorContext, rpc_error_response};
    use lilo_rm_core::{ErrorCode, RuntimeKind, RuntimeResponse};
    use lilo_runtime_platform::test_support::TmuxSession;
    use std::path::PathBuf;

    fn test_config() -> DaemonConfig {
        DaemonConfig::test_fixture()
    }

    #[test]
    fn shim_env_only_contains_socket_path() {
        // Contract: the bootstrap env (the only env that ever rides through
        // tmux respawn-pane -e and is inherited by the shim process) is
        // exactly {LILO_SOCKET_PATH}. Runtime env arrives over UDS.
        let env = shim_env(&test_config()).expect("shim env");
        assert_eq!(env.len(), 1, "bootstrap env widened unexpectedly: {env:?}");
        assert_eq!(env[0].key, "LILO_SOCKET_PATH");
        assert_eq!(env[0].value, "/tmp/rtm.sock");
    }

    #[test]
    fn session_log_paths_are_config_owned() {
        let session_id = uuid::Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let paths = test_config().session_log_paths(session_id);

        assert_eq!(
            paths.log_dir,
            PathBuf::from("/tmp/rtm/logs/11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(paths.stdout_path.parent(), Some(paths.log_dir.as_path()));
        assert_eq!(paths.stderr_path.parent(), Some(paths.log_dir.as_path()));
        assert_ne!(paths.stdout_path, paths.stderr_path);
    }

    #[tokio::test]
    async fn launch_shim_types_dead_tmux_respawn_target() {
        let Some(tmux_session) = TmuxSession::start("rtm-respawn-dead") else {
            eprintln!("skipping tmux launch_shim test because tmux is unavailable");
            return;
        };
        let address = tmux_session.pane();
        tmux_session.kill();
        let mut config = test_config();
        config.tmux_server_label = Some(tmux_session.server_label().to_owned());

        let Err(error) = launch_shim(
            &config,
            &SpawnRequest {
                session_id: uuid::Uuid::now_v7(),
                runtime: RuntimeKind::Claude,
                isolation: lilo_rm_core::IsolationPolicy::default(),
                image: None,
                env: Vec::new(),
                mounts: Vec::new(),
                cwd: PathBuf::from("/tmp"),
                target: SpawnTarget::Tmux(TmuxSpawnTarget {
                    address: address.parse().expect("tmux address"),
                }),
                force: false,
                shell_resume: None,
            },
        )
        .await
        else {
            panic!("dead pane should fail launch");
        };

        let RuntimeResponse::Error(payload) = rpc_error_response(RpcErrorContext::Spawn, &error)
        else {
            panic!("expected error response");
        };
        assert_eq!(payload.code, ErrorCode::TmuxPaneDead);
    }
}
