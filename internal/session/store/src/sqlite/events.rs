use chrono::Utc;
use lilo_rm_core::{EventCursor, LostEvidence, RuntimeEvent, TerminationEvidence};
use rusqlite::types::Type;
use rusqlite::{OptionalExtension, params};

use super::SqliteStore;

impl SqliteStore {
    pub fn event_cursor(&self) -> rusqlite::Result<Option<EventCursor>> {
        self.connection
            .query_row("SELECT cursor FROM event_cursor WHERE id = 1", [], |row| {
                let value: Vec<u8> = row.get(0)?;
                decode_cursor(&value)
            })
            .optional()
    }

    pub fn apply_cursor(&mut self, cursor: EventCursor) -> rusqlite::Result<()> {
        let transaction = self.connection.transaction()?;
        write_cursor(&transaction, cursor)?;
        transaction.commit()
    }

    pub fn apply_runtime_events_and_cursor(
        &mut self,
        events: &[RuntimeEvent],
        next_cursor: EventCursor,
    ) -> rusqlite::Result<()> {
        let transaction = self.connection.transaction()?;
        for event in events {
            apply_runtime_event(&transaction, event)?;
        }
        write_cursor(&transaction, next_cursor)?;
        transaction.commit()
    }
}

fn apply_runtime_event(
    transaction: &rusqlite::Transaction<'_>,
    event: &RuntimeEvent,
) -> rusqlite::Result<()> {
    match event {
        RuntimeEvent::Running {
            session_id,
            runtime_pid,
            start_time,
        } => transaction.execute(
            "UPDATE sessions
             SET state = 'RUNNING',
                 runtime_pid = ?1,
                 started_at = ?2,
                 updated_at = ?3
             WHERE id = ?4
               AND state IN ('SPAWNING', 'RUNNING')",
            params![
                runtime_pid,
                start_time.to_rfc3339(),
                Utc::now().to_rfc3339(),
                session_id.to_string(),
            ],
        )?,
        RuntimeEvent::Terminated {
            session_id,
            exit_code,
            signal: _,
            evidence,
        } => match evidence {
            TerminationEvidence::Lost(lost_evidence) => {
                mark_lost(transaction, &session_id.to_string(), *lost_evidence)?
            }
            _ => transaction.execute(
                "UPDATE sessions
                 SET state = 'TERMINATED',
                     lost_evidence = NULL,
                     exit_code = ?1,
                     terminated_at = ?2,
                     updated_at = ?2
                 WHERE id = ?3
                   AND state IN ('SPAWNING', 'RUNNING')",
                params![exit_code, Utc::now().to_rfc3339(), session_id.to_string()],
            )?,
        },
        RuntimeEvent::Lost {
            session_id,
            evidence,
        } => mark_lost(transaction, &session_id.to_string(), *evidence)?,
    };
    Ok(())
}

fn mark_lost(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
    evidence: LostEvidence,
) -> rusqlite::Result<usize> {
    transaction.execute(
        "UPDATE sessions
         SET state = 'LOST',
             lost_evidence = ?1,
             updated_at = ?2
         WHERE id = ?3
           AND state IN ('SPAWNING', 'RUNNING')",
        params![
            lost_evidence_to_sql(evidence),
            Utc::now().to_rfc3339(),
            session_id,
        ],
    )
}

