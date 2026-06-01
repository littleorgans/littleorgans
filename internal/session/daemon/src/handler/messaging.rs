use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result, bail};
use chrono::Utc;
use lilo_im_core::Action;
use lilo_session_core::{
    Mail, MailCheckRequest, MailCheckResponse, MailCountView, MailDeliveryStatus, MailNotifyStatus,
    MailReadRequest, MailReadResponse, MailSendRequest, MailSendResponse, MailSendResult,
    MailStopCheckRequest, MailStopCheckResponse, MessageView, NudgeDelivery, NudgeRequest,
    NudgeResponse, RecipientSummary, RpcResponse, Selector, SenderRef, SenderView, Session,
};
use uuid::Uuid;

use crate::identity_client::RequestContext;

use super::DaemonState;
use super::target::target_error;

impl DaemonState {
    pub(super) async fn mail_send(
        &self,
        context: &RequestContext,
        request: MailSendRequest,
    ) -> Result<RpcResponse> {
        request.intent.ensure_client_send_allowed()?;
        let recipients = self.resolve_selector(&request.to, "recipient").await?;
        let sender = self.effective_sender(context).await?;
        let mut results = Vec::new();
        let mut errors = Vec::new();
        let mut deliverable = Vec::new();
        for recipient in recipients {
            let recipient_summary = RecipientSummary::from_session(&recipient);
            if !recipient.state.is_active() {
                let message = format!("recipient is {}; mail not delivered", recipient.state);
                results.push(failed_send_result(&recipient_summary, &message));
                errors.push(lilo_session_core::TargetError {
                    target: recipient.id.to_string(),
                    message,
                });
                continue;
            }
            match self
                .identity
                .authorize_session(&context.principal, Action::MailSend, recipient.id)
                .await
            {
                Ok(()) => deliverable.push((recipient.id, recipient_summary)),
                Err(error) => {
                    let message = format!("{error:#}");
                    results.push(failed_send_result(&recipient_summary, &message));
                    errors.push(target_error(&recipient.id, &error));
                }
            }
        }
        if !deliverable.is_empty() {
            let recipient_ids = deliverable
                .iter()
                .map(|(recipient_id, _)| *recipient_id)
                .collect::<Vec<_>>();
            let mail = Mail {
                id: Uuid::now_v7(),
                sender,
                recipient_id: recipient_ids[0],
                content: request.content.clone(),
                sent_at: Utc::now(),
                read_at: None,
                context_id: request.context_id.clone(),
                intent: request.intent,
                idempotency_key: request.idempotency_key.clone(),
            };
            match self
                .store()
                .insert_mail_for_recipients(&mail, &recipient_ids)
                .await
                .context("failed to persist mail")
            {
                Ok(mail) => {
                    let views = self.message_views(mail).await?;
                    for view in views {
                        results.push(successful_send_result(view.recipient.clone(), view));
                    }
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    for (recipient_id, recipient_summary) in deliverable {
                        results.push(failed_send_result(&recipient_summary, &message));
                        errors.push(target_error(&recipient_id, &error));
                    }
                }
            }
        }

        Ok(RpcResponse::MailSent {
            response: MailSendResponse { results, errors },
        })
    }

    pub(super) async fn mail_read(
        &self,
        context: &RequestContext,
        request: MailReadRequest,
    ) -> Result<RpcResponse> {
        let recipient = self.caller_mailbox(context).await?;
        let mail = self
            .mail_read_one(context, &recipient, request.peek)
            .await?;

        Ok(RpcResponse::MailRead {
            response: MailReadResponse {
                messages: mail,
                errors: Vec::new(),
            },
        })
    }

    pub(super) async fn mail_check(&self, request: &MailCheckRequest) -> Result<RpcResponse> {
        self.mail_count_response(&request.selector, |unread, counts| {
            RpcResponse::MailChecked {
                response: MailCheckResponse { unread, counts },
            }
        })
        .await
    }

    pub(super) async fn mail_stop_check(
        &self,
        request: &MailStopCheckRequest,
    ) -> Result<RpcResponse> {
        self.mail_count_response(&request.selector, |unread, counts| {
            RpcResponse::MailStopChecked {
                response: MailStopCheckResponse { unread, counts },
            }
        })
        .await
    }

    pub(super) async fn nudge(
        &self,
        context: &RequestContext,
        request: NudgeRequest,
    ) -> Result<RpcResponse> {
        let recipients = self.resolve_selector(&request.to, "recipient").await?;
        let mut nudges = Vec::new();
        let mut errors = Vec::new();
        for recipient in recipients {
            match self
                .nudge_one(context, recipient.id, &request.content)
                .await
            {
                Ok(nudge) => nudges.push(nudge),
                Err(error) => errors.push(target_error(&recipient.id, &error)),
            }
        }

        Ok(RpcResponse::Nudged {
            response: NudgeResponse { nudges, errors },
        })
    }

