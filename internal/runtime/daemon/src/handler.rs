use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use lilo_rm_core::{
    CapturePayload, CursorExpiredPayload, DoctorPayload, EventsPayload, EventsRequest,
    KillByPidPayload, KilledPayload, McpBridgePayload, NudgePayload, RuntimeResponse, RuntimeRpc,
    ShimLaunchPayload, SpawnedPayload, StatusPayload, ValidateTargetPayload, VersionPayload,
    WatchersPayload, clamped_event_wait_ms, read_json_line, write_json_line,
};
use lilo_wire::LilodRpc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::broadcast;

use crate::{
    backend::RuntimeBackends,
    doctor,
    error::{RpcErrorContext, protocol_error_response, rpc_error_response},
    mcp_bridge,
    server::ServerState,
    spawn_preflight,
};

pub(crate) async fn handle_connection(
    stream: UnixStream,
    state: Arc<ServerState>,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let response = match read_json_line::<_, LilodRpc>(&mut reader).await {
        Ok(LilodRpc::Runtime(rpc)) => {
            let Some(response) = handle_rpc_or_disconnect(rpc, state, &mut reader).await? else {
                return Ok(());
            };
            response
        }
        Ok(LilodRpc::Session(_)) => RuntimeResponse::error(
            lilo_rm_core::ErrorCode::ProtocolMismatch,
            "session RPC sent to runtime handler",
        ),
        Err(error) => protocol_error_response(&error),
    };
    let should_stop = matches!(response, RuntimeResponse::Stopping);

    write_json_line(&mut write_half, &response).await?;
    if should_stop {
        let _ = shutdown_tx.send(());
    }
    Ok(())
}

async fn handle_rpc_or_disconnect<R>(
    rpc: RuntimeRpc,
    state: Arc<ServerState>,
    reader: &mut R,
) -> Result<Option<RuntimeResponse>>
where
    R: AsyncBufRead + Unpin,
{
    match rpc {
        RuntimeRpc::Events { request } if clamped_event_wait_ms(request.wait_ms) > 0 => {
            tokio::select! {
                response = handle_rpc(RuntimeRpc::Events { request }, state) => Ok(Some(response)),
                disconnected = wait_for_disconnect(reader) => {
                    disconnected?;
                    Ok(None)
                }
            }
        }
        other => Ok(Some(handle_rpc(other, state).await)),
    }
}

async fn wait_for_disconnect<R>(reader: &mut R) -> Result<()>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let buffer = reader.fill_buf().await?;
        if buffer.is_empty() {
            return Ok(());
        }
        let consumed = buffer.len();
        reader.consume(consumed);
    }
}

pub(crate) async fn handle_rpc(rpc: RuntimeRpc, state: Arc<ServerState>) -> RuntimeResponse {
    let error_context = error_context(&rpc);
    match handle_rpc_result(rpc, state).await {
        Ok(response) => response,
        Err(error) => rpc_error_response(error_context, &error),
    }
}

fn error_context(rpc: &RuntimeRpc) -> RpcErrorContext {
    match rpc {
        RuntimeRpc::Spawn { .. } => RpcErrorContext::Spawn,
        _ => RpcErrorContext::Other,
    }
}

