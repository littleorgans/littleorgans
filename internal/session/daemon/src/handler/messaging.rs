use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use chrono::Utc;
use lilo_im_core::Action;
use lilo_session_core::{
    Mail, MailCheckRequest, MailCheckResponse, MailCountView, MailDeliveryStatus, MailIntent,
    MailNotifyStatus, MailReadRequest, MailReadResponse, MailSendRequest, MailSendResponse,
    MailSendResult, MailStatus, MailStopCheckRequest, MailStopCheckResponse, MessageView,
    NudgeDelivery, NudgeRequest, NudgeResponse, RecipientSummary, RpcResponse, Selector, SenderRef,
    Session, SessionState, TargetError,
};
use uuid::Uuid;

use crate::identity_client::RequestContext;

use super::DaemonState;
use super::message_view;
use super::target::target_error;

const MAIL_NOTIFY_NUDGE_CONTENT: &str = "you have mail";

impl DaemonState {
    pub(super) async fn mail_send(
        &self,
        context: &RequestContext,
        request: MailSendRequest,
    ) -> Result<RpcResponse> {
        request.intent.ensure_client_send_allowed()?;
        let recipients = self.resolve_selector(&request.to, "recipient").await?;
        let sender = self.effective_sender(context).await?;
        let mut response = empty_send_response();
        let deliverable = self
            .deliverable_mail_recipients(context, recipients, &mut response)
            .await;
        if deliverable.is_empty() {
            return Ok(RpcResponse::MailSent { response });
        }

        let recipient_ids = deliverable
            .iter()
            .map(|(recipient_id, _)| *recipient_id)
            .collect::<Vec<_>>();
        let mail = mail_from_request(&request, sender, recipient_ids[0]);
        if self
            .append_idempotent_mail(&mail, &recipient_ids, &mut response)
            .await?
        {
            return Ok(RpcResponse::MailSent { response });
        }

        self.enforce_mail_safety(context, &mail).await?;
        self.persist_mail_send(
            context,
            &request,
            &mail,
            &recipient_ids,
            deliverable,
            &mut response,
        )
        .await?;

        Ok(RpcResponse::MailSent { response })
    }