    async fn mail_read_one(
        &self,
        context: &RequestContext,
        recipient: &Session,
        peek: bool,
    ) -> Result<Vec<MessageView>> {
        self.identity
            .authorize_session(&context.principal, Action::MailRead, recipient.id)
            .await?;
        let mail = self
            .store()
            .read_unread_mail(&recipient.id, Utc::now(), peek)
            .await
            .context("failed to read mail")?;
        self.message_views(mail).await
    }

    async fn mail_counts(&self, selector: &Selector) -> Result<Vec<MailCountView>> {
        let recipients = self.resolve_selector(selector, "recipient").await?;
        let mut counts = Vec::new();
        for session in recipients {
            counts.push(MailCountView::from_session(
                &session,
                self.unread_mail_count(&session.id).await?,
            ));
        }
        Ok(counts)
    }

    async fn nudge_one(
        &self,
        context: &RequestContext,
        recipient_id: Uuid,
        message: &str,
    ) -> Result<NudgeDelivery> {
        self.identity
            .authorize_session(&context.principal, Action::Nudge, recipient_id)
            .await?;
        let to = recipient_id.to_string();
        let result = self
            .runtime
            .nudge(&to, message)
            .await
            .context("nudge runtime port failed")?;
        Ok(NudgeDelivery {
            to,
            delivered: result.delivered,
            message: result.message,
        })
    }

    async fn unread_mail_count(&self, recipient_id: &Uuid) -> Result<usize> {
        self.require_session(recipient_id, "recipient").await?;
        self.store()
            .count_unread_mail(recipient_id)
            .await
            .context("failed to count unread mail")
    }

    async fn mail_count_response<F>(&self, selector: &Selector, response: F) -> Result<RpcResponse>
    where
        F: FnOnce(usize, Vec<MailCountView>) -> RpcResponse,
    {
        let counts = self.mail_counts(selector).await?;
        let unread = total_unread(&counts);
        Ok(response(unread, counts))
    }

    async fn effective_sender(&self, context: &RequestContext) -> Result<SenderRef> {
        if let Some(id) = context.mcp_caller_session_id {
            self.require_session(&id, "sender").await?;
            return Ok(SenderRef::session(id));
        }
        Ok(SenderRef::operator(serde_json::to_value(
            &context.principal,
        )?))
    }

    async fn caller_mailbox(&self, context: &RequestContext) -> Result<Session> {
        let Some(id) = context.mcp_caller_session_id else {
            bail!("mail read requires a caller session; operator has no mailbox");
        };
        let mut sessions = self
            .resolve_selector(&Selector::Id { id }, "recipient")
            .await?;
        Ok(sessions.remove(0))
    }

    async fn message_views(&self, mail: Vec<Mail>) -> Result<Vec<MessageView>> {
        let sessions = self.message_sessions(&mail).await?;
        mail.iter()
            .map(|item| {
                let sender = sender_view(&item.sender, &sessions)?;
                let recipient = recipient_summary(item.recipient_id, &sessions)?;
                Ok(MessageView::from_mail(item, sender, recipient))
            })
            .collect()
    }

    async fn message_sessions(&self, mail: &[Mail]) -> Result<HashMap<Uuid, Session>> {
        let mut ids = BTreeSet::new();
        for item in mail {
            ids.insert(item.recipient_id);
            if let SenderRef::Session { session_id } = &item.sender {
                ids.insert(*session_id);
            }
        }
        let ids = ids.into_iter().collect::<Vec<_>>();
        let sessions = self
            .store()
            .list_sessions_by_ids(&ids)
            .await
            .context("failed to load message session summaries")?;
        Ok(sessions
            .into_iter()
            .map(|session| (session.id, session))
            .collect())
    }
}

fn total_unread(counts: &[MailCountView]) -> usize {
    counts.iter().map(|count| count.unread).sum()
}

fn failed_send_result(recipient: &RecipientSummary, error: &str) -> MailSendResult {
    MailSendResult {
        recipient: recipient.clone(),
        mail: MailDeliveryStatus::Err,
        notify: MailNotifyStatus::Skipped,
        message: None,
        error: Some(error.to_string()),
    }
}

fn successful_send_result(recipient: RecipientSummary, message: MessageView) -> MailSendResult {
    MailSendResult {
        recipient,
        mail: MailDeliveryStatus::Ok,
        notify: MailNotifyStatus::Skipped,
        message: Some(message),
        error: None,
    }
}

fn sender_view(sender: &SenderRef, sessions: &HashMap<Uuid, Session>) -> Result<SenderView> {
    match sender {
        SenderRef::Session { session_id } => {
            let session = sessions
                .get(session_id)
                .ok_or_else(|| anyhow::anyhow!("unknown sender session: {session_id}"))?;
            Ok(SenderView::session(session))
        }
        SenderRef::Operator { principal } => Ok(SenderView::operator(principal.clone())),
        SenderRef::System => Ok(SenderView::System),
    }
}

fn recipient_summary(
    recipient_id: Uuid,
    sessions: &HashMap<Uuid, Session>,
) -> Result<RecipientSummary> {
    let session = sessions
        .get(&recipient_id)
        .ok_or_else(|| anyhow::anyhow!("unknown recipient session: {recipient_id}"))?;
    Ok(RecipientSummary::from_session(session))
}
