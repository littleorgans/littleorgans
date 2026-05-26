use anyhow::Result;
use lilo_rm_core::{EventsRequest, RuntimeEvent};

use crate::event_log::{CursorExpired, EventLog, EventLogPage};

pub(super) struct EventAppender {
    event_log: EventLog,
}

impl EventAppender {
    pub(super) fn new(event_log: EventLog) -> Self {
        Self { event_log }
    }

    pub(super) async fn events(
        &self,
        request: EventsRequest,
    ) -> std::result::Result<EventLogPage, CursorExpired> {
        self.event_log
            .events_since_or_wait(request.since, request.wait_ms)
            .await
    }

    pub(super) async fn append_event(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        self.event_log.append(event).await
    }

    pub(super) fn event_waiter_count(&self) -> usize {
        self.event_log.waiter_count()
    }
}
