use clap::Args;
use lilo_common::diagnostic::Diagnostic;
use lilo_db::LiloDb;
use lilo_paths::{DaemonEndpoint, LiloPaths};
use lilo_session_core::{DoctorRequest, RpcResponse, SessionRpc};
use serde::Serialize;

use super::{Output, resolve_lilo_paths};

const UNKNOWN_DAEMON_VERSION: &str = "unknown/pre-field";

fn daemon_version_skew_warning(daemon_version: &str) -> String {
    format!(
        "client lilo {} but daemon lilod {} — restart the daemon (lilo daemon stop && lilo daemon start)",
        crate::VERSION,
        daemon_version
    )
}

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
        let warnings = Self::warnings_for_daemon(&daemon);

        Ok(Self {
            daemon,
            database: database.health,
            substrates,
            warnings,
        })
    }

    fn render_human(&self) -> String {
        let daemon_status = if self.daemon.reachable {
            "reachable"
        } else {
            "unreachable"
        };
        let warnings = self.render_warnings();
        format!(
            "lilo doctor\n\
             daemon: {daemon_status} ({})\n\
             db: {} ({})\n\
             pragmas: journal_mode={} busy_timeout={} synchronous={}\n\
             substrates: sessions_active={} runtimes_active={} audit_rows={}\n\
             warnings: {}{}",
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
            },
            warnings
        )
    }

    fn render_json(&self) -> Result<String, Diagnostic> {
        serde_json::to_string(self).map_err(|error| {
            Diagnostic::internal("failed to serialize doctor status").with_detail(error.to_string())
        })
    }

    fn warnings_for_daemon(daemon: &DaemonHealth) -> Vec<String> {
        daemon
            .version_warning()
            .into_iter()
            .collect::<Vec<String>>()
    }

    fn render_warnings(&self) -> String {
        let mut rendered = String::new();
        for warning in &self.warnings {
            rendered.push_str("\nwarn: ");
            rendered.push_str(warning);
        }
        rendered
    }
}

#[derive(Debug, Serialize)]
struct DaemonHealth {
    socket_path: String,
    socket_exists: bool,
    reachable: bool,
    version: Option<String>,
}

impl DaemonHealth {
    async fn collect(paths: &LiloPaths) -> Self {
        let socket_path = paths.socket_path();
        let endpoint = DaemonEndpoint::from_paths(paths);
        let response = lilo_session_daemon::send_request(
            &endpoint,
            &SessionRpc::Doctor {
                request: DoctorRequest::default(),
            },
        )
        .await;
        let (reachable, version) = match response {
            Ok(RpcResponse::Doctor { response }) => (true, response.daemon_version),
            _ => (false, None),
        };
        Self {
            socket_exists: socket_path.exists(),
            reachable,
            version,
            socket_path: socket_path.display().to_string(),
        }
    }

    fn version_warning(&self) -> Option<String> {
        if !self.reachable || self.version.as_deref() == Some(crate::VERSION) {
            return None;
        }
        Some(daemon_version_skew_warning(
            self.version.as_deref().unwrap_or(UNKNOWN_DAEMON_VERSION),
        ))
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
                Ok(pragmas) => Self::probe(path_label, "ok", pragmas, None, Some(db)),
                Err(error) => Self::error_probe(path_label, error, Some(db)),
            },
            Err(error) => Self::error_probe(path_label, error.to_string(), None),
        }
    }

    fn probe(
        path: String,
        status: &'static str,
        pragmas: DbPragmas,
        error: Option<String>,
        db: Option<LiloDb>,
    ) -> DatabaseProbe {
        DatabaseProbe {
            health: Self {
                path,
                status,
                pragmas,
                error,
            },
            db,
        }
    }

    fn error_probe(path: String, error: String, db: Option<LiloDb>) -> DatabaseProbe {
        Self::probe(path, "error", DbPragmas::default(), Some(error), db)
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
    resolve_lilo_paths().map_err(|error| {
        Diagnostic::internal("failed to resolve lilo home").with_detail(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilo_paths::LiloHome;

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
        assert_eq!(status.daemon.version, None);
        assert_eq!(status.database.status, "ok");
        assert_eq!(status.database.pragmas.journal_mode, "wal");
        assert_eq!(status.database.pragmas.busy_timeout, 5_000);
        assert_eq!(status.database.pragmas.synchronous, 1);
        assert_eq!(status.substrates.sessions.active, 0);
        assert_eq!(status.substrates.runtimes.active, 0);
        assert_eq!(status.substrates.identity.audit_rows, 0);
        assert!(status.warnings.is_empty());
    }

    #[test]
    fn daemon_version_warning_rules_cover_equal_mismatch_and_missing() {
        let equal = daemon_health(true, Some(crate::VERSION));
        assert_eq!(
            DoctorStatus::warnings_for_daemon(&equal),
            Vec::<String>::new()
        );

        let mismatch = daemon_health(true, Some("0.0.0+old"));
        let mismatch_warnings = DoctorStatus::warnings_for_daemon(&mismatch);
        assert_eq!(
            mismatch_warnings,
            vec![daemon_version_skew_warning("0.0.0+old")]
        );

        let missing = daemon_health(true, None);
        let missing_warnings = DoctorStatus::warnings_for_daemon(&missing);
        assert_eq!(
            missing_warnings,
            vec![daemon_version_skew_warning(UNKNOWN_DAEMON_VERSION)]
        );

        let down = daemon_health(false, None);
        assert_eq!(
            DoctorStatus::warnings_for_daemon(&down),
            Vec::<String>::new()
        );
    }

    #[test]
    fn render_human_prints_skew_warning_while_reachable() {
        let daemon = daemon_health(true, Some("0.0.0+old"));
        let status = DoctorStatus {
            warnings: DoctorStatus::warnings_for_daemon(&daemon),
            daemon,
            database: DatabaseHealth {
                path: "/tmp/lilo.db".to_string(),
                status: "ok",
                pragmas: DbPragmas::default(),
                error: None,
            },
            substrates: SubstrateHealth::default(),
        };

        let rendered = status.render_human();

        assert!(rendered.contains("daemon: reachable"));
        assert!(rendered.contains("warnings: present"));
        assert!(rendered.contains(&format!(
            "warn: {}",
            daemon_version_skew_warning("0.0.0+old")
        )));
    }

    fn daemon_health(reachable: bool, version: Option<&str>) -> DaemonHealth {
        DaemonHealth {
            socket_path: "/tmp/lilod.sock".to_string(),
            socket_exists: reachable,
            reachable,
            version: version.map(ToOwned::to_owned),
        }
    }
}