fn write_cursor(
    transaction: &rusqlite::Transaction<'_>,
    cursor: EventCursor,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO event_cursor (id, cursor, updated_at)
         VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE
         SET cursor = excluded.cursor,
             updated_at = excluded.updated_at",
        params![cursor.to_be_bytes().to_vec(), Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

pub(crate) fn lost_evidence_from_sql(value: &str) -> Option<LostEvidence> {
    match value {
        "shim_died_before_report" => Some(LostEvidence::ShimDiedBeforeReport),
        "pid_not_alive" => Some(LostEvidence::PidNotAlive),
        "pid_reuse_detected" => Some(LostEvidence::PidReuseDetected),
        _ => None,
    }
}

pub(crate) fn lost_evidence_to_sql(evidence: LostEvidence) -> &'static str {
    match evidence {
        LostEvidence::ShimDiedBeforeReport => "shim_died_before_report",
        LostEvidence::PidNotAlive => "pid_not_alive",
        LostEvidence::PidReuseDetected => "pid_reuse_detected",
        _ => "unknown",
    }
}

fn decode_cursor(value: &[u8]) -> rusqlite::Result<EventCursor> {
    let bytes: [u8; 8] = value.try_into().map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, Type::Blob, Box::new(error))
    })?;
    Ok(EventCursor::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};
    use chrono::Utc;
    use lilo_rm_core::{RuntimeEvent, TerminationEvidence};
    use lilo_session_core::{Label, Namespace, RuntimeKind, Session, SessionState};
    use uuid::Uuid;

    use super::*;

    #[test]
    fn applies_runtime_events_and_cursor_atomically() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");
        let session = test_session();
        store.insert_session(&session).or_panic("session inserts");

        store
            .apply_runtime_events_and_cursor(
                &[
                    RuntimeEvent::Running {
                        session_id: session.id,
                        runtime_pid: 101,
                        start_time: Utc::now(),
                    },
                    RuntimeEvent::Terminated {
                        session_id: session.id,
                        exit_code: Some(7),
                        signal: None,
                        evidence: TerminationEvidence::ProcessExit,
                    },
                ],
                42,
            )
            .or_panic("events apply");

        let updated = store
            .get_session(&session.id)
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(updated.state, SessionState::Terminated);
        assert_eq!(updated.runtime_pid, 101);
        assert_eq!(updated.exit_code, Some(7));
        assert_eq!(store.event_cursor().or_panic("cursor loads"), Some(42));
    }

    #[test]
    fn persists_lost_evidence_from_runtime_events() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");
        let session = test_session();
        store.insert_session(&session).or_panic("session inserts");

        store
            .apply_runtime_events_and_cursor(
                &[RuntimeEvent::Lost {
                    session_id: session.id,
                    evidence: LostEvidence::PidReuseDetected,
                }],
                9,
            )
            .or_panic("lost event applies");

        let updated = store
            .get_session(&session.id)
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(
            updated.state,
            SessionState::Lost {
                evidence: LostEvidence::PidReuseDetected
            }
        );
        assert_eq!(store.event_cursor().or_panic("cursor loads"), Some(9));
    }

    #[test]
    fn rolls_back_events_when_cursor_write_fails() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");
        let session = test_session();
        store.insert_session(&session).or_panic("session inserts");
        store
            .connection
            .execute(
                "CREATE TRIGGER fail_event_cursor_insert
                 BEFORE INSERT ON event_cursor
                 BEGIN
                     SELECT RAISE(ABORT, 'cursor write failed');
                 END",
                [],
            )
            .or_panic("trigger creates");

        let error = store
            .apply_runtime_events_and_cursor(
                &[RuntimeEvent::Terminated {
                    session_id: session.id,
                    exit_code: Some(1),
                    signal: None,
                    evidence: TerminationEvidence::ShimExit,
                }],
                1,
            )
            .err_or_panic("cursor conversion fails");

        assert!(matches!(error, rusqlite::Error::SqliteFailure(_, _)));
        let unchanged = store
            .get_session(&session.id)
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(unchanged.state, SessionState::Running);
        assert_eq!(unchanged.exit_code, None);
        assert_eq!(store.event_cursor().or_panic("cursor loads"), None);
    }

    #[test]
    fn applies_cursor_without_events() {
        let mut store = SqliteStore::open_in_memory().or_panic("store opens");

        store.apply_cursor(77).or_panic("cursor applies");

        assert_eq!(store.event_cursor().or_panic("cursor loads"), Some(77));
    }

    #[test]
    fn persists_cursor_across_reopen() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let db_path = dir.path().join("store.sqlite");
        {
            let mut store = SqliteStore::open(&db_path).or_panic("store opens");
            store.apply_cursor(42).or_panic("cursor applies");
        }

        let store = SqliteStore::open(&db_path).or_panic("store reopens");

        assert_eq!(store.event_cursor().or_panic("cursor loads"), Some(42));
    }

    fn test_session() -> Session {
        let now = Utc::now();
        Session {
            id: Uuid::now_v7(),
            runtime: RuntimeKind::Claude,
            role: "general".to_string(),
            workspace: "test".to_string(),
            namespace: Namespace::default(),
            dir: "test".into(),
            state: SessionState::Running,
            runtime_pid: 42,
            runtime_session: None,
            transcript_path: None,
            tmux_pane: None,
            agent_config: None,
            created_at: now,
            started_at: now,
            terminated_at: None,
            exit_code: None,
            updated_at: now,
            labels: Vec::<Label>::new(),
        }
    }
}
