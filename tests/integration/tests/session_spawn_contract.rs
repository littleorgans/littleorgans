use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use lilo_db::{begin_immediate_tx, finish_immediate_tx};
use lilo_integration_tests::{
    IntegrationFixture, count_rows, draft_session, event_log_line_count, fixed_uuid, running_event,
    running_lifecycle, runtime_config, runtime_request,
};
use lilo_runtime_daemon::{RuntimeService, RuntimeServiceContext};
use lilo_runtime_store::LifecycleStore;
use lilo_session_store::{PendingSpawnIntent, SessionDraft, SqliteStore};
use uuid::Uuid;

const DAEMON_TIMEOUT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn lilo_session_user_verbs_route_through_session_spawn() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let mut daemon = LiloDaemon::start(&fixture)?;
    let workspace = fixture.paths.tmp_root().join("workspace");
    fs::create_dir_all(&workspace)?;

    let run = daemon
        .command(["run", "claude", "--role", "worker", "--dir"])
        .arg(&workspace)
        .args(["--target", "headless"])
        .output()
        .context("lilo run executes")?;
    assert_success("lilo run", &run);
    let run_id = stdout_session_id(&run)?;

    let create = daemon
        .command(["create", "session", "claude", "--role", "worker", "--dir"])
        .arg(&workspace)
        .output()
        .context("lilo create session executes")?;
    assert_success("lilo create session", &create);
    let created_id = stdout_session_id(&create)?;

    for session_id in [run_id, created_id] {
        assert_eq!(session_count(&fixture, session_id).await?, 1);
        assert_eq!(resolved_count(&fixture, session_id).await?, 1);
        assert_eq!(pending_count(&fixture, session_id).await?, 0);
        assert!(allowed_spawn_audit_count(&fixture, session_id).await? >= 1);
    }

    let raw_runtime_id = fixed_uuid(30);
    LifecycleStore::open(&fixture.db)
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            raw_runtime_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;

    let get = daemon
        .command(["get", "session", "--output", "json"])
        .output()
        .context("lilo get session executes")?;
    assert_success("lilo get session --output json", &get);
    let listed_ids = listed_session_ids(&get)?;
    assert_eq!(listed_ids.len(), 2, "stdout: {}", stdout(&get));
    assert!(listed_ids.contains(&run_id));
    assert!(listed_ids.contains(&created_id));
    assert!(!listed_ids.contains(&raw_runtime_id));

    daemon.stop();
    Ok(())
}

#[tokio::test]
async fn doctor_reachability_probe_does_not_warn_on_bare_connect() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let mut daemon = LiloDaemon::start(&fixture)?;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let stderr_before = daemon.stderr();
    let doctor = daemon
        .command(["doctor"])
        .output()
        .context("lilo doctor executes")?;
    assert_success("lilo doctor", &doctor);
    let doctor_stdout = stdout(&doctor);
    assert!(
        doctor_stdout.contains("daemon: reachable"),
        "doctor must keep daemon reachability output\nstdout:\n{doctor_stdout}"
    );
    assert!(
        doctor_stdout.contains("warnings: none"),
        "matching daemon and client builds must not warn\nstdout:\n{doctor_stdout}"
    );

    tokio::time::sleep(Duration::from_millis(50)).await;
    let stderr_after = daemon.stderr();
    let daemon_stderr = stderr_after
        .strip_prefix(&stderr_before)
        .unwrap_or(&stderr_after);
    let _ = daemon.kill_and_stderr();
    assert!(
        !daemon_stderr.contains("Broken pipe"),
        "doctor bare-connect probe must not emit broken-pipe warning\nstderr:\n{daemon_stderr}"
    );
    assert!(
        !daemon_stderr.contains("lilod connection failed"),
        "doctor bare-connect probe must not emit connection warning\nstderr:\n{daemon_stderr}"
    );
    Ok(())
}

