use anyhow::{Context, Result};
use chrono::Utc;
use lilo_im_core::Action;
use lilo_session_core::{
    Mail, MailCheckRequest, MailCheckResponse, MailReadRequest, MailReadResponse, MailSendRequest,
    MailSendResponse, MailStopCheckRequest, MailStopCheckResponse, MailUnreadCount, NudgeDelivery,
    NudgeRequest, NudgeResponse, RpcResponse, Selector,
};
use uuid::Uuid;

use crate::identity_client::{RequestContext, session_resource};

use super::DaemonState;
use super::target::target_error;

impl DaemonState {
    pub(super) async fn mail_send(
        &self,
        context: &RequestContext,
        request: MailSendRequest,
    ) -> Result<RpcResponse> {
        let recipients = self.resolve_selector(&request.to, "recipient").await?;
        let sender_id = match request.from {
            Some(from) => {
                let id = Uuid::parse_str(&from).context("invalid sender session id")?;
                self.require_session(&id, "sender").await?;
                id
            }
            None => Uuid::nil(),
        };
        let mut mail = Vec::new();
        let mut errors = Vec::new();
        for recipient in recipients {
            if !recipient.state.is_active() {
                errors.push(lilo_session_core::TargetError {
                    target: recipient.id.to_string(),
                    message: format!("recipient is {}; mail not delivered", recipient.state),
                });
                continue;
            }
            match self
                .mail_send_one(context, sender_id, recipient.id, &request.content)
                .await
            {
                Ok(item) => mail.push(item),
                Err(error) => errors.push(target_error(&recipient.id, &error)),
            }
        }

        Ok(RpcResponse::MailSent {
            response: MailSendResponse { mail, errors },
        })
    }

    pub(super) async fn mail_read(
        &self,
        context: &RequestContext,
        request: MailReadRequest,
    ) -> Result<RpcResponse> {
        let recipients = self
            .resolve_selector(&request.selector, "recipient")
            .await?;
        let mut mail = Vec::new();
        let mut errors = Vec::new();
        for recipient in recipients {
            match self
                .mail_read_one(context, recipient.id, request.peek)
                .await
            {
                Ok(mut items) => mail.append(&mut items),
                Err(error) => errors.push(target_error(&recipient.id, &error)),
            }
        }

        Ok(RpcResponse::MailRead {
            response: MailReadResponse { mail, errors },
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

    async fn mail_send_one(
        &self,
        context: &RequestContext,
        sender_id: Uuid,
        recipient_id: Uuid,
        body: &str,
    ) -> Result<Mail> {
        self.identity
            .authorize(
                &context.principal,
                Action::MailSend,
                &session_resource(recipient_id),
            )
            .await?;
        let mail = Mail {
            id: Uuid::now_v7(),
            sender_id,
            recipient_id,
            content: body.to_string(),
            sent_at: Utc::now(),
            read_at: None,
        };
        self.store()
            .insert_mail(&mail)
            .await
            .context("failed to persist mail")?;
        Ok(mail)
    }

    async fn mail_read_one(
        &self,
        context: &RequestContext,
        recipient_id: Uuid,
        peek: bool,
    ) -> Result<Vec<Mail>> {
        self.identity
            .authorize(
                &context.principal,
                Action::MailRead,
                &session_resource(recipient_id),
            )
            .await?;
        self.store()
            .read_unread_mail(&recipient_id, Utc::now(), peek)
            .await
            .context("failed to read mail")
    }

    async fn mail_counts(&self, selector: &Selector) -> Result<Vec<MailUnreadCount>> {
        let recipients = self.resolve_selector(selector, "recipient").await?;
        let mut counts = Vec::new();
        for session in recipients {
            counts.push(MailUnreadCount {
                session_id: session.id.to_string(),
                unread: self.unread_mail_count(&session.id).await?,
            });
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
            .authorize(
                &context.principal,
                Action::Nudge,
                &session_resource(recipient_id),
            )
            .await?;
        let to = recipient_id.to_string();
        let result = self
            .driver
            .nudge(&to, message)
            .await
            .context("nudge driver failed")?;
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
        F: FnOnce(usize, Vec<MailUnreadCount>) -> RpcResponse,
    {
        let counts = self.mail_counts(selector).await?;
        let unread = total_unread(&counts);
        Ok(response(unread, counts))
    }
}

fn total_unread(counts: &[MailUnreadCount]) -> usize {
    counts.iter().map(|count| count.unread).sum()
}
