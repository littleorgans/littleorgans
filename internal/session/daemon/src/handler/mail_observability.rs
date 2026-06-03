use anyhow::{Context, Result, bail};
use lilo_common::id::SessionId;
use lilo_im_core::Action;
use lilo_session_core::{
    MailLogCursor, MailLogFilter, MailPeekRequest, MailPeekResponse, MailTailRequest,
    MailTailResponse, MessageView, RpcResponse, Selector,
};
use tokio::sync::broadcast;

use crate::identity_client::RequestContext;

use super::DaemonState;
use super::message_view;

impl DaemonState {
    pub(super) async fn mail_peek(
        &self,
        context: &RequestContext,
        request: &MailPeekRequest,
    ) -> Result<RpcResponse> {
        Self::ensure_operator_observer(context)?;
        self.authorize_mail_observation(context).await?;
        let messages = self
            .message_log_views(&request.filter, None)
            .await
            .context("failed to load mail transcript")?;
        Ok(RpcResponse::MailPeek {
            response: MailPeekResponse {
                cursor: cursor_for(&messages),
                messages,
            },
        })
    }

    pub(super) async fn mail_tail(
        &self,
        context: &RequestContext,
        request: &MailTailRequest,
    ) -> Result<RpcResponse> {
        Self::ensure_operator_observer(context)?;
        self.authorize_mail_observation(context).await?;
        let messages = self
            .tail_messages(&request.filter, request.after.as_ref(), request.follow)
            .await?;
        Ok(RpcResponse::MailTail {
            response: MailTailResponse {
                cursor: cursor_for(&messages).or_else(|| request.after.clone()),
                messages,
            },
        })
    }

    async fn tail_messages(
        &self,
        filter: &MailLogFilter,
        after: Option<&MailLogCursor>,
        follow: bool,
    ) -> Result<Vec<MessageView>> {
        let mut messages = self.message_log_views(filter, after).await?;
        if !follow || !messages.is_empty() {
            return Ok(messages);
        }

        let mut appends = self.subscribe_mail_appends();
        loop {
            match appends.recv().await {
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    messages = self.message_log_views(filter, after).await?;
                    if !messages.is_empty() {
                        return Ok(messages);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => return Ok(Vec::new()),
            }
        }
    }

    async fn message_log_views(
        &self,
        filter: &MailLogFilter,
        after: Option<&MailLogCursor>,
    ) -> Result<Vec<MessageView>> {
        let participant_ids = self.filter_ids(filter.selector.as_ref()).await?;
        let recipient_ids = self.filter_ids(filter.recipient.as_ref()).await?;
        let after = after.map(|cursor| (&cursor.sent_at, &cursor.message_id));
        let mail = self
            .store()
            .list_message_log(
                filter.context_id.as_deref(),
                participant_ids.as_deref(),
                recipient_ids.as_deref(),
                filter.include_system,
                after,
            )
            .await?;
        message_view::message_views(self, mail).await
    }

    async fn filter_ids(&self, selector: Option<&Selector>) -> Result<Option<Vec<SessionId>>> {
        let Some(selector) = selector else {
            return Ok(None);
        };
        Ok(Some(
            self.store()
                .list_sessions_by_selector(selector)
                .await?
                .into_iter()
                .map(|session| session.id)
                .collect(),
        ))
    }

    async fn authorize_mail_observation(&self, context: &RequestContext) -> Result<()> {
        self.identity
            .authorize(
                &context.principal,
                Action::MailRead,
                &lilo_im_core::ResourceSpec::default(),
            )
            .await
    }

    fn ensure_operator_observer(context: &RequestContext) -> Result<()> {
        if context.caller_session_id.is_some() {
            bail!("mail observation is operator-only");
        }
        Ok(())
    }
}

fn cursor_for(messages: &[MessageView]) -> Option<MailLogCursor> {
    messages.last().map(MailLogCursor::from_message)
}
