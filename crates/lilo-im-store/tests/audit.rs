use std::collections::HashMap;

use chrono::{Duration, Utc};
use lilo_im_core::{
    Action, AuditDecision, AuditRow, AuditSink, Authorizer, AuthzError, Principal, ResourceSpec,
    RuntimeKind,
};
use lilo_im_store::schema::RESERVED_AUDIT_COLUMNS;
use lilo_im_store::{AuditFilters, AuditTableColumn, SqliteAuditSink, query_audit};
use lilo_im_stub::StubAuthorizer;
use uuid::Uuid;

#[tokio::test]
async fn sqlite_sink_persists_authorizer_audit_rows() {
    let temp_dir = tempfile::tempdir().expect("create audit sqlite temp dir");
    let db_path = temp_dir.path().join("audit.sqlite");
    let sink = SqliteAuditSink::connect(&db_path)
        .await
        .expect("connect audit sqlite sink");

    sink.run_migrations().await.expect("run audit migrations");
    let columns = sink
        .audit_table_columns()
        .await
        .expect("read audit table info");
    sink.run_migrations().await.expect("rerun audit migrations");
    assert_eq!(
        sink.audit_table_columns()
            .await
            .expect("read rerun table info"),
        columns
    );
    assert_reserved_columns_are_nullable(&columns);
    assert_primary_key_is_uuid_column(&columns);
    insta::assert_snapshot!("audit_table_columns", audit_columns_snapshot(&columns));

    let process_uid = nix::unistd::getuid().as_raw();
    let authorizer = StubAuthorizer::new(&sink, process_uid);
    let resource = resource();
    let started_at = Utc::now();

    for action in Action::ALL {
        let authorized = authorizer
            .authorize(&Principal::Local(process_uid), action, &resource)
            .await
            .expect("local uid should authorize");

        assert_eq!(authorized.principal, Principal::Local(process_uid));
        assert_eq!(authorized.role, "admin");
        assert!(authorized.capabilities.is_empty());
    }

    let rows = query_audit(&db_path, AuditFilters::default())
        .await
        .expect("read audit rows");
    assert_eq!(rows.len(), Action::ALL.len());

    for (row, expected_action) in rows.iter().zip(Action::ALL) {
        assert_eq!(row.principal, Principal::Local(process_uid));
        assert_eq!(row.action, expected_action);
        assert_eq!(row.resource, resource);
        assert_eq!(row.decision, AuditDecision::Allow);
        assert_eq!(row.session_ref, resource.session_id);
        assert!(row.timestamp >= started_at);
        assert!(row.timestamp <= Utc::now());
        assert_uuid_v7(row.id);
    }

    let denial = authorizer
        .authorize(
            &Principal::Local(different_uid(process_uid)),
            Action::Spawn,
            &resource,
        )
        .await;

    assert_eq!(denial, Err(AuthzError::UnknownPrincipal));
    let rows = query_audit(&db_path, AuditFilters::default())
        .await
        .expect("read audit rows after denial");
    let denied = &rows[Action::ALL.len()];
    assert_eq!(
        denied.decision,
        AuditDecision::Deny {
            reason: "non-local uid".to_owned(),
        }
    );
    assert_eq!(denied.denial_reason.as_deref(), Some("non-local uid"));
    assert_uuid_v7(denied.id);
}

#[tokio::test]
async fn query_audit_filters_rows_without_redeclaring_audit_types() {
    let temp_dir = tempfile::tempdir().expect("create audit sqlite temp dir");
    let db_path = temp_dir.path().join("audit.sqlite");
    let sink = SqliteAuditSink::connect(&db_path)
        .await
        .expect("connect audit sqlite sink");
    sink.run_migrations().await.expect("run audit migrations");

    let local_uid = nix::unistd::getuid().as_raw();
    let other_uid = different_uid(local_uid);
    let session = Uuid::now_v7();
    let old_timestamp = Utc::now() - Duration::minutes(10);
    let recent_timestamp = Utc::now();

    let old_spawn = audit_row(
        Principal::Local(local_uid),
        Action::Spawn,
        AuditDecision::Allow,
        Some(session),
        old_timestamp,
    );
    let recent_spawn = audit_row(
        Principal::Local(local_uid),
        Action::Spawn,
        AuditDecision::Allow,
        Some(session),
        recent_timestamp,
    );
    let recent_kill = audit_row(
        Principal::Local(local_uid),
        Action::Kill,
        AuditDecision::Allow,
        Some(session),
        recent_timestamp,
    );
    let other_spawn = audit_row(
        Principal::Local(other_uid),
        Action::Spawn,
        AuditDecision::Allow,
        Some(session),
        recent_timestamp,
    );

    for row in [
        old_spawn.clone(),
        recent_spawn.clone(),
        recent_kill.clone(),
        other_spawn.clone(),
    ] {
        sink.record(row).await.expect("record audit row");
    }

    let all = query_audit(&db_path, AuditFilters::default())
        .await
        .expect("read all audit rows");
    assert_eq!(
        all.iter().map(|row| row.id).collect::<Vec<_>>(),
        vec![
            old_spawn.id,
            recent_spawn.id,
            recent_kill.id,
            other_spawn.id,
        ]
    );

    let filtered = query_audit(
        &db_path,
        AuditFilters {
            principal: Some(Principal::Local(local_uid)),
            action: Some(Action::Spawn),
            since: Some(old_timestamp + Duration::minutes(1)),
            limit: Some(1),
        },
    )
    .await
    .expect("read filtered audit rows");

    assert_eq!(filtered, vec![recent_spawn]);
}

fn resource() -> ResourceSpec {
    ResourceSpec {
        workspace: Some("identity-matters".to_owned()),
        role: Some("worker".to_owned()),
        runtime: Some(RuntimeKind::Codex),
        session_id: Some(Uuid::now_v7()),
        labels: HashMap::from([("issue".to_owned(), "ALP-2457".to_owned())]),
    }
}

fn audit_row(
    principal: Principal,
    action: Action,
    decision: AuditDecision,
    session_id: Option<Uuid>,
    timestamp: chrono::DateTime<Utc>,
) -> AuditRow {
    AuditRow {
        id: Uuid::now_v7(),
        timestamp,
        principal,
        action,
        resource: ResourceSpec {
            session_id,
            ..Default::default()
        },
        decision,
        session_ref: session_id,
        notes: None,
        policy_id: None,
        evaluation_trace: None,
        denial_reason: None,
    }
}

fn assert_reserved_columns_are_nullable(columns: &[AuditTableColumn]) {
    for name in RESERVED_AUDIT_COLUMNS {
        let column = audit_column(columns, name);
        assert!(!column.not_null, "{name} should be nullable");
    }
}

fn audit_columns_snapshot(columns: &[AuditTableColumn]) -> String {
    columns
        .iter()
        .map(|column| {
            format!(
                "{} {} not_null={} primary_key={}",
                column.name, column.data_type, column.not_null, column.primary_key
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_primary_key_is_uuid_column(columns: &[AuditTableColumn]) {
    let id = audit_column(columns, "id");
    assert!(id.primary_key);
    assert_eq!(id.data_type, "TEXT");
}

fn audit_column<'a>(columns: &'a [AuditTableColumn], name: &str) -> &'a AuditTableColumn {
    columns
        .iter()
        .find(|column| column.name == name)
        .unwrap_or_else(|| panic!("missing audit column {name}"))
}

fn assert_uuid_v7(id: Uuid) {
    assert_eq!(id.to_string().chars().nth(14), Some('7'));
}

fn different_uid(uid: u32) -> u32 {
    uid.checked_add(1).unwrap_or(uid - 1)
}
