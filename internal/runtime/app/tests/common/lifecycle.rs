use chrono::{DateTime, TimeZone, Utc};
use lilo_common::id::SessionId;
use lilo_rm_core::{Lifecycle, RuntimeKind, ShimReady};
use lilo_runtime_store::LifecycleStore;
use uuid::Uuid;

use super::process::process_alive;

pub fn persist_running(database_url: &str, session_id: SessionId, runtime_pid: u32) {
    persist_running_with_start_time(
        database_url,
        session_id,
        runtime_pid,
        Utc.timestamp_opt(1_000, 0).unwrap(),
    );
}

pub fn persist_running_with_start_time(
    database_url: &str,
    session_id: SessionId,
    runtime_pid: u32,
    start_time: DateTime<Utc>,
) {
    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async {
            let db = lilo_db::LiloDb::open_postgres(lilo_db::DbConfig::from_url(database_url))
                .await
                .expect("store db");
            let store = LifecycleStore::from_db(&db);
            let mut lifecycle = Lifecycle::forking(session_id, RuntimeKind::Claude);
            store.insert_forking(&lifecycle).await.expect("insert");
            lifecycle.mark_running(ShimReady {
                session_id,
                shim_pid: runtime_pid + 1,
                runtime_pid,
                start_time,
                tmux_pane: None,
            });
            store.update_lifecycle(&lifecycle).await.expect("running");
        });
}

pub fn unused_pid() -> u32 {
    (60_000..61_000)
        .find(|pid| !process_alive(*pid))
        .expect("unused pid")
}