    pub(super) async fn mail_read(
        &self,
        context: &RequestContext,
        _request: MailReadRequest,
    ) -> Result<RpcResponse> {
        let recipient = self.caller_mailbox(context).await?;
        let mail = self.mail_read_one(context, &recipient, false).await?;

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
        if !peek {
            self.emit_read_receipts(recipient, &mail).await?;
        }
        message_view::message_views(self, mail).await
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

    async fn deliverable_mail_recipients(
        &self,
        context: &RequestContext,
        recipients: Vec<Session>,
        response: &mut MailSendResponse,
    ) -> Vec<(Uuid, RecipientSummary)> {
        let mut deliverable = Vec::new();
        for recipient in recipients {
            if recipient.state != SessionState::Running {
                continue;
            }
            let recipient_summary = RecipientSummary::from_session(&recipient);

            match self
                .identity
                .authorize_session(&context.principal, Action::MailSend, recipient.id)
                .await
            {
                Ok(()) => deliverable.push((recipient.id, recipient_summary)),
                Err(error) => {
                    let message = format!("{error:#}");
                    response
                        .results
                        .push(failed_send_result(&recipient_summary, &message));
                    response.errors.push(target_error(&recipient.id, &error));
                }
            }
        }
        deliverable
    }

    async fn append_idempotent_mail(
        &self,
        mail: &Mail,
        recipient_ids: &[Uuid],
        response: &mut MailSendResponse,
    ) -> Result<bool> {
        let Some(mail) = self
            .store()
            .idempotent_mail_for_recipients(mail, recipient_ids)
            .await
            .context("failed to load idempotent mail")?
        else {
            return Ok(false);
        };
        let views = message_view::message_views(self, mail).await?;
        for view in views {
            response.results.push(successful_send_result(
                view.recipient.clone(),
                view,
                MailNotifyStatus::Skipped,
                None,
            ));
        }
        Ok(true)
    }

    async fn persist_mail_send(
        &self,
        context: &RequestContext,
        request: &MailSendRequest,
        mail: &Mail,
        recipient_ids: &[Uuid],
        deliverable: Vec<(Uuid, RecipientSummary)>,
        response: &mut MailSendResponse,
    ) -> Result<()> {
        match self
            .store()
            .insert_mail_for_recipients_with_outcome(mail, recipient_ids)
            .await
            .context("failed to persist mail")
        {
            Ok(outcome) => {
                let inserted = outcome.inserted;
                if inserted {
                    self.emit_mail_appends(&outcome.mail);
                }
                let views = message_view::message_views(self, outcome.mail).await?;
                for view in views {
                    let notify = if inserted {
                        self.notify_result(context, request, view.recipient.session_id)
                            .await
                    } else {
                        NotifyResult::skipped()
                    };
                    if let Some(error) = notify.error.clone() {
                        response.errors.push(TargetError {
                            target: view.recipient.session_id.to_string(),
                            message: error,
                        });
                    }
                    response.results.push(successful_send_result(
                        view.recipient.clone(),
                        view,
                        notify.status,
                        notify.error,
                    ));
                }
            }
            Err(error) => {
                let message = format!("{error:#}");
                for (recipient_id, recipient_summary) in deliverable {
                    response
                        .results
                        .push(failed_send_result(&recipient_summary, &message));
                    response.errors.push(target_error(&recipient_id, &error));
                }
            }
        }
        Ok(())
    }

    async fn enforce_mail_safety(&self, context: &RequestContext, mail: &Mail) -> Result<()> {
        if mail.intent == MailIntent::Receipt {
            return Ok(());
        }

        let limits = self.mail_safety;
        let depth = self
            .store()
            .count_conversation_depth(&mail.context_id)
            .await
            .context("failed to count conversation depth")?;
        if depth >= limits.conversation_depth_limit {
            tracing::warn!(
                principal = ?context.principal,
                sender = ?mail.sender,
                context_id = %mail.context_id,
                depth,
                limit = limits.conversation_depth_limit,
                "mail circuit breaker audit alert: conversation depth"
            );
            bail!(
                "mail circuit breaker tripped for context {}: conversation depth {} reached limit {}",
                mail.context_id,
                depth,
                limits.conversation_depth_limit
            );
        }

        let since = Utc::now() - limits.sender_rate_window;
        let rate = self
            .store()
            .count_sender_rate_since(&mail.sender, since)
            .await
            .context("failed to count sender mail rate")?;
        if rate >= limits.sender_rate_limit {
            tracing::warn!(
                principal = ?context.principal,
                sender = ?mail.sender,
                rate,
                limit = limits.sender_rate_limit,
                window_secs = limits.sender_rate_window.num_seconds(),
                "mail circuit breaker audit alert: sender rate"
            );
            bail!(
                "mail circuit breaker throttled sender: {} sends in {} seconds reached limit {}",
                rate,
                limits.sender_rate_window.num_seconds(),
                limits.sender_rate_limit
            );
        }
        Ok(())
    }

    async fn notify_result(
        &self,
        context: &RequestContext,
        request: &MailSendRequest,
        recipient_id: Uuid,
    ) -> NotifyResult {
        let Some(mode) = request.notify else {
            return NotifyResult::skipped();
        };
        tracing::debug!(
            ?mode,
            recipient_id = %recipient_id,
            "mail notify forwarding wake mode"
        );
        match self
            .nudge_one(context, recipient_id, MAIL_NOTIFY_NUDGE_CONTENT)
            .await
        {
            Ok(nudge) if nudge.delivered => NotifyResult::ok(),
            Ok(nudge) => NotifyResult::err(nudge.message),
            Err(error) => NotifyResult::err(format!("{error:#}")),
        }
    }

    async fn emit_read_receipts(&self, reader: &Session, mail: &[Mail]) -> Result<()> {
        for item in mail {
            let SenderRef::Session { session_id } = &item.sender else {
                continue;
            };
            if item.intent == MailIntent::Receipt {
                continue;
            }
            let Some(read_at) = item.read_at else {
                bail!("drained message {} did not include read_at", item.id);
            };
            let receipt = Mail {
                id: Uuid::now_v7(),
                sender: SenderRef::System,
                recipient_id: *session_id,
                content: read_receipt_content(reader, item, read_at),
                sent_at: Utc::now(),
                read_at: None,
                status: MailStatus::Unread,
                context_id: item.context_id.clone(),
                intent: MailIntent::Receipt,
                idempotency_key: None,
            };
            let inserted = self
                .store()
                .insert_mail(&receipt)
                .await
                .context("failed to persist read receipt")?;
            self.emit_mail_append(inserted.id);
        }
        Ok(())
    }

    fn emit_mail_appends(&self, mail: &[Mail]) {
        let ids = mail.iter().map(|item| item.id).collect::<BTreeSet<_>>();
        for message_id in ids {
            self.emit_mail_append(message_id);
        }
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
        if let Some(id) = context.caller_session_id {
            self.require_session(&id, "sender").await?;
            return Ok(SenderRef::session(id));
        }
        Ok(SenderRef::operator(serde_json::to_value(
            &context.principal,
        )?))
    }

    async fn caller_mailbox(&self, context: &RequestContext) -> Result<Session> {
        let Some(id) = context.caller_session_id else {
            bail!("mail read requires a caller session; operator has no mailbox");
        };
        let mut sessions = self
            .resolve_selector(&Selector::Id { id }, "recipient")
            .await?;
        Ok(sessions.remove(0))
    }
}

fn total_unread(counts: &[MailCountView]) -> usize {
    counts.iter().map(|count| count.unread).sum()
}

fn empty_send_response() -> MailSendResponse {
    MailSendResponse {
        results: Vec::new(),
        errors: Vec::new(),
    }
}

fn mail_from_request(request: &MailSendRequest, sender: SenderRef, recipient_id: Uuid) -> Mail {
    Mail {
        id: Uuid::now_v7(),
        sender,
        recipient_id,
        content: request.content.clone(),
        sent_at: Utc::now(),
        read_at: None,
        status: MailStatus::Unread,
        context_id: request.context_id.clone(),
        intent: request.intent,
        idempotency_key: request.idempotency_key.clone(),
    }
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

struct NotifyResult {
    status: MailNotifyStatus,
    error: Option<String>,
}

impl NotifyResult {
    fn ok() -> Self {
        Self {
            status: MailNotifyStatus::Ok,
            error: None,
        }
    }

    fn err(error: String) -> Self {
        Self {
            status: MailNotifyStatus::Err,
            error: Some(error),
        }
    }

    fn skipped() -> Self {
        Self {
            status: MailNotifyStatus::Skipped,
            error: None,
        }
    }
}

fn successful_send_result(
    recipient: RecipientSummary,
    message: MessageView,
    notify: MailNotifyStatus,
    error: Option<String>,
) -> MailSendResult {
    MailSendResult {
        recipient,
        mail: MailDeliveryStatus::Ok,
        notify,
        message: Some(message),
        error,
    }
}

fn read_receipt_content(reader: &Session, item: &Mail, read_at: chrono::DateTime<Utc>) -> String {
    format!(
        "read receipt: {} ({}) read message {} at {}",
        reader.role,
        reader.id,
        item.id,
        read_at.to_rfc3339()
    )
}