#[tokio::test]
async fn session_spawn_persists_fixed_order_across_two_transactions() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let session_store = SqliteStore::open(&fixture.db);
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(1);
    let mut observed = Vec::new();
    let intent = PendingSpawnIntent::new(
        fixed_uuid(2),
        runtime_request(session_id),
        SessionDraft::new(&draft_session(session_id)),
    );

    let mut tx_a = fixture.db.session_pool().acquire().await?;
    begin_immediate_tx(&mut tx_a, "integration tx a").await?;
    let tx_a_result = async {
        insert_audit(&mut *tx_a, "audit-session-spawn", session_id).await?;
        observed.push("identity-audit");
        session_store
            .insert_pending_spawn_intent_in(&mut tx_a, &intent)
            .await?;
        observed.push("intent-pending");
        lifecycle_store
            .insert_forking_in(
                &mut tx_a,
                &lilo_rm_core::Lifecycle::forking(session_id, lilo_rm_core::RuntimeKind::Claude),
            )
            .await?;
        observed.push("runtime-forking");
        Result::<()>::Ok(())
    }
    .await;
    finish_immediate_tx(&mut tx_a, tx_a_result, "integration tx a").await?;

    assert_eq!(pending_count(&fixture, session_id).await?, 1);
    assert_eq!(resolved_count(&fixture, session_id).await?, 0);
    assert_eq!(session_count(&fixture, session_id).await?, 0);

    let running = running_lifecycle(session_id);
    lifecycle_store.update_lifecycle(&running).await?;
    observed.push("runtime-kqueue-ready");

    let mut tx_b = fixture.db.session_pool().acquire().await?;
    begin_immediate_tx(&mut tx_b, "integration tx b").await?;
    let tx_b_result = async {
        let session = intent
            .session_draft
            .running_session(&running, None, chrono::Utc::now())?;
        session_store.insert_session_in(&mut tx_b, &session).await?;
        observed.push("session-record");
        session_store
            .resolve_spawn_intent_in(&mut tx_b, session_id)
            .await?;
        observed.push("intent-resolved");
        Result::<()>::Ok(())
    }
    .await;
    finish_immediate_tx(&mut tx_b, tx_b_result, "integration tx b").await?;

    assert_eq!(
        observed,
        [
            "identity-audit",
            "intent-pending",
            "runtime-forking",
            "runtime-kqueue-ready",
            "session-record",
            "intent-resolved",
        ]
    );
    assert_eq!(pending_count(&fixture, session_id).await?, 0);
    assert_eq!(resolved_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 1);
    Ok(())
}

#[tokio::test]
async fn raw_runtime_spawn_keeps_session_tables_empty() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(10);

    insert_audit(
        fixture.db.identity_pool(),
        "audit-raw-runtime-spawn",
        session_id,
    )
    .await?;
    lifecycle_store
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            session_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;

    assert_eq!(audit_count(&fixture, session_id).await?, 1);
    assert_eq!(lifecycle_count(&fixture, session_id).await?, 1);
    assert_eq!(pending_count(&fixture, session_id).await?, 0);
    assert_eq!(session_count(&fixture, session_id).await?, 0);
    Ok(())
}

