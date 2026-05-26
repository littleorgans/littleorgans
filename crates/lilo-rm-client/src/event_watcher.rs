use lilo_rm_core::{EventBatch, EventCursor, EventsRequest};

use crate::{ClientError, RuntimeClient};

const DEFAULT_WAIT_MS: u32 = 30_000;

/// Builder for [`EventWatcher`].
#[derive(Clone, Debug, Default)]
pub struct EventWatcherBuilder {
    cursor: Option<EventCursor>,
    wait_ms: Option<u32>,
}

impl EventWatcherBuilder {
    /// Resume watching from `cursor`.
    #[must_use]
    pub fn since(mut self, cursor: EventCursor) -> Self {
        self.cursor = Some(cursor);
        self
    }

    /// Set the long poll wait window in milliseconds.
    #[must_use]
    pub fn wait_ms(mut self, ms: u32) -> Self {
        self.wait_ms = Some(ms);
        self
    }

    /// Build a watcher without touching the daemon.
    pub fn build(self, client: RuntimeClient) -> EventWatcher {
        EventWatcher {
            client,
            cursor: self.cursor,
            wait_ms: self.wait_ms.or(Some(DEFAULT_WAIT_MS)),
        }
    }

    /// Check daemon protocol compatibility, then build a watcher.
    pub async fn connect(self, client: RuntimeClient) -> Result<EventWatcher, ClientError> {
        client.check_protocol_version().await?;
        Ok(self.build(client))
    }
}

/// Long poll event consumer with caller visible cursor state.
#[derive(Clone, Debug)]
pub struct EventWatcher {
    client: RuntimeClient,
    cursor: Option<EventCursor>,
    wait_ms: Option<u32>,
}

impl EventWatcher {
    /// Start building an event watcher.
    pub fn builder() -> EventWatcherBuilder {
        EventWatcherBuilder::default()
    }

    /// Return the cursor from the last successful batch.
    ///
    /// Callers persist this value after applying each batch, then pass it to
    /// [`EventWatcherBuilder::since`] when rebuilding a watcher.
    pub fn current_cursor(&self) -> Option<&EventCursor> {
        self.cursor.as_ref()
    }

    /// Override the next request cursor.
    pub fn seek(&mut self, cursor: EventCursor) {
        self.cursor = Some(cursor);
    }

    /// Fetch the next event batch and advance this watcher's cursor.
    pub async fn next(&mut self) -> Result<EventBatch, ClientError> {
        let batch = self
            .client
            .events(EventsRequest {
                since: self.cursor,
                wait_ms: self.wait_ms,
            })
            .await?;
        self.update_cursor(&batch);
        Ok(batch)
    }

    fn update_cursor(&mut self, batch: &EventBatch) {
        match batch {
            EventBatch::Events { cursor, .. } => {
                self.cursor = Some(*cursor);
            }
            EventBatch::CursorExpired { oldest } => {
                self.cursor = Some(*oldest);
            }
            _ => {}
        }
    }
}
