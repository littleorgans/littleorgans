use anyhow::{Context, Result};
use chrono::Utc;
use lilo_im_core::{Action, AuditDecision, AuditRow, AuditSink, Principal, ResourceSpec};
use lilo_integration_tests::{IntegrationFixture, fixed_uuid};
use lilo_test_support::{LiloDaemon, assert_success, fake_runtime_path, stdout};
use std::fs;
use std::path::{Path, PathBuf};

#[tokio::test]
async fn identity_cli_reads_principal_and_seeded_audit_rows() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    seed_audit_row(&fixture).await?;
    let runtime_path = fake_runtime_path("claude")?;
    let mut daemon = LiloDaemon::start(
        lilo_home(&fixture)?,
        fixture.paths.run_root().join("lilod.sock"),
        Some(runtime_path.path()),
    )?;

    let whoami = daemon
        .command(["identity", "whoami"])
        .output()
        .context("lilo identity whoami executes")?;
    assert_success("lilo identity whoami", &whoami);
    assert!(
        stdout(&whoami).starts_with("local:"),
        "stdout: {}",
        stdout(&whoami)
    );

    let audit = daemon
        .command(["identity", "audit", "--limit", "10", "--output", "json"])
        .output()
        .context("lilo identity audit executes")?;
    assert_success("lilo identity audit --output json", &audit);
    let response: serde_json::Value =
        serde_json::from_slice(&audit.stdout).context("identity audit JSON parses")?;
    let rows = response
        .get("rows")
        .and_then(serde_json::Value::as_array)
        .context("identity audit JSON has rows")?;

    assert!(
        rows.iter()
            .any(|row| row.get("id").and_then(serde_json::Value::as_str)
                == Some(&fixed_uuid(90).to_string())),
        "stdout: {}",
        stdout(&audit)
    );

    daemon.stop();
    Ok(())
}

async fn seed_audit_row(fixture: &IntegrationFixture) -> Result<()> {
    fs::create_dir_all(fixture.paths.data_root())?;
    let row = AuditRow {
        id: fixed_uuid(90),
        timestamp: Utc::now(),
        principal: Principal::Local(0),
        action: Action::Read,
        resource: ResourceSpec::default(),
        decision: AuditDecision::Allow,
        session_ref: None,
        notes: Some("seeded identity audit fixture".to_string()),
        policy_id: None,
        evaluation_trace: None,
        denial_reason: None,
    };
    let sink = lilo_im_store::SqliteAuditSink::with_pool(fixture.db.identity_pool().clone());
    sink.record(row).await?;
    Ok(())
}

fn lilo_home(fixture: &IntegrationFixture) -> Result<PathBuf> {
    fixture
        .paths
        .data_root()
        .parent()
        .map(Path::to_path_buf)
        .context("fixture data root has a home parent")
}
