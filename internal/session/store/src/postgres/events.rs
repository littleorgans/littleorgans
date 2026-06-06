use chrono::Utc;
use lilo_rm_core::{EventCursor, LostEvidence, RuntimeEvent, TerminationEvidence};
use sqlx::{Postgres, Row, Transaction};

use super::SessionStore;

impl SessionStore {
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
    transaction: &mut Transaction<'_, Postgres>,
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
                 runtime_pid = $1,
                 started_at = $2,
                 updated_at = $3
             WHERE id = $4
               AND state IN ('SPAWNING', 'RUNNING')
               AND (state = 'SPAWNING' OR runtime_pid != $5)",
        )
        .bind(i64::from(*runtime_pid))
        .bind(start_time)
        .bind(Utc::now())
        .bind(session_id.to_string())
        .bind(i64::from(*runtime_pid))
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
                let now = Utc::now();
                sqlx::query(
                    "UPDATE session_sessions
             SET state = 'TERMINATED',
                 lost_evidence = NULL,
                 exit_code = $1,
                 terminated_at = $2,
                 updated_at = $3
             WHERE id = $4
               AND state IN ('SPAWNING', 'RUNNING')",
                )
                .bind(exit_code.map(i64::from))
                .bind(now)
                .bind(now)
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
    transaction: &mut Transaction<'_, Postgres>,
    session_id: &str,
    evidence: LostEvidence,
) -> sqlx::Result<u64> {
    let result = sqlx::query(
        "UPDATE session_sessions
         SET state = 'LOST',
             lost_evidence = $1,
             updated_at = $2
         WHERE id = $3
           AND state IN ('SPAWNING', 'RUNNING')",
    )
    .bind(lost_evidence_to_sql(evidence))
    .bind(Utc::now())
    .bind(session_id)
    .execute(&mut **transaction)
    .await?;
    Ok(result.rows_affected())
}

async fn write_cursor(
    transaction: &mut Transaction<'_, Postgres>,
    cursor: EventCursor,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO session_event_cursor (id, cursor, updated_at)
         VALUES (1, $1, $2)
         ON CONFLICT(id) DO UPDATE
         SET cursor = excluded.cursor,
             updated_at = excluded.updated_at",
    )
    .bind(cursor.to_be_bytes().to_vec())
    .bind(Utc::now())
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
    use lilo_db::test_support::TestDb;
    use lilo_rm_core::{RuntimeEvent, TerminationEvidence};
    use lilo_session_core::SessionState;

    use super::super::test_support::running_session;
    use super::*;

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn applies_runtime_events_and_cursor_atomically() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        let store = SessionStore::from_db(testdb.db());
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
        testdb.cleanup().await.or_panic("test db cleans up");
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn duplicate_running_event_keeps_existing_running_session_timestamps() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        let store = SessionStore::from_db(testdb.db());
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
        testdb.cleanup().await.or_panic("test db cleans up");
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn persists_lost_evidence_from_runtime_events() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        let store = SessionStore::from_db(testdb.db());
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
        testdb.cleanup().await.or_panic("test db cleans up");
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn rolls_back_events_when_cursor_write_fails() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        let store = SessionStore::from_db(testdb.db());
        let session = running_session("general", "test");
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
        sqlx::query(
            "CREATE FUNCTION fail_event_cursor_insert() RETURNS trigger AS $$
                 BEGIN
                     RAISE EXCEPTION 'cursor write failed';
                 END;
             $$ LANGUAGE plpgsql",
        )
        .execute(store.pool())
        .await
        .or_panic("trigger function creates");
        sqlx::query(
            "CREATE TRIGGER fail_event_cursor_insert
                 BEFORE INSERT ON session_event_cursor
                 FOR EACH ROW EXECUTE FUNCTION fail_event_cursor_insert()",
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
        testdb.cleanup().await.or_panic("test db cleans up");
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn applies_cursor_without_events() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        let store = SessionStore::from_db(testdb.db());

        store.apply_cursor(77).await.or_panic("cursor applies");

        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(77)
        );
        testdb.cleanup().await.or_panic("test db cleans up");
    }

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn persists_cursor_across_store_handles() {
        let testdb = TestDb::create().await.or_panic("test db creates");
        {
            let store = SessionStore::from_db(testdb.db());
            store.apply_cursor(42).await.or_panic("cursor applies");
        }

        let store = SessionStore::from_db(testdb.db());

        assert_eq!(
            store.event_cursor().await.or_panic("cursor loads"),
            Some(42)
        );
        testdb.cleanup().await.or_panic("test db cleans up");
    }
}