#[tokio::test]
async fn startup_reconcile_appends_d9_only_after_tx_b_commit() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let runtime = Arc::new(
        RuntimeService::build(RuntimeServiceContext::new(
            runtime_config(&fixture.paths),
            fixture.db.clone(),
        ))
        .await?,
    );
    let session_store = SqliteStore::open(&fixture.db);
    let lifecycle_store = LifecycleStore::open(&fixture.db);
    let session_id = fixed_uuid(20);
    let intent = PendingSpawnIntent::new(
        fixed_uuid(21),
        runtime_request(session_id),
        SessionDraft::new(&draft_session(session_id)),
    );

    session_store.insert_pending_spawn_intent(&intent).await?;
    lifecycle_store
        .insert_forking(&lilo_rm_core::Lifecycle::forking(
            session_id,
            lilo_rm_core::RuntimeKind::Claude,
        ))
        .await?;
    lifecycle_store
        .update_lifecycle(&running_lifecycle(session_id))
        .await?;
    sqlx::query(
        "CREATE TRIGGER fail_resolve_before_event
         BEFORE UPDATE OF status ON session_spawn_intents
         WHEN NEW.status = 'resolved'
         BEGIN
           SELECT RAISE(ABORT, 'forced tx b failure');
         END",
    )
    .execute(fixture.db.session_pool())
    .await?;

    let service = lilo_session_daemon::SessionService::build(
        lilo_session_daemon::SessionServiceContext::new(
            fixture.paths.clone(),
            "test-daemon",
            fixture.db.clone(),
            Arc::clone(&runtime),
        ),
    )?;
    service.reconcile_pending_spawn_intents().await?;
    assert_eq!(event_log_line_count(&fixture.paths)?, 0);
    assert_eq!(pending_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 0);

    sqlx::query("DROP TRIGGER fail_resolve_before_event")
        .execute(fixture.db.session_pool())
        .await?;
    service.reconcile_pending_spawn_intents().await?;

    assert_eq!(resolved_count(&fixture, session_id).await?, 1);
    assert_eq!(session_count(&fixture, session_id).await?, 1);
    assert_eq!(event_log_line_count(&fixture.paths)?, 1);

    runtime.append_event(running_event(session_id)).await?;
    assert_eq!(event_log_line_count(&fixture.paths)?, 1);
    runtime.shutdown().await?;
    Ok(())
}

async fn insert_audit<'e, E>(executor: E, id: &str, session_id: Uuid) -> Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    sqlx::query(
        "INSERT INTO identity_audit
         (id, timestamp, principal, action, resource, decision, session_ref)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(id)
    .bind("2026-05-28T00:00:00Z")
    .bind("local:0")
    .bind("runtime_spawn")
    .bind(format!("runtime:{session_id}"))
    .bind("allow")
    .bind(session_id.to_string())
    .execute(executor)
    .await?;
    Ok(())
}

async fn audit_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.identity_pool(),
        "SELECT COUNT(*) FROM identity_audit WHERE session_ref = ?",
        &session_id.to_string(),
    )
    .await
}

async fn lifecycle_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.runtime_pool(),
        "SELECT COUNT(*) FROM runtime_lifecycle WHERE session_id = ?",
        &session_id.to_string(),
    )
    .await
}

async fn pending_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'pending'",
        &session_id.to_string(),
    )
    .await
}

async fn resolved_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_spawn_intents WHERE session_id = ? AND status = 'resolved'",
        &session_id.to_string(),
    )
    .await
}

async fn session_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.session_pool(),
        "SELECT COUNT(*) FROM session_sessions WHERE id = ?",
        &session_id.to_string(),
    )
    .await
}

async fn allowed_spawn_audit_count(fixture: &IntegrationFixture, session_id: Uuid) -> Result<i64> {
    count_rows(
        fixture.db.identity_pool(),
        "SELECT COUNT(*) FROM identity_audit
         WHERE session_ref = ? AND action = '\"spawn\"' AND decision = '{\"kind\":\"allow\"}'",
        &session_id.to_string(),
    )
    .await
}

struct LiloDaemon {
    child: Option<Child>,
    lilo: PathBuf,
    home: PathBuf,
    socket: PathBuf,
    stderr: PathBuf,
    path: OsString,
}

impl LiloDaemon {
    fn start(fixture: &IntegrationFixture) -> Result<Self> {
        let lilo = assert_cmd::cargo::cargo_bin("lilo");
        let home = lilo_home(fixture)?;
        let socket = fixture.paths.run_root().join("lilod.sock");
        let fake_bin = fixture.paths.tmp_root().join("fake-bin");
        let stderr = fixture.paths.tmp_root().join("lilod.stderr.log");
        fs::create_dir_all(&fake_bin)?;
        write_sleeping_runtime(&fake_bin, "claude")?;
        let path = path_with_prefix(&fake_bin)?;
        let stderr_file = fs::File::create(&stderr).context("daemon stderr log creates")?;
        let mut child = Command::new(&lilo)
            .args(["daemon", "start"])
            .env("LILO_HOME", &home)
            .env("LILO_SOCKET_PATH", &socket)
            .env("HOME", &home)
            .env("PATH", &path)
            .stdout(Stdio::piped())
            .stderr(Stdio::from(stderr_file))
            .spawn()
            .context("lilo daemon start spawns")?;
        wait_for_socket(&socket, &mut child, &stderr)?;
        Ok(Self {
            child: Some(child),
            lilo,
            home,
            socket,
            stderr,
            path,
        })
    }