async fn handle_rpc_result(rpc: RuntimeRpc, state: Arc<ServerState>) -> Result<RuntimeResponse> {
    match rpc {
        RuntimeRpc::Spawn { mut request } => {
            if let Some(conflict) = spawn_preflight::check(&state, &mut request).await? {
                return Ok(conflict);
            }
            let launch =
                lilo_runtime_launchers::dispatch(&request.runtime)?.launch_spec(&request)?;
            let backends = RuntimeBackends::new(state.config());
            let launch = backends.prepare_launch(&request, launch)?;
            let ready_rx = state.begin_spawn(&request, launch.clone()).await?;
            let evidence = match backends.spawn(&request, &launch).await {
                Ok(evidence) => evidence,
                Err(error) => {
                    state.cancel_spawn(request.session_id).await;
                    return Err(error);
                }
            };

            let ready = tokio::time::timeout(Duration::from_secs(10), ready_rx)
                .await
                .context("timed out waiting for ShimReady")?
                .context("shim ready channel closed")?;
            let (lifecycle, event) = state.record_running(&request, ready).await?;
            let (log_dir, stdout_path, stderr_path) = match evidence.log_paths {
                Some(paths) => (
                    Some(paths.log_dir),
                    Some(paths.stdout_path),
                    Some(paths.stderr_path),
                ),
                None => (None, None, None),
            };
            Ok(RuntimeResponse::Spawned(SpawnedPayload {
                lifecycle,
                event,
                log_dir,
                stdout_path,
                stderr_path,
            }))
        }
        RuntimeRpc::ValidateTarget { request } => {
            Ok(RuntimeResponse::ValidateTarget(ValidateTargetPayload {
                response: state.validate_target_request(request).await?,
            }))
        }
        RuntimeRpc::Kill { request } => Ok(RuntimeResponse::Killed(KilledPayload {
            outcome: state.kill_runtime(request).await?,
        })),
        RuntimeRpc::KillByPid { request } => Ok(RuntimeResponse::KillByPid(KillByPidPayload {
            response: state.kill_pid(request).await?,
        })),
        RuntimeRpc::Nudge { request } => {
            let response = state.nudge_runtime(request).await?;
            Ok(RuntimeResponse::Nudge(NudgePayload { response }))
        }
        RuntimeRpc::Capture { request } => Ok(RuntimeResponse::Capture(CapturePayload {
            response: state.capture_pane(request).await?,
        })),
        RuntimeRpc::Status { request } => Ok(RuntimeResponse::Status(StatusPayload {
            lifecycles: state.status(request.into()).await,
        })),
        RuntimeRpc::Version => Ok(RuntimeResponse::Version(VersionPayload {
            version: crate::version::runtime_version_info(),
        })),
        RuntimeRpc::Watchers => Ok(RuntimeResponse::Watchers(WatchersPayload {
            watchers: state.watcher_counts().await,
        })),
        RuntimeRpc::Doctor => Ok(RuntimeResponse::Doctor(DoctorPayload {
            doctor: doctor::collect(state).await?,
        })),
        RuntimeRpc::Events { request } => events_response(&state, request).await,
        RuntimeRpc::Stop => Ok(RuntimeResponse::Stopping),
        RuntimeRpc::McpBridge { request } => Ok(RuntimeResponse::McpBridge(McpBridgePayload {
            response: lilo_rm_core::McpBridgeResponse {
                line: mcp_bridge::handle_line(&state, &request.line).await,
            },
        })),
        RuntimeRpc::ShimLaunch { request } => {
            let launch = state.take_launch_spec(request.session_id).await?;
            Ok(RuntimeResponse::ShimLaunch(ShimLaunchPayload { launch }))
        }
        RuntimeRpc::ShimReady { ready } => {
            state.complete_shim_ready(ready).await?;
            Ok(RuntimeResponse::Ack)
        }
        RuntimeRpc::ShimExit { exit } => {
            let _ = state.record_shim_exit(exit).await?;
            Ok(RuntimeResponse::Ack)
        }
        _ => Ok(RuntimeResponse::error(
            lilo_rm_core::ErrorCode::ProtocolMismatch,
            "unsupported runtime rpc",
        )),
    }
}

async fn events_response(state: &ServerState, request: EventsRequest) -> Result<RuntimeResponse> {
    match state.events(request).await {
        Ok(batch) => Ok(RuntimeResponse::Events(EventsPayload {
            events: batch.events,
            cursor: batch.cursor,
        })),
        Err(expired) => Ok(RuntimeResponse::CursorExpired(CursorExpiredPayload {
            oldest: expired.oldest,
        })),
    }
}
