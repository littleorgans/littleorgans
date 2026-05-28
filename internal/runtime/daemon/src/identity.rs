use anyhow::{Context, Result};
use lilo_db::{begin_immediate_tx, finish_immediate_tx};
use lilo_identity_service::IdentityClient;
use lilo_im_core::{Action, Principal, ResourceSpec, RuntimeKind as IdentityRuntimeKind};
use lilo_rm_core::{RuntimeKind, RuntimeRpc, SpawnRequest, StatusRequest};

use crate::server::ServerState;

pub(crate) async fn authorize_runtime_rpc(
    state: &ServerState,
    principal: &Principal,
    rpc: &RuntimeRpc,
) -> Result<()> {
    match rpc {
        RuntimeRpc::Spawn { request } => authorize_runtime_spawn(state, principal, request).await,
        RuntimeRpc::ShimLaunch { request } => {
            authorize_shim_callback(state.identity(), principal, request.session_id).await
        }
        RuntimeRpc::ShimReady { ready } => {
            authorize_shim_callback(state.identity(), principal, ready.session_id).await
        }
        RuntimeRpc::ShimExit { exit } => {
            authorize_shim_callback(state.identity(), principal, exit.session_id).await
        }
        other => {
            let (action, resource) = runtime_authorization(other);
            state
                .identity()
                .authorize(principal, action, &resource)
                .await
        }
    }
}

async fn authorize_runtime_spawn(
    state: &ServerState,
    principal: &Principal,
    request: &SpawnRequest,
) -> Result<()> {
    let mut conn = state
        .store()
        .pool()
        .acquire()
        .await
        .context("failed to acquire runtime spawn authorization connection")?;
    begin_immediate_tx(&mut conn, "runtime spawn authorization").await?;
    let result = state
        .identity()
        .authorize_in_tx(
            &mut conn,
            principal,
            Action::Spawn,
            &runtime_spawn_resource(request),
        )
        .await;
    if let Err(error) = result {
        finish_immediate_tx(&mut conn, Ok(()), "runtime spawn authorization").await?;
        return Err(error);
    }
    finish_immediate_tx(&mut conn, Ok(()), "runtime spawn authorization").await
}

async fn authorize_shim_callback(
    identity: &IdentityClient,
    principal: &Principal,
    session_id: uuid::Uuid,
) -> Result<()> {
    // Shim callbacks are local control-plane continuations from a process this
    // daemon launched on the same host. They are accepted only for local peer
    // credentials and are still audited through the identity client.
    identity
        .authorize(
            principal,
            Action::ShimCallback,
            &ResourceSpec {
                session_id: Some(session_id),
                ..Default::default()
            },
        )
        .await
}

fn runtime_authorization(rpc: &RuntimeRpc) -> (Action, ResourceSpec) {
    match rpc {
        RuntimeRpc::ValidateTarget { .. } => (Action::Spawn, ResourceSpec::default()),
        RuntimeRpc::Kill { request } => (Action::Kill, session_resource(request.session_id)),
        RuntimeRpc::KillByPid { .. } => (Action::Kill, ResourceSpec::default()),
        RuntimeRpc::Nudge { .. } => (Action::Nudge, ResourceSpec::default()),
        RuntimeRpc::Capture { request } => (Action::Logs, session_resource(request.session_id)),
        RuntimeRpc::Status { request } => (Action::List, status_resource(request)),
        RuntimeRpc::Version | RuntimeRpc::Watchers | RuntimeRpc::Events { .. } => {
            (Action::Read, ResourceSpec::default())
        }
        RuntimeRpc::Doctor => (Action::Doctor, ResourceSpec::default()),
        RuntimeRpc::Stop | RuntimeRpc::McpBridge { .. } => {
            (Action::Daemon, ResourceSpec::default())
        }
        RuntimeRpc::Spawn { .. }
        | RuntimeRpc::ShimLaunch { .. }
        | RuntimeRpc::ShimReady { .. }
        | RuntimeRpc::ShimExit { .. } => {
            unreachable!("delegated upstream by authorize_runtime_rpc")
        }
        _ => {
            unreachable!("unhandled RuntimeRpc variant: add explicit runtime_authorization mapping")
        }
    }
}

fn runtime_spawn_resource(request: &SpawnRequest) -> ResourceSpec {
    ResourceSpec {
        runtime: Some(identity_runtime(request.runtime.clone())),
        session_id: Some(request.session_id),
        ..Default::default()
    }
}

fn status_resource(request: &StatusRequest) -> ResourceSpec {
    ResourceSpec {
        session_id: request
            .session_id
            .or_else(|| request.session_ids.first().copied()),
        ..Default::default()
    }
}

fn session_resource(session_id: uuid::Uuid) -> ResourceSpec {
    ResourceSpec {
        session_id: Some(session_id),
        ..Default::default()
    }
}

fn identity_runtime(runtime: RuntimeKind) -> IdentityRuntimeKind {
    match runtime {
        RuntimeKind::Claude => IdentityRuntimeKind::Claude,
        RuntimeKind::Codex => IdentityRuntimeKind::Codex,
        RuntimeKind::Other(value) => IdentityRuntimeKind::Other(value),
    }
}
