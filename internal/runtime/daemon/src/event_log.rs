use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use lilo_rm_core::{
    EVENT_LOG_RETENTION_MIN_AGE_SECS, EVENT_LOG_RETENTION_MIN_EVENTS, EventCursor, RuntimeEvent,
    clamped_event_wait_ms,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

const EVENT_LOG_SYNC_BATCH: usize = 32;
const EVENT_LOG_SYNC_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub(crate) struct EventLogPage {
    pub(crate) events: Vec<RuntimeEvent>,
    pub(crate) cursor: EventCursor,
}

#[derive(Debug)]
pub(crate) struct CursorExpired {
    pub(crate) oldest: EventCursor,
}

pub(crate) struct EventLog {
    path: PathBuf,
    inner: Mutex<EventLogInner>,
    append_notify: Notify,
    waiter_count: AtomicUsize,
}

struct EventLogInner {
    file: File,
    events: Vec<EventLogEntry>,
    seen_event_keys: HashSet<EventLogKey>,
    next_seq: EventCursor,
    events_since_sync: usize,
    last_sync: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct EventLogKey {
    session_id: Uuid,
    kind: EventLogKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EventLogKind {
    Running,
    Terminated,
    Lost,
}

impl EventLogKey {
    fn from_event(event: &RuntimeEvent) -> Self {
        match event {
            RuntimeEvent::Running { session_id, .. } => Self {
                session_id: *session_id,
                kind: EventLogKind::Running,
            },
            RuntimeEvent::Terminated { session_id, .. } => Self {
                session_id: *session_id,
                kind: EventLogKind::Terminated,
            },
            RuntimeEvent::Lost { session_id, .. } => Self {
                session_id: *session_id,
                kind: EventLogKind::Lost,
            },
        }
    }
}

#[derive(Clone)]
struct EventLogEntry {
    seq: EventCursor,
    ts_ms: u64,
    event: RuntimeEvent,
}

#[derive(Deserialize, Serialize)]
struct EventLogRecord {
    seq: EventCursor,
    ts_ms: u64,
    kind: String,
    payload: Value,
}

impl EventLog {
    pub(crate) fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let path = lilo_paths::event_log_path(data_dir.as_ref());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        recover_partial_tail(&path)?;
        let events = read_entries(&path)?;
        let seen_event_keys = events
            .iter()
            .map(|entry| EventLogKey::from_event(&entry.event))
            .collect();
        let next_seq = events.last().map_or(1, |entry| entry.seq.saturating_add(1));
        let file = open_append_file(&path)?;
        Ok(Self {
            path,
            append_notify: Notify::new(),
            waiter_count: AtomicUsize::new(0),
            inner: Mutex::new(EventLogInner {
                file,
                events,
                seen_event_keys,
                next_seq,
                events_since_sync: 0,
                last_sync: Instant::now(),
            }),
        })
    }

    pub(crate) async fn append(&self, event: RuntimeEvent) -> Result<RuntimeEvent> {
        let mut inner = self.inner.lock().await;
        if !inner
            .seen_event_keys
            .insert(EventLogKey::from_event(&event))
        {
            return Ok(event);
        }
        let entry = EventLogEntry {
            seq: inner.next_seq,
            ts_ms: current_timestamp_ms()?,
            event,
        };
        inner.next_seq = inner.next_seq.saturating_add(1);
        let record = EventLogRecord::from_entry(&entry)?;
        serde_json::to_writer(&mut inner.file, &record).context("failed to encode event log")?;
        inner
            .file
            .write_all(b"\n")
            .context("failed to append event log newline")?;
        inner.events.push(entry.clone());
        inner.events_since_sync += 1;
        sync_if_due(&mut inner)?;
        compact_if_due(&self.path, &mut inner)?;
        drop(inner);
        self.append_notify.notify_waiters();
        Ok(entry.event)
    }

    pub(crate) async fn events_since(
        &self,
        since: Option<EventCursor>,
    ) -> std::result::Result<EventLogPage, CursorExpired> {
        let cursor = since.unwrap_or_default();
        let inner = self.inner.lock().await;
        if let Some(oldest) = oldest_valid_cursor(&inner.events)
            && cursor < oldest
        {
            return Err(CursorExpired { oldest });
        }
        let events = inner
            .events
            .iter()
            .filter(|entry| entry.seq > cursor)
            .map(|entry| entry.event.clone())
            .collect();
        Ok(EventLogPage {
            events,
            cursor: inner.events.last().map_or(cursor, |entry| entry.seq),
        })
    }

    pub(crate) async fn events_since_or_wait(
        &self,
        since: Option<EventCursor>,
        wait_ms: Option<u32>,
    ) -> std::result::Result<EventLogPage, CursorExpired> {
        let wait_ms = clamped_event_wait_ms(wait_ms);
        let immediate = self.events_since(since).await?;
        if wait_ms == 0 || !immediate.events.is_empty() {
            return Ok(immediate);
        }

        let notified = self.append_notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        let second_check = self.events_since(since).await?;
        if !second_check.events.is_empty() {
            return Ok(second_check);
        }

        let _guard = EventWaiterGuard::new(&self.waiter_count);
        tokio::select! {
            () = notified => self.events_since(since).await,
            () = tokio::time::sleep(Duration::from_millis(u64::from(wait_ms))) => Ok(second_check),
        }
    }

    pub(crate) fn waiter_count(&self) -> usize {
        self.waiter_count.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    pub(crate) async fn append_with_ts(
        &self,
        event: RuntimeEvent,
        ts_ms: u64,
    ) -> Result<RuntimeEvent> {
        let mut inner = self.inner.lock().await;
        if !inner
            .seen_event_keys
            .insert(EventLogKey::from_event(&event))
        {
            return Ok(event);
        }
        let entry = EventLogEntry {
            seq: inner.next_seq,
            ts_ms,
            event,
        };
        inner.next_seq = inner.next_seq.saturating_add(1);
        let record = EventLogRecord::from_entry(&entry)?;
        serde_json::to_writer(&mut inner.file, &record).context("failed to encode event log")?;
        inner.file.write_all(b"\n")?;
        inner.events.push(entry.clone());
        compact_if_due(&self.path, &mut inner)?;
        drop(inner);
        self.append_notify.notify_waiters();
        Ok(entry.event)
    }
}

struct EventWaiterGuard<'a> {
    waiter_count: &'a AtomicUsize,
}

impl<'a> EventWaiterGuard<'a> {
    fn new(waiter_count: &'a AtomicUsize) -> Self {
        waiter_count.fetch_add(1, Ordering::SeqCst);
        Self { waiter_count }
    }
}

impl Drop for EventWaiterGuard<'_> {
    fn drop(&mut self) {
        self.waiter_count.fetch_sub(1, Ordering::SeqCst);
    }
}

impl EventLogRecord {
    fn from_entry(entry: &EventLogEntry) -> Result<Self> {
        let value = serde_json::to_value(&entry.event).context("failed to encode event")?;
        let mut object = value
            .as_object()
            .cloned()
            .ok_or_else(|| anyhow!("event encoded as non-object"))?;
        let kind = object
            .remove("type")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or_else(|| anyhow!("event encoded without type"))?;
        let payload = object.remove("payload").unwrap_or(Value::Null);
        Ok(Self {
            seq: entry.seq,
            ts_ms: entry.ts_ms,
            kind,
            payload,
        })
    }

    fn into_entry(self) -> Result<EventLogEntry> {
        let event = serde_json::from_value(serde_json::json!({
            "type": self.kind,
            "payload": self.payload,
        }))
        .context("failed to decode event log record")?;
        Ok(EventLogEntry {
            seq: self.seq,
            ts_ms: self.ts_ms,
            event,
        })
    }
}

fn open_append_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .create(true)
        .append(true)
        .read(true)
        .open(path)
        .with_context(|| format!("failed to open {}", path.display()))
}