    fn command<const N: usize>(&self, args: [&str; N]) -> Command {
        let mut command = Command::new(&self.lilo);
        command
            .args(args)
            .env("LILO_HOME", &self.home)
            .env("LILO_SOCKET_PATH", &self.socket)
            .env("HOME", &self.home)
            .env("PATH", &self.path);
        command
    }

    fn stop(&mut self) -> String {
        let _ = Command::new(&self.lilo)
            .args(["daemon", "stop"])
            .env("LILO_HOME", &self.home)
            .env("LILO_SOCKET_PATH", &self.socket)
            .env("HOME", &self.home)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + DAEMON_TIMEOUT;
            while Instant::now() < deadline {
                if child.try_wait().ok().flatten().is_some() {
                    return self.stderr();
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            let _ = child.kill();
            let _ = child.wait();
            return self.stderr();
        }
        String::new()
    }

    fn stderr(&self) -> String {
        read_daemon_stderr(&self.stderr)
    }

    fn kill_and_stderr(&mut self) -> String {
        let Some(mut child) = self.child.take() else {
            return String::new();
        };
        let _ = child.kill();
        let _ = child.wait();
        self.stderr()
    }
}

impl Drop for LiloDaemon {
    fn drop(&mut self) {
        self.stop();
    }
}

fn lilo_home(fixture: &IntegrationFixture) -> Result<PathBuf> {
    fixture
        .paths
        .data_root()
        .parent()
        .map(Path::to_path_buf)
        .context("fixture data root has a home parent")
}

fn write_sleeping_runtime(dir: &Path, name: &str) -> Result<()> {
    let path = dir.join(name);
    fs::write(
        &path,
        "#!/bin/sh\ntrap 'exit 0' TERM INT\nwhile :; do sleep 60; done\n",
    )?;
    let mut permissions = fs::metadata(&path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)?;
    Ok(())
}

fn path_with_prefix(prefix: &Path) -> Result<OsString> {
    let paths = std::iter::once(prefix.to_path_buf()).chain(
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|path| std::env::split_paths(&path).collect::<Vec<_>>()),
    );
    std::env::join_paths(paths).context("PATH can be joined")
}

fn wait_for_socket(socket: &Path, child: &mut Child, stderr: &Path) -> Result<()> {
    let deadline = Instant::now() + DAEMON_TIMEOUT;
    let mut last_error = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket) {
            Ok(_) => return Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::NotFound | ErrorKind::ConnectionRefused
                ) =>
            {
                last_error = Some(error);
            }
            Err(error) => return Err(error).context("daemon socket connect failed"),
        }
        if let Some(status) = child.try_wait()? {
            bail!(
                "daemon exited before socket accepted connections: {status}\nstderr:\n{}",
                read_daemon_stderr(stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    bail!(
        "daemon socket did not accept connections at {}; last error={last_error:?}",
        socket.display()
    )
}

fn assert_success(command: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn stdout_session_id(output: &Output) -> Result<Uuid> {
    stdout(output)
        .split_whitespace()
        .next()
        .context("session output has an id")?
        .parse()
        .context("session id parses")
}

fn listed_session_ids(output: &Output) -> Result<Vec<Uuid>> {
    let sessions: serde_json::Value =
        serde_json::from_slice(&output.stdout).context("session list JSON parses")?;
    let array = sessions
        .as_array()
        .context("session list JSON is an array")?;
    array
        .iter()
        .map(|session| {
            session
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("session JSON has an id")?
                .parse()
                .context("session JSON id parses")
        })
        .collect()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}

fn read_daemon_stderr(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}
