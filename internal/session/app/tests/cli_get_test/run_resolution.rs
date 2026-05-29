use crate::common::{self, OrPanic as _};
use crate::{assert_success, canonical_display, first_field};
use serde_json::Value;

#[test]
pub(crate) fn run_persists_canonical_dir_from_cli_resolution() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let project = daemon.dir.path().join("project");
    std::fs::create_dir_all(&project).or_panic("project dir");

    let run = daemon
        .command()
        .current_dir(&project)
        .args(["run", "claude", "--role", "engineer", "--dir", "."])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run --dir", &run);
    let id = first_field(&run.stdout);

    let single = daemon
        .command()
        .args(["get", "session", &id, "--json"])
        .output()
        .or_panic("sm get session <id> --json executes");
    assert_success("sm get session <id> --json", &single);
    let session: Value = serde_json::from_slice(&single.stdout).or_panic("session JSON parses");
    let canonical_project = canonical_display(&project);
    assert_eq!(session["dir"], canonical_project);
    assert_eq!(session["workspace"], canonical_project);
    assert_eq!(session["namespace"], "default");
}

#[test]
pub(crate) fn run_resolves_spawn_intent_and_persists_session() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());

    let run = daemon
        .command()
        .args(["run", "claude", "--role", "engineer"])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &run);
    let id = first_field(&run.stdout);

    let counts = spawn_intent_counts(&daemon.audit_path(), &id);
    assert_eq!(counts.pending, 0);
    assert_eq!(counts.resolved, 1);
    assert_eq!(counts.sessions, 1);
}

#[test]
pub(crate) fn workspace_arg_is_rejected_by_clap() {
    let daemon = common::DaemonFixture::start();

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &daemon.dir.path().display().to_string(),
            "--workspace",
            &daemon.dir.path().display().to_string(),
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("unexpected argument '--workspace'"));
    assert!(!stderr.contains("--workspace is deprecated"));
}

struct SpawnIntentCounts {
    pending: i64,
    resolved: i64,
    sessions: i64,
}

fn spawn_intent_counts(path: &std::path::Path, id: &str) -> SpawnIntentCounts {
    let runtime = tokio::runtime::Runtime::new().or_panic("tokio runtime");
    runtime.block_on(async move {
        let db = lilo_db::LiloDb::open_path(path).await.or_panic("db opens");
        SpawnIntentCounts {
            pending: count_rows(
                db.session_pool(),
                "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'pending'",
                id,
            )
            .await,
            resolved: count_rows(
                db.session_pool(),
                "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'resolved'",
                id,
            )
            .await,
            sessions: count_rows(
                db.session_pool(),
                "SELECT COUNT(*) FROM session_sessions WHERE id = ?",
                id,
            )
            .await,
        }
    })
}

async fn count_rows(pool: &sqlx::SqlitePool, sql: &str, id: &str) -> i64 {
    sqlx::query_scalar(sql)
        .bind(id)
        .fetch_one(pool)
        .await
        .or_panic("count rows")
}

#[test]
pub(crate) fn unknown_namespace_error_is_surfaced_from_daemon() {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());

    let run = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &daemon.dir.path().display().to_string(),
            "--namespace",
            "missing",
        ])
        .output()
        .or_panic("sm run executes");

    assert!(!run.status.success());
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(stderr.contains("namespace not found: missing"));
}
