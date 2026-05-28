use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use lilo_common::sql::WhereClause;
use lilo_db::LiloDb;
use lilo_rm_core::{
    Lifecycle, LifecycleCounts, LifecycleState, MigrationState, RecentLostEvent, StatusFilter,
};
use sqlx::{
    Executor, QueryBuilder, Sqlite, SqliteConnection, SqlitePool, query::Query,
    sqlite::SqliteArguments,
};
use uuid::Uuid;

use crate::schema;

mod codec;

use codec::{
    EncodedLifecycle, LifecycleRow, RecentLostRow, STATE_LOST, STATE_RUNNING, StateCountRow,
    count_lifecycle_state, encode_tmux_pane, parse_time,
};

macro_rules! lifecycle_row_columns {
    () => {
        "session_id, runtime, isolation, state, shim_pid, runtime_pid, start_time, tmux_pane, \
         exit_code, exit_signal, lost_evidence"
    };
}

const LIFECYCLE_ROW_COLUMNS: &str = lifecycle_row_columns!();
const INSERT_FORKING_SQL: &str = concat!(
    "INSERT INTO runtime_lifecycle (",
    lifecycle_row_columns!(),
    ", spawned_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
);
const UPDATE_LIFECYCLE_SQL: &str = "\
UPDATE runtime_lifecycle
SET runtime = ?,
    isolation = ?,
    state = ?,
    shim_pid = ?,
    runtime_pid = ?,
    start_time = ?,
    tmux_pane = ?,
    exit_code = ?,
    exit_signal = ?,
    lost_evidence = ?,
    updated_at = ?
WHERE session_id = ?";
const LAST_PROBE_SWEEP_KEY: &str = "last_probe_sweep_at";

#[derive(Clone)]
pub struct LifecycleStore {
    pool: SqlitePool,
}

