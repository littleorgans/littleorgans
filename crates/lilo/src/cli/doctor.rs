use clap::Args;
use lilo_common::diagnostic::Diagnostic;
use lilo_db::LiloDb;
use lilo_paths::{LiloHome, LiloPaths};
use serde::Serialize;
use tokio::net::UnixStream;

use super::Output;

#[derive(Debug, Args)]
pub struct DoctorCommand {}

impl DoctorCommand {
    pub async fn run(&self, output: Output) -> Result<(), Diagnostic> {
        let status = DoctorStatus::collect().await?;

        match output {
            Output::Human => println!("{}", status.render_human()),
            Output::Json => println!("{}", status.render_json()?),
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct DoctorStatus {
    daemon: DaemonHealth,
    database: DatabaseHealth,
    substrates: SubstrateHealth,
    warnings: Vec<String>,
}

impl DoctorStatus {
    async fn collect() -> Result<Self, Diagnostic> {
        let paths = paths()?;
        let daemon = DaemonHealth::collect(&paths).await;
        let database = DatabaseHealth::collect(&paths).await;
        let substrates = match database.db.as_ref() {
            Some(db) => SubstrateHealth::collect(db).await,
            None => SubstrateHealth::default(),
        };

        Ok(Self {
            daemon,
            database: database.health,
            substrates,
            warnings: Vec::new(),
        })
    }

    fn render_human(&self) -> String {
        let daemon_status = if self.daemon.reachable {
            "reachable"
        } else {
            "unreachable"
        };
        format!(
            "lilo doctor\n\
             daemon: {daemon_status} ({})\n\
             db: {} ({})\n\
             pragmas: journal_mode={} busy_timeout={} synchronous={}\n\
             substrates: sessions_active={} runtimes_active={} audit_rows={}\n\
             warnings: {}",
            self.daemon.socket_path,
            self.database.status,
            self.database.path,
            self.database.pragmas.journal_mode,
            self.database.pragmas.busy_timeout,
            self.database.pragmas.synchronous,
            self.substrates.sessions.active,
            self.substrates.runtimes.active,
            self.substrates.identity.audit_rows,
            if self.warnings.is_empty() {
                "none"
            } else {
                "present"
            }
        )
    }

    fn render_json(&self) -> Result<String, Diagnostic> {
        serde_json::to_string(self).map_err(|error| {
            Diagnostic::internal("failed to serialize doctor status").with_detail(error.to_string())
        })
    }
}

#[derive(Debug, Serialize)]
struct DaemonHealth {
    socket_path: String,
    socket_exists: bool,
    reachable: bool,
}

impl DaemonHealth {
    async fn collect(paths: &LiloPaths) -> Self {
        let socket_path = paths.socket_path();
        Self {
            socket_exists: socket_path.exists(),
            reachable: UnixStream::connect(&socket_path).await.is_ok(),
            socket_path: socket_path.display().to_string(),
        }
    }
}

#[derive(Debug, Serialize)]
struct DatabaseHealth {
    path: String,
    status: &'static str,
    pragmas: DbPragmas,
    error: Option<String>,
}

struct DatabaseProbe {
    health: DatabaseHealth,
    db: Option<LiloDb>,
}

impl DatabaseHealth {
    async fn collect(paths: &LiloPaths) -> DatabaseProbe {
        let path = paths.db_path();
        let path_label = path.display().to_string();
        match LiloDb::open(paths).await {
            Ok(db) => match DbPragmas::collect(&db).await {
                Ok(pragmas) => DatabaseProbe {
                    health: Self {
                        path: path_label,
                        status: "ok",
                        pragmas,
                        error: None,
                    },
                    db: Some(db),
                },
                Err(error) => DatabaseProbe {
                    health: Self {
                        path: path_label,
                        status: "error",
                        pragmas: DbPragmas::default(),
                        error: Some(error),
                    },
                    db: Some(db),
                },
            },
            Err(error) => DatabaseProbe {
                health: Self {
                    path: path_label,
                    status: "error",
                    pragmas: DbPragmas::default(),
                    error: Some(error.to_string()),
                },
                db: None,
            },
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct DbPragmas {
    journal_mode: String,
    busy_timeout: i64,
    synchronous: i64,
}

impl DbPragmas {
    async fn collect(db: &LiloDb) -> Result<Self, String> {
        let pool = db.session_pool();
        let journal_mode = sqlx::query_scalar::<_, String>("PRAGMA journal_mode")
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;
        let busy_timeout = sqlx::query_scalar::<_, i64>("PRAGMA busy_timeout")
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;
        let synchronous = sqlx::query_scalar::<_, i64>("PRAGMA synchronous")
            .fetch_one(pool)
            .await
            .map_err(|error| error.to_string())?;

        Ok(Self {
            journal_mode,
            busy_timeout,
            synchronous,
        })
    }
}

#[derive(Debug, Default, Serialize)]
struct SubstrateHealth {
    identity: IdentityHealth,
    sessions: SessionHealth,
    runtimes: RuntimeHealth,
}

impl SubstrateHealth {
    async fn collect(db: &LiloDb) -> Self {
        Self {
            identity: IdentityHealth {
                audit_rows: count(db.identity_pool(), "SELECT COUNT(*) FROM identity_audit").await,
            },
            sessions: SessionHealth {
                active: count(
                    db.session_pool(),
                    "SELECT COUNT(*) FROM session_sessions WHERE state IN ('SPAWNING', 'RUNNING')",
                )
                .await,
            },
            runtimes: RuntimeHealth {
                active: count(
                    db.runtime_pool(),
                    "SELECT COUNT(*) FROM runtime_lifecycle WHERE state IN ('Forking', 'Running')",
                )
                .await,
            },
        }
    }
}

#[derive(Debug, Default, Serialize)]
struct IdentityHealth {
    audit_rows: i64,
}

#[derive(Debug, Default, Serialize)]
struct SessionHealth {
    active: i64,
}

#[derive(Debug, Default, Serialize)]
struct RuntimeHealth {
    active: i64,
}

async fn count(pool: &sqlx::SqlitePool, query: &'static str) -> i64 {
    sqlx::query_scalar::<_, i64>(query)
        .fetch_one(pool)
        .await
        .unwrap_or_default()
}

fn paths() -> Result<LiloPaths, Diagnostic> {
    let home = LiloHome::from_env().map_err(|error| {
        Diagnostic::internal("failed to resolve lilo home").with_detail(error.to_string())
    })?;
    Ok(LiloPaths::new(home))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn collected_status_has_backend_probe_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).expect("home path is valid"),
        );

        let database = DatabaseHealth::collect(&paths).await;
        let db = database.db.expect("db opens");
        let status = DoctorStatus {
            daemon: DaemonHealth::collect(&paths).await,
            substrates: SubstrateHealth::collect(&db).await,
            database: database.health,
            warnings: Vec::new(),
        };

        assert!(!status.daemon.reachable);
        assert_eq!(status.database.status, "ok");
        assert_eq!(status.database.pragmas.journal_mode, "wal");
        assert_eq!(status.database.pragmas.busy_timeout, 5_000);
        assert_eq!(status.database.pragmas.synchronous, 1);
        assert_eq!(status.substrates.sessions.active, 0);
        assert_eq!(status.substrates.runtimes.active, 0);
        assert_eq!(status.substrates.identity.audit_rows, 0);
        assert!(status.warnings.is_empty());
    }
}
