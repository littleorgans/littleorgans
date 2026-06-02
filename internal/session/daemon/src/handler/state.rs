use std::sync::Arc;

use lilo_runtime_daemon::RuntimeService;
use lilo_session_core::RpcResponse;
use lilo_session_driver::RuntimePort;
use lilo_session_store::SqliteStore;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::identity_client::IdentityPort;
use crate::mail_safety::MailSafetyConfig;

const MAIL_APPEND_EVENT_BUFFER: usize = 128;

pub struct DaemonState {
    pub store: SqliteStore,
    pub(crate) daemon_version: String,
    pub(crate) runtime: Arc<dyn RuntimePort>,
    pub(crate) runtime_service: Arc<RuntimeService>,
    pub(crate) identity: Arc<dyn IdentityPort>,
    pub(crate) mail_safety: MailSafetyConfig,
    mail_append_events: broadcast::Sender<MailAppendEvent>,
}

pub struct HandlerResult {
    pub response: RpcResponse,
    pub shutdown: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MailAppendEvent {
    pub message_id: Uuid,
}

impl DaemonState {
    pub fn new(
        store: SqliteStore,
        daemon_version: impl Into<String>,
        runtime: Arc<dyn RuntimePort>,
        identity: Arc<dyn IdentityPort>,
        runtime_service: Arc<RuntimeService>,
    ) -> Self {
        Self {
            store,
            daemon_version: daemon_version.into(),
            runtime,
            runtime_service,
            identity,
            mail_safety: MailSafetyConfig::load(),
            mail_append_events: broadcast::channel(MAIL_APPEND_EVENT_BUFFER).0,
        }
    }

    pub fn subscribe_mail_appends(&self) -> broadcast::Receiver<MailAppendEvent> {
        self.mail_append_events.subscribe()
    }

    #[doc(hidden)]
    pub fn set_mail_safety_limits(
        &mut self,
        conversation_depth_limit: usize,
        sender_rate_limit: usize,
        sender_rate_window_secs: i64,
    ) {
        self.mail_safety = MailSafetyConfig::from_limits(
            conversation_depth_limit,
            sender_rate_limit,
            sender_rate_window_secs,
        );
    }

    pub(crate) fn emit_mail_append(&self, message_id: Uuid) {
        let _ = self.mail_append_events.send(MailAppendEvent { message_id });
    }
}