impl LifecycleStore {
    pub fn open(db: &LiloDb) -> Self {
        let pool = db.runtime_pool().clone();
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub async fn insert_forking(&self, lifecycle: &Lifecycle) -> Result<()> {
        if lifecycle.state != LifecycleState::Forking {
            bail!("insert_forking requires Forking lifecycle state");
        }
        insert_forking_with(&self.pool, lifecycle).await
    }

    pub async fn insert_forking_in(
        &self,
        conn: &mut SqliteConnection,
        lifecycle: &Lifecycle,
    ) -> Result<()> {
        if lifecycle.state != LifecycleState::Forking {
            bail!("insert_forking requires Forking lifecycle state");
        }
        insert_forking_with(conn, lifecycle).await
    }

    pub async fn update_lifecycle(&self, lifecycle: &Lifecycle) -> Result<()> {
        update_lifecycle_with(&self.pool, lifecycle).await
    }

    pub async fn update_lifecycle_in(
        &self,
        conn: &mut SqliteConnection,
        lifecycle: &Lifecycle,
    ) -> Result<()> {
        update_lifecycle_with(conn, lifecycle).await
    }

    pub async fn delete(&self, session_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM runtime_lifecycle WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .with_context(|| format!("failed to delete lifecycle {session_id}"))?;
        Ok(())
    }

    pub async fn delete_in(&self, conn: &mut SqliteConnection, session_id: Uuid) -> Result<()> {
        sqlx::query("DELETE FROM runtime_lifecycle WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(conn)
            .await
            .with_context(|| format!("failed to delete lifecycle {session_id}"))?;
        Ok(())
    }

    pub async fn get(&self, session_id: Uuid) -> Result<Option<Lifecycle>> {
        let mut query = lifecycle_rows_query();
        query
            .push(" WHERE session_id = ")
            .push_bind(session_id.to_string());
        let row = query
            .build_query_as::<LifecycleRow>()
            .fetch_optional(&self.pool)
            .await
            .with_context(|| format!("failed to fetch lifecycle {session_id}"))?;
        row.map(TryInto::try_into).transpose()
    }

    pub async fn list(&self, filter: &StatusFilter) -> Result<Vec<Lifecycle>> {
        let session_ids = filter.requested_session_ids();
        let mut query = lifecycle_rows_query();
        let mut where_clause = WhereClause::new();
        if !session_ids.is_empty() {
            query.push(where_clause.predicate_prefix());
            query.push("session_id IN (");
            {
                let mut separated = query.separated(", ");
                for session_id in session_ids {
                    separated.push_bind(session_id.to_string());
                }
            }
            query.push(")");
        }
        if let Some(runtime) = &filter.runtime {
            query.push(where_clause.predicate_prefix());
            query.push("runtime = ");
            query.push_bind(runtime);
        }
        if let Some(state) = &filter.state {
            query.push(where_clause.predicate_prefix());
            query.push("LOWER(state) = LOWER(");
            query.push_bind(state);
            query.push(")");
        }
        if let Some(updated_since) = &filter.updated_since {
            query.push(where_clause.predicate_prefix());
            query.push("updated_at >= ");
            query.push_bind(updated_since.to_rfc3339());
        }
        query.push(" ORDER BY session_id");

        let rows = query
            .build_query_as::<LifecycleRow>()
            .fetch_all(&self.pool)
            .await
            .context("failed to list lifecycles")?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn running(&self) -> Result<Vec<Lifecycle>> {
        let mut query = lifecycle_rows_query();
        query.push(" WHERE state = ").push_bind(STATE_RUNNING);
        query.push(" ORDER BY spawned_at");
        let rows = query
            .build_query_as::<LifecycleRow>()
            .fetch_all(&self.pool)
            .await
            .context("failed to list running lifecycles")?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn running_tmux_occupant(
        &self,
        tmux_pane: &lilo_rm_core::TmuxAddress,
    ) -> Result<Option<Lifecycle>> {
        let mut query = lifecycle_rows_query();
        query.push(" WHERE state = ").push_bind(STATE_RUNNING);
        query
            .push(" AND tmux_pane = ")
            .push_bind(encode_tmux_pane(Some(tmux_pane))?);
        query.push(" ORDER BY spawned_at LIMIT 1");
        let row = query
            .build_query_as::<LifecycleRow>()
            .fetch_optional(&self.pool)
            .await
            .context("failed to fetch running tmux pane occupant")?;
        row.map(TryInto::try_into).transpose()
    }

    pub async fn lifecycle_counts(&self) -> Result<LifecycleCounts> {
        let rows = sqlx::query_as::<_, StateCountRow>(
            r"
            SELECT state, COUNT(*) AS count
            FROM runtime_lifecycle
            GROUP BY state
            ",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to count lifecycle states")?;

        let mut counts = LifecycleCounts::default();
        for row in rows {
            let count = u64::try_from(row.count).context("lifecycle count out of range")?;
            count_lifecycle_state(&mut counts, &row.state, count)?;
        }
        Ok(counts)
    }

    pub async fn recent_lost_since(&self, since: DateTime<Utc>) -> Result<Vec<RecentLostEvent>> {
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT session_id, lost_evidence, updated_at FROM runtime_lifecycle WHERE state = ",
        );
        query.push_bind(STATE_LOST);
        query
            .push(" AND updated_at >= ")
            .push_bind(since.to_rfc3339());
        query.push(" ORDER BY updated_at DESC, session_id");
        let rows = query
            .build_query_as::<RecentLostRow>()
            .fetch_all(&self.pool)
            .await
            .context("failed to list recent lost lifecycles")?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    pub async fn record_probe_sweep(&self, swept_at: DateTime<Utc>) -> Result<()> {
        let value = swept_at.to_rfc3339();
        sqlx::query(
            r"
            INSERT INTO runtime_metadata (key, value, updated_at)
            VALUES (?, ?, ?)
            ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                updated_at = excluded.updated_at
            ",
        )
        .bind(LAST_PROBE_SWEEP_KEY)
        .bind(value.clone())
        .bind(value)
        .execute(&self.pool)
        .await
        .context("failed to record last probe sweep")?;
        Ok(())
    }

    pub async fn last_probe_sweep(&self) -> Result<Option<DateTime<Utc>>> {
        let value = sqlx::query_scalar::<_, String>(
            r"
            SELECT value
            FROM runtime_metadata
            WHERE key = ?
            ",
        )
        .bind(LAST_PROBE_SWEEP_KEY)
        .fetch_optional(&self.pool)
        .await
        .context("failed to read last probe sweep")?;
        value.map(|time| parse_time(&time)).transpose()
    }

    pub async fn migration_state(&self) -> Result<MigrationState> {
        let known = schema::known_migrations();
        let applied_versions = sqlx::query_scalar::<_, i64>(
            r"
            SELECT version
            FROM _sqlx_migrations
            WHERE success = 1
            ORDER BY version
            ",
        )
        .fetch_all(&self.pool)
        .await
        .context("failed to read applied migrations")?;

        let mut applied_descriptions = Vec::new();
        let mut pending_descriptions = Vec::new();
        for migration in &known {
            if applied_versions.contains(&migration.version) {
                applied_descriptions.push(migration.description.clone());
            } else {
                pending_descriptions.push(migration.description.clone());
            }
        }
        Ok(MigrationState {
            applied: applied_descriptions.len(),
            total: known.len(),
            applied_descriptions,
            pending_descriptions,
        })
    }

    pub async fn reset(&self) -> Result<()> {
        self.pool
            .execute("DELETE FROM runtime_lifecycle")
            .await
            .context("failed to reset lifecycle table")?;
        Ok(())
    }

    #[must_use]
    pub fn from_pool(pool: SqlitePool) -> Self {
        Self { pool }
    }

    #[cfg(test)]
    pub async fn path_open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        let db = LiloDb::open_path(path).await?;
        Ok(Self::open(&db))
    }
}

fn lifecycle_rows_query<'q>() -> QueryBuilder<'q, Sqlite> {
    let mut query = QueryBuilder::<Sqlite>::new("SELECT ");
    query.push(LIFECYCLE_ROW_COLUMNS);
    query.push(" FROM runtime_lifecycle");
    query
}

async fn insert_forking_with<'e, E>(executor: E, lifecycle: &Lifecycle) -> Result<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let encoded = EncodedLifecycle::from_lifecycle(lifecycle)?;
    bind_lifecycle_snapshot(
        sqlx::query(INSERT_FORKING_SQL).bind(encoded.session_id.clone()),
        &encoded,
    )
    .bind(encoded.now.clone())
    .bind(encoded.now)
    .execute(executor)
    .await
    .with_context(|| format!("failed to insert lifecycle {}", lifecycle.session_id))?;
    Ok(())
}

async fn update_lifecycle_with<'e, E>(executor: E, lifecycle: &Lifecycle) -> Result<()>
where
    E: Executor<'e, Database = Sqlite>,
{
    let encoded = EncodedLifecycle::from_lifecycle(lifecycle)?;
    let result = bind_lifecycle_snapshot(sqlx::query(UPDATE_LIFECYCLE_SQL), &encoded)
        .bind(encoded.now)
        .bind(encoded.session_id)
        .execute(executor)
        .await
        .with_context(|| format!("failed to update lifecycle {}", lifecycle.session_id))?;
    if result.rows_affected() == 0 {
        bail!("session {} not found", lifecycle.session_id);
    }
    Ok(())
}

fn bind_lifecycle_snapshot<'q>(
    query: Query<'q, Sqlite, SqliteArguments<'q>>,
    encoded: &EncodedLifecycle,
) -> Query<'q, Sqlite, SqliteArguments<'q>> {
    query
        .bind(encoded.runtime.clone())
        .bind(encoded.isolation.clone())
        .bind(encoded.state)
        .bind(encoded.shim_pid)
        .bind(encoded.runtime_pid)
        .bind(encoded.start_time.clone())
        .bind(encoded.tmux_pane.clone())
        .bind(encoded.exit_code)
        .bind(encoded.exit_signal)
        .bind(encoded.lost_evidence)
}

#[cfg(test)]
mod tests;
