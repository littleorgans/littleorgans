use std::time::Duration;

use clap::Args;
use lilo_common::diagnostic::Diagnostic;
use lilo_db::LiloDb;
use lilo_paths::{DaemonEndpoint, LiloPaths};
use lilo_session_core::{DoctorRequest, RpcResponse, RuntimeDoctorReport, SessionRpc};
use serde::Serialize;

use super::{Output, resolve_lilo_paths};

mod runtime;

const DOCTOR_RPC_TIMEOUT: Duration = Duration::from_secs(3);
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
    runtime: Option<RuntimeDoctorReport>,
    warnings: Vec<String>,
}

impl DoctorStatus {
    async fn collect() -> Result<Self, Diagnostic> {
        let paths = paths()?;
        let DaemonProbe {
            health: daemon,
            runtime,
        } = DaemonHealth::collect(&paths).await;
        let database = DatabaseHealth::collect().await;
        let substrates = match database.db.as_ref() {
            Some(db) => SubstrateHealth::collect(db).await,
            None => SubstrateHealth::default(),
        };
        let mut warnings = Self::warnings_for_daemon(&daemon);
        if let Some(runtime) = &runtime {
            warnings.extend(Self::warnings_for_runtime(runtime));
        }

        Ok(Self {
            daemon,
            database: database.health,
            substrates,
            runtime,
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
             db: {}\n\
             substrates: sessions_active={} runtimes_active={} audit_rows={}\n\
             warnings: {}{}",
            self.daemon.socket_path,
            self.database.status,
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

    fn warnings_for_runtime(report: &RuntimeDoctorReport) -> Vec<String> {
        runtime::warnings_for_report(report)
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

struct DaemonProbe {
    health: DaemonHealth,
    runtime: Option<RuntimeDoctorReport>,
}

impl DaemonHealth {
    async fn collect(paths: &LiloPaths) -> DaemonProbe {
        Self::collect_with_timeout(paths, DOCTOR_RPC_TIMEOUT).await
    }

    async fn collect_with_timeout(paths: &LiloPaths, timeout: Duration) -> DaemonProbe {
        let socket_path = paths.socket_path();
        let endpoint = DaemonEndpoint::from_paths(paths);
        let response = lilo_session_daemon::send_request_with_timeout(
            &endpoint,
            &SessionRpc::Doctor {
                request: DoctorRequest::default(),
            },
            timeout,
        )
        .await;
        let (reachable, version, runtime) = match response {
            Ok(RpcResponse::Doctor { response }) => (
                true,
                response.daemon_version,
                Some(response.runtime_matters),
            ),
            _ => (false, None, None),
        };
        DaemonProbe {
            health: Self {
                socket_exists: socket_path.exists(),
                reachable,
                version,
                socket_path: socket_path.display().to_string(),
            },
            runtime,
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
    status: &'static str,
    error: Option<String>,
}

struct DatabaseProbe {
    health: DatabaseHealth,
    db: Option<LiloDb>,
}

impl DatabaseHealth {
    async fn collect() -> DatabaseProbe {
        match LiloDb::open_postgres_resolved().await {
            Ok(db) => match Self::ping(&db).await {
                Ok(()) => Self::probe("ok", None, Some(db)),
                Err(error) => Self::error_probe(error, Some(db)),
            },
            Err(error) => Self::error_probe(error.to_string(), None),
        }
    }

    /// Validate the pool is live with a trivial round-trip.
    async fn ping(db: &LiloDb) -> Result<(), String> {
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(db.pool())
            .await
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn probe(status: &'static str, error: Option<String>, db: Option<LiloDb>) -> DatabaseProbe {
        DatabaseProbe {
            health: Self { status, error },
            db,
        }
    }

    fn error_probe(error: String, db: Option<LiloDb>) -> DatabaseProbe {
        Self::probe("error", Some(error), db)
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
                audit_rows: count(db.pool(), "SELECT COUNT(*) FROM identity_audit").await,
            },
            sessions: SessionHealth {
                active: count(
                    db.pool(),
                    "SELECT COUNT(*) FROM session_sessions WHERE state IN ('SPAWNING', 'RUNNING')",
                )
                .await,
            },
            runtimes: RuntimeHealth {
                active: count(
                    db.pool(),
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

async fn count(pool: &sqlx::PgPool, query: &'static str) -> i64 {
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
    use tokio::net::UnixListener;
    use tokio::time::Instant;

    #[tokio::test]
    #[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
    async fn collected_status_has_backend_probe_shape() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).expect("home path is valid"),
        );

        // DatabaseHealth::collect resolves its pool from LILO_DATABASE_URL.
        // Point it at a throwaway database for the duration of this test.
        // SAFETY: nextest runs each test in its own process; no concurrent env
        // readers in this single-threaded test.
        let testdb = lilo_db::test_support::TestDb::create()
            .await
            .expect("test database");
        unsafe {
            std::env::set_var("LILO_DATABASE_URL", testdb.database_url());
        }

        let database = DatabaseHealth::collect().await;
        let db = database.db.expect("db opens");
        let status = DoctorStatus {
            daemon: DaemonHealth::collect(&paths).await.health,
            substrates: SubstrateHealth::collect(&db).await,
            database: database.health,
            runtime: None,
            warnings: Vec::new(),
        };

        assert!(!status.daemon.reachable);
        assert_eq!(status.daemon.version, None);
        assert_eq!(status.database.status, "ok");
        assert_eq!(status.substrates.sessions.active, 0);
        assert_eq!(status.substrates.runtimes.active, 0);
        assert_eq!(status.substrates.identity.audit_rows, 0);
        assert!(status.warnings.is_empty());

        testdb.cleanup().await.expect("test db cleans up");
    }

    #[tokio::test]
    async fn doctor_rpc_timeout_marks_hung_socket_unreachable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = LiloPaths::new(
            LiloHome::from_path(dir.path().join("lilo")).expect("home path is valid"),
        );
        std::fs::create_dir_all(paths.run_root()).expect("run dir exists");
        let listener = UnixListener::bind(paths.socket_path()).expect("bind hung daemon socket");
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept client");
            tokio::time::sleep(Duration::from_mins(1)).await;
        });

        let started = Instant::now();
        let health = DaemonHealth::collect_with_timeout(&paths, Duration::from_millis(50))
            .await
            .health;
        let elapsed = started.elapsed();
        server.abort();

        assert!(elapsed < Duration::from_secs(1), "elapsed: {elapsed:?}");
        assert!(health.socket_exists);
        assert!(!health.reachable);
        assert_eq!(health.version, None);
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
                status: "ok",
                error: None,
            },
            substrates: SubstrateHealth::default(),
            runtime: None,
        };

        let rendered = status.render_human();

        assert!(rendered.contains("daemon: reachable"));
        assert!(rendered.contains("warnings: present"));
        assert!(rendered.contains(&format!(
            "warn: {}",
            daemon_version_skew_warning("0.0.0+old")
        )));
    }

    #[test]
    fn render_json_includes_runtime_detail_section() {
        let report = RuntimeDoctorReport {
            status: "ok".to_string(),
            doctor: Some(Box::new(runtime::response())),
            socket_path: Some("/tmp/rtmd.sock".to_string()),
            code: None,
            message: None,
        };
        let status = DoctorStatus {
            daemon: daemon_health(true, Some(crate::VERSION)),
            database: DatabaseHealth {
                status: "ok",
                error: None,
            },
            substrates: SubstrateHealth::default(),
            runtime: Some(report),
            warnings: Vec::new(),
        };

        let value: serde_json::Value =
            serde_json::from_str(&status.render_json().expect("json")).expect("valid json");

        assert_eq!(value["runtime"]["status"], "ok");
        assert_eq!(value["runtime"]["doctor"]["socket_path"], "/tmp/rtmd.sock");
        assert!(value["runtime"]["doctor"]["launchers"].is_array());
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
