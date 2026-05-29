use std::sync::Arc;

use anyhow::Result;
use lilo_im_core::{Principal, peer_creds};
use lilo_rm_core::{
    CapturePayload, CursorExpiredPayload, DoctorPayload, EventBatch, EventsPayload, EventsRequest,
    KillByPidPayload, KilledPayload, McpBridgePayload, NudgePayload, RuntimeResponse, RuntimeRpc,
    ShimLaunchPayload, StatusPayload, ValidateTargetPayload, VersionPayload, WatchersPayload,
    clamped_event_wait_ms, read_json_line, write_json_line,
};
use lilo_wire::LilodRpc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::broadcast;

use crate::{
    doctor,
    error::{RpcErrorContext, protocol_error_response, rpc_error_response},
    identity::authorize_runtime_rpc,
    mcp_bridge,
    server::ServerState,
    service::{SpawnOutcome, poll_events_batch, spawn_domain},
};

pub(crate) async fn handle_connection(
    stream: UnixStream,
    state: Arc<ServerState>,
    shutdown_tx: broadcast::Sender<()>,
) -> Result<()> {
    let principal = match peer_creds::extract(&stream).await {
        Ok(principal) => principal,
        Err(error) => {
            let response = RuntimeResponse::error(
                lilo_rm_core::ErrorCode::ProtocolMismatch,
                error.to_string(),
            );
            let (_read_half, mut write_half) = stream.into_split();
            write_json_line(&mut write_half, &response).await?;
            return Ok(());
        }
    };

    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);

    let response = match read_json_line::<_, LilodRpc>(&mut reader).await {
        Ok(LilodRpc::Runtime(rpc)) => {
            let Some(response) =
                handle_rpc_or_disconnect(principal, rpc, state, &mut reader).await?
            else {
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
    principal: Principal,
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
                response = handle_rpc(principal, RuntimeRpc::Events { request }, state) => Ok(Some(response)),
                disconnected = wait_for_disconnect(reader) => {
                    disconnected?;
                    Ok(None)
                }
            }
        }
        other => Ok(Some(handle_rpc(principal, other, state).await)),
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

pub(crate) async fn handle_rpc(
    principal: Principal,
    rpc: RuntimeRpc,
    state: Arc<ServerState>,
) -> RuntimeResponse {
    let error_context = error_context(&rpc);
    match handle_rpc_result(principal, rpc, state).await {
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

async fn handle_rpc_result(
    principal: Principal,
    rpc: RuntimeRpc,
    state: Arc<ServerState>,
) -> Result<RuntimeResponse> {
    authorize_runtime_rpc(&state, &principal, &rpc).await?;
    match rpc {
        RuntimeRpc::Spawn { request } => Ok(spawn_response(spawn_domain(&state, request).await?)),
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

fn spawn_response(outcome: SpawnOutcome) -> RuntimeResponse {
    match outcome {
        SpawnOutcome::Spawned(payload) => RuntimeResponse::Spawned(payload),
        SpawnOutcome::Conflict(payload) => RuntimeResponse::SpawnConflict(payload),
    }
}

async fn events_response(state: &ServerState, request: EventsRequest) -> Result<RuntimeResponse> {
    Ok(event_batch_response(
        poll_events_batch(state, request).await,
    ))
}

fn event_batch_response(batch: EventBatch) -> RuntimeResponse {
    match batch {
        EventBatch::Events { events, cursor } => {
            RuntimeResponse::Events(EventsPayload { events, cursor })
        }
        EventBatch::CursorExpired { oldest } => {
            RuntimeResponse::CursorExpired(CursorExpiredPayload { oldest })
        }
        _ => RuntimeResponse::error(
            lilo_rm_core::ErrorCode::ProtocolMismatch,
            "unsupported event batch variant",
        ),
    }
}

#[cfg(test)]
mod tests;
