use chrono::Utc;
use lilo_rm_core::{EventCursor, LostEvidence, RuntimeEvent, TerminationEvidence};
use sqlx::{Row, Sqlite, Transaction};

use super::SqliteStore;

impl SqliteStore {
    pub async fn event_cursor(&self) -> sqlx::Result<Option<EventCursor>> {
        let value = sqlx::query("SELECT cursor FROM session_event_cursor WHERE id = 1")
            .fetch_optional(&self.pool)
            .await?
            .map(|row| row.try_get::<Vec<u8>, _>("cursor"))
            .transpose()?;
        value.map(|cursor| decode_cursor(&cursor)).transpose()
    }

    pub async fn apply_cursor(&self, cursor: EventCursor) -> sqlx::Result<()> {
        let mut transaction = self.pool.begin().await?;
        write_cursor(&mut transaction, cursor).await?;
        transaction.commit().await
    }

    pub async fn apply_runtime_events_and_cursor(
        &self,
        events: &[RuntimeEvent],
        next_cursor: EventCursor,
    ) -> sqlx::Result<()> {
        let mut transaction = self.pool.begin().await?;
        for event in events {
            apply_runtime_event(&mut transaction, event).await?;
        }
        write_cursor(&mut transaction, next_cursor).await?;
        transaction.commit().await
    }
}

async fn apply_runtime_event(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RuntimeEvent,
) -> sqlx::Result<()> {
    match event {
        RuntimeEvent::Running {
            session_id,
            runtime_pid,
            start_time,
        } => sqlx::query(
            "UPDATE session_sessions
             SET state = 'RUNNING',
                 runtime_pid = ?,
                 started_at = ?,
                 updated_at = ?
             WHERE id = ?
               AND state IN ('SPAWNING', 'RUNNING')
               AND (state = 'SPAWNING' OR runtime_pid != ?)",
        )
        .bind(runtime_pid)
        .bind(start_time.to_rfc3339())
        .bind(Utc::now().to_rfc3339())
        .bind(session_id.to_string())
        .bind(runtime_pid)
        .execute(&mut **transaction)
        .await?
        .rows_affected(),
        RuntimeEvent::Terminated {
            session_id,
            exit_code,
            signal: _,
            evidence,
        } => {
            if let TerminationEvidence::Lost(lost_evidence) = evidence {
                mark_lost(transaction, &session_id.to_string(), *lost_evidence).await?
            } else {
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "UPDATE session_sessions
             SET state = 'TERMINATED',
                 lost_evidence = NULL,
                 exit_code = ?,
                 terminated_at = ?,
                 updated_at = ?
             WHERE id = ?
               AND state IN ('SPAWNING', 'RUNNING')",
                )
                .bind(exit_code)
                .bind(&now)
                .bind(&now)
                .bind(session_id.to_string())
                .execute(&mut **transaction)
                .await?
                .rows_affected()
            }
        }
        RuntimeEvent::Lost {
            session_id,
            evidence,
        } => mark_lost(transaction, &session_id.to_string(), *evidence).await?,
    };
    Ok(())
}

async fn mark_lost(
    transaction: &mut Transaction<'_, Sqlite>,
    session_id: &str,
    evidence: LostEvidence,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE session_sessions
         SET state = 'LOST',
             lost_evidence = ?,
             updated_at = ?
         WHERE id = ?
           AND state IN ('SPAWNING', 'RUNNING')",
    )
    .bind(lost_evidence_to_sql(evidence))
    .bind(Utc::now().to_rfc3339())
    .bind(session_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

async fn write_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    cursor: EventCursor,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO session_event_cursor (id, cursor, updated_at)
         VALUES (1, ?, ?)
         ON CONFLICT(id) DO UPDATE
         SET cursor = excluded.cursor,
             updated_at = excluded.updated_at",
    )
    .bind(cursor.to_be_bytes().to_vec())
    .bind(Utc::now().to_rfc3339())
    .execute(&mut **transaction)
    .await?;
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