fn recover_partial_tail(path: &Path) -> Result<()> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(());
    };
    if bytes.last().is_none_or(|byte| *byte == b'\n') {
        return Ok(());
    }
    let len = bytes
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or_default();
    OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("failed to open {} for recovery", path.display()))?
        .set_len(len as u64)
        .with_context(|| format!("failed to truncate {}", path.display()))
}

fn read_entries(path: &Path) -> Result<Vec<EventLogEntry>> {
    let file = open_append_file(path)?;
    let reader = BufReader::new(file);
    reader
        .lines()
        .map(|line| {
            let line = line.context("failed to read event log line")?;
            serde_json::from_str::<EventLogRecord>(&line)
                .context("failed to parse event log line")?
                .into_entry()
        })
        .collect()
}

fn sync_if_due(inner: &mut EventLogInner) -> Result<()> {
    if inner.events_since_sync < EVENT_LOG_SYNC_BATCH
        && inner.last_sync.elapsed() < EVENT_LOG_SYNC_INTERVAL
    {
        return Ok(());
    }
    inner.file.sync_data().context("failed to sync event log")?;
    inner.events_since_sync = 0;
    inner.last_sync = Instant::now();
    Ok(())
}

fn compact_if_due(path: &Path, inner: &mut EventLogInner) -> Result<()> {
    let Some(retain_from) = retain_from_index(&inner.events)? else {
        return Ok(());
    };
    let retained = inner.events.split_off(retain_from);
    let tmp = path.with_extension("jsonl.tmp");
    {
        let mut file =
            File::create(&tmp).with_context(|| format!("failed to create {}", tmp.display()))?;
        for entry in &retained {
            let record = EventLogRecord::from_entry(entry)?;
            serde_json::to_writer(&mut file, &record).context("failed to compact event log")?;
            file.write_all(b"\n")
                .context("failed to compact event log newline")?;
        }
        file.sync_all()
            .with_context(|| format!("failed to sync {}", tmp.display()))?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    inner.file = open_append_file(path)?;
    inner.events = retained;
    inner.events_since_sync = 0;
    inner.last_sync = Instant::now();
    Ok(())
}

fn retain_from_index(events: &[EventLogEntry]) -> Result<Option<usize>> {
    let Some(extra_events) = events.len().checked_sub(EVENT_LOG_RETENTION_MIN_EVENTS) else {
        return Ok(None);
    };
    let now_ms = current_timestamp_ms()?;
    let max_age_ms = EVENT_LOG_RETENTION_MIN_AGE_SECS * 1_000;
    let old_events = events
        .iter()
        .take_while(|entry| now_ms.saturating_sub(entry.ts_ms) > max_age_ms)
        .count();
    let compact_count = extra_events.min(old_events);
    Ok((compact_count > 0).then_some(compact_count))
}

fn current_timestamp_ms() -> Result<u64> {
    Utc::now()
        .timestamp_millis()
        .try_into()
        .context("current timestamp is before Unix epoch")
}

fn oldest_valid_cursor(events: &[EventLogEntry]) -> Option<EventCursor> {
    events.first().map(|entry| entry.seq.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;
    use uuid::Uuid;

    #[tokio::test]
    async fn replay_survives_reopen() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        log.append(running_event()).await.expect("append");
        let batch = EventLog::open(temp.path())
            .expect("reopen")
            .events_since(Some(0))
            .await
            .expect("events");

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.cursor, 1);
    }

    #[tokio::test]
    async fn append_dedups_by_session_and_event_kind() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        let event = running_event();
        log.append(event.clone()).await.expect("append first");
        log.append(event).await.expect("append duplicate");

        let batch = log.events_since(Some(0)).await.expect("events");
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.cursor, 1);
    }

    #[tokio::test]
    async fn recovery_drops_partial_tail() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        log.append(running_event()).await.expect("append");
        drop(log);
        let path = lilo_paths::event_log_path(temp.path());
        let mut file = OpenOptions::new().append(true).open(&path).expect("append");
        file.write_all(br#"{"seq":2"#).expect("corrupt");
        drop(file);

        let batch = EventLog::open(temp.path())
            .expect("reopen")
            .events_since(Some(0))
            .await
            .expect("events");

        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.cursor, 1);
    }

    #[tokio::test]
    async fn compaction_requires_age_and_count() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        let old_ms = old_timestamp_ms();
        for index in 0..=EVENT_LOG_RETENTION_MIN_EVENTS {
            log.append_with_ts(running_event_for(index), old_ms)
                .await
                .expect("append");
        }

        let expired = log.events_since(Some(0)).await.expect_err("cursor expired");

        assert_eq!(expired.oldest, 1);
    }

    #[tokio::test]
    async fn compaction_does_not_truncate_for_count_alone() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        let event_count = EVENT_LOG_RETENTION_MIN_EVENTS + 1;
        for index in 0..event_count {
            log.append(running_event_for(index)).await.expect("append");
        }

        assert_all_events_readable_from_start(&log, event_count).await;
    }

    #[tokio::test]
    async fn compaction_does_not_truncate_for_age_alone() {
        let temp = TempDir::new().expect("temp");
        let log = EventLog::open(temp.path()).expect("open");
        let old_ms = old_timestamp_ms();
        let event_count = EVENT_LOG_RETENTION_MIN_EVENTS - 1;
        for index in 0..event_count {
            log.append_with_ts(running_event_for(index), old_ms)
                .await
                .expect("append");
        }

        assert_all_events_readable_from_start(&log, event_count).await;
    }

    async fn assert_all_events_readable_from_start(log: &EventLog, event_count: usize) {
        let batch = log
            .events_since(Some(0))
            .await
            .expect("events remain readable from start");

        assert_eq!(batch.events.len(), event_count);
        assert_eq!(batch.cursor, event_count as EventCursor);
    }

    fn old_timestamp_ms() -> u64 {
        u64::try_from((Utc::now() - chrono::Duration::days(8)).timestamp_millis())
            .expect("test timestamp is after Unix epoch")
    }

    fn running_event() -> RuntimeEvent {
        running_event_for(1)
    }

    fn running_event_for(index: usize) -> RuntimeEvent {
        RuntimeEvent::Running {
            session_id: Uuid::parse_str(&format!("018f6e28-0000-7000-8000-{index:012}")).unwrap(),
            runtime_pid: 42,
            start_time: Utc.timestamp_opt(1_700_000_000, 0).unwrap(),
        }
    }
}
