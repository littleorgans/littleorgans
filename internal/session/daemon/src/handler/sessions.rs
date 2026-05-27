use std::time::Duration;

use anyhow::{Context, Result};
use lilo_im_core::Action;
use lilo_session_core::{
    CaptureRequest, CaptureResponse, DeleteRequest, DeleteResponse, LabelRequest, LabelResponse,
    ListRequest, ListResponse, RpcResponse, Session, SessionState,
};
use uuid::Uuid;

use crate::identity_client::{RequestContext, session_resource};

use super::DaemonState;
use super::target::target_error;

impl DaemonState {
    pub(super) fn list(&self, request: ListRequest) -> Result<RpcResponse> {
        let selector = request.selector.unwrap_or_default();
        let sessions = self
            .store()?
            .list_sessions_by_selector(&selector)
            .context("failed to list sessions")?;

        Ok(RpcResponse::Listed {
            response: ListResponse { sessions },
        })
    }

    pub(super) async fn capture(
        &self,
        context: &RequestContext,
        request: CaptureRequest,
    ) -> Result<RpcResponse> {
        let session = self
            .store()?
            .get_session(&request.session_id)
            .context("failed to load capture session")?
            .ok_or_else(|| anyhow::anyhow!("unknown capture session: {}", request.session_id))?;
        self.identity
            .authorize(
                &context.principal,
                Action::Read,
                &session_resource(session.id),
            )
            .await?;
        let capture = self
            .driver
            .capture(&session.id.to_string(), request.scrollback_lines)
            .await
            .context("runtime capture failed")?
            .response;
        Ok(RpcResponse::Capture {
            response: CaptureResponse { session, capture },
        })
    }

    pub(super) async fn delete(
        &self,
        context: &RequestContext,
        request: DeleteRequest,
    ) -> Result<RpcResponse> {
        let targets = self.resolve_selector(&request.selector, "session")?;
        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            match self.delete_one(context, &request, target.id).await {
                Ok(session) => sessions.push(session),
                Err(error) => errors.push(target_error(&target.id, &error)),
            }
        }

        Ok(RpcResponse::Deleted {
            response: DeleteResponse { sessions, errors },
        })
    }

    pub(super) async fn label(
        &self,
        context: &RequestContext,
        request: LabelRequest,
    ) -> Result<RpcResponse> {
        let targets = self.resolve_selector(&request.selector, "session")?;
        let mut sessions = Vec::new();
        let mut errors = Vec::new();
        for target in targets {
            match self.label_one(context, target.id, &request).await {
                Ok(session) => sessions.push(session),
                Err(error) => errors.push(target_error(&target.id, &error)),
            }
        }
        Ok(RpcResponse::Labeled {
            response: LabelResponse { sessions, errors },
        })
    }

    pub(crate) async fn delete_one(
        &self,
        context: &RequestContext,
        request: &DeleteRequest,
        id: Uuid,
    ) -> Result<Session> {
        self.identity
            .authorize(&context.principal, Action::Kill, &session_resource(id))
            .await?;
        crate::lifecycle::refresh_exits(self).await?;
        let id_string = id.to_string();
        let session = self
            .store()?
            .get_session(&id)
            .context("failed to load session")?
            .with_context(|| format!("unknown session: {id}"))?;
        if session.state == SessionState::Terminated {
            return Ok(session);
        }
        let exit = self
            .driver
            .terminate(
                &id_string,
                &request.signal,
                Duration::from_secs(request.grace_secs),
            )
            .await
            .context("failed to terminate runtime")?
            .with_context(|| {
                format!(
                    "runtime did not terminate within {} grace seconds: {id}",
                    request.grace_secs
                )
            })?;
        crate::lifecycle::persist_child_exit(self, exit)
            .context("failed to persist terminated session")?
            .with_context(|| format!("unknown session: {id}"))
    }

    async fn label_one(
        &self,
        context: &RequestContext,
        target_id: Uuid,
        request: &LabelRequest,
    ) -> Result<Session> {
        self.identity
            .authorize(
                &context.principal,
                Action::Link,
                &session_resource(target_id),
            )
            .await?;
        self.store()?
            .apply_label_mutation(&target_id, &request.mutation)
            .context("failed to persist label")?
            .with_context(|| format!("unknown session: {target_id}"))
    }
}
