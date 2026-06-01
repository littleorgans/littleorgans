use std::collections::{BTreeSet, HashMap};

use anyhow::{Context, Result};
use lilo_session_core::{Mail, MessageView, RecipientSummary, SenderRef, SenderView, Session};
use uuid::Uuid;

use super::DaemonState;

pub(super) async fn message_views(
    state: &DaemonState,
    mail: Vec<Mail>,
) -> Result<Vec<MessageView>> {
    let sessions = message_sessions(state, &mail).await?;
    mail.iter()
        .map(|item| {
            let sender = sender_view(&item.sender, &sessions)?;
            let recipient = recipient_summary(item.recipient_id, &sessions)?;
            Ok(MessageView::from_mail(item, sender, recipient))
        })
        .collect()
}

async fn message_sessions(state: &DaemonState, mail: &[Mail]) -> Result<HashMap<Uuid, Session>> {
    let mut ids = BTreeSet::new();
    for item in mail {
        ids.insert(item.recipient_id);
        if let SenderRef::Session { session_id } = &item.sender {
            ids.insert(*session_id);
        }
    }
    let ids = ids.into_iter().collect::<Vec<_>>();
    let sessions = state
        .store()
        .list_sessions_by_ids(&ids)
        .await
        .context("failed to load message session summaries")?;
    Ok(sessions
        .into_iter()
        .map(|session| (session.id, session))
        .collect())
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