fn decode_cursor(value: &[u8]) -> sqlx::Result<EventCursor> {
    let bytes: [u8; 8] = value
        .try_into()
        .map_err(|error| sqlx::Error::ColumnDecode {
            index: "cursor".to_string(),
            source: Box::new(error),
        })?;
    Ok(EventCursor::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};
    use chrono::Utc;
    use lilo_rm_core::{RuntimeEvent, TerminationEvidence};
    use lilo_session_core::SessionState;

    use super::super::test_support::running_session;
    use super::*;

    #[tokio::test]
    async fn applies_runtime_events_and_cursor_atomically() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let session = running_session("general", "test");
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");

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
            .await
            .or_panic("events apply");

        let updated = store
            .get_session(&session.id)
            .await
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(updated.state, SessionState::Terminated);
        assert_eq!(updated.runtime_pid, 101);
        assert_eq!(updated.exit_code, Some(7));
        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(42)
        );
    }

    #[tokio::test]
    async fn duplicate_running_event_keeps_existing_running_session_timestamps() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let session = running_session("general", "test");
        let original_started_at = session.started_at;
        let original_updated_at = session.updated_at;
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");

        store
            .apply_runtime_events_and_cursor(
                &[RuntimeEvent::Running {
                    session_id: session.id,
                    runtime_pid: session.runtime_pid,
                    start_time: original_started_at + chrono::Duration::seconds(10),
                }],
                43,
            )
            .await
            .or_panic("events apply");

        let updated = store
            .get_session(&session.id)
            .await
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(updated.runtime_pid, session.runtime_pid);
        assert_eq!(updated.started_at, original_started_at);
        assert_eq!(updated.updated_at, original_updated_at);
        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(43)
        );
    }

    #[tokio::test]
    async fn persists_lost_evidence_from_runtime_events() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let session = running_session("general", "test");
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");

        store
            .apply_runtime_events_and_cursor(
                &[RuntimeEvent::Lost {
                    session_id: session.id,
                    evidence: LostEvidence::PidReuseDetected,
                }],
                9,
            )
            .await
            .or_panic("lost event applies");

        let updated = store
            .get_session(&session.id)
            .await
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(
            updated.state,
            SessionState::Lost {
                evidence: LostEvidence::PidReuseDetected
            }
        );
        assert_eq!(store.event_cursor().await.or_panic("cursor loads"), Some(9));
    }

    #[tokio::test]
    async fn rolls_back_events_when_cursor_write_fails() {
        let (_dir, store) = SqliteStore::open_temp().await;
        let session = running_session("general", "test");
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
        sqlx::query(
            "CREATE TRIGGER fail_event_cursor_insert
                 BEFORE INSERT ON session_event_cursor
                 BEGIN
                     SELECT RAISE(ABORT, 'cursor write failed');
                 END",
        )
        .execute(store.pool())
        .await
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
            .await
            .err_or_panic("cursor conversion fails");

        assert!(matches!(error, sqlx::Error::Database(_)));
        let unchanged = store
            .get_session(&session.id)
            .await
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(unchanged.state, SessionState::Running);
        assert_eq!(unchanged.exit_code, None);
        assert_eq!(store.event_cursor().await.or_panic("cursor loads"), None);
    }

    #[tokio::test]
    async fn applies_cursor_without_events() {
        let (_dir, store) = SqliteStore::open_temp().await;

        store.apply_cursor(77).await.or_panic("cursor applies");

        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(77)
        );
    }

    #[tokio::test]
    async fn persists_cursor_across_reopen() {
        let dir = tempfile::tempdir().or_panic("tempdir creates");
        let db_path = dir.path().join("store.sqlite");
        {
            let db = lilo_db::LiloDb::open_path(&db_path)
                .await
                .or_panic("db opens");
            let store = SqliteStore::open(&db);
            store.apply_cursor(42).await.or_panic("cursor applies");
        }

        let db = lilo_db::LiloDb::open_path(&db_path)
            .await
            .or_panic("db reopens");
        let store = SqliteStore::open(&db);

        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(42)
        );
    }
}
