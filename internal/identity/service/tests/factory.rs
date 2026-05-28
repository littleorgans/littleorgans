use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_im_core::{Action, AuditDecision, Principal, ResourceSpec};
use lilo_im_store::AuditFilters;
use lilo_paths::{LiloHome, LiloPaths};

async fn open_test_db() -> (tempfile::TempDir, LiloDb) {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let home = LiloHome::from_path(tempdir.path().join("lilo")).expect("home path");
    let db = LiloDb::open(&LiloPaths::new(home))
        .await
        .expect("open lilo db");
    (tempdir, db)
}

#[tokio::test]
async fn client_from_db_authorizes_and_records_an_audit_row() {
    let (_tempdir, db) = open_test_db().await;
    let local_uid = 501;
    let principal = Principal::local(local_uid);
    let resource = ResourceSpec::default();

    let client = IdentityClient::from_db(&db, local_uid);

    client
        .authorize(&principal, Action::Spawn, &resource)
        .await
        .expect("authorize local principal");

    let rows = client
        .audit_sink()
        .query_audit(AuditFilters::default())
        .await
        .expect("query audit rows");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].principal, principal);
    assert_eq!(rows[0].action, Action::Spawn);
    assert_eq!(rows[0].resource, resource);
    assert_eq!(rows[0].decision, AuditDecision::Allow);
}

/// The stub authorizer path (`IdentityClient::authorize` -> `StubAuthorizer`)
/// and the in-transaction path (`IdentityClient::authorize_in_tx`) must reach
/// the same decision for the same principal. Before both paths shared
/// `AuditDecision::evaluate_local`, the in-tx path re-derived allow/deny on its
/// own and could drift from the audit decision it had just recorded. This test
/// pins them together: a non-local principal is denied with the identical
/// decision and reason on both paths, so they cannot silently diverge again.
#[tokio::test]
async fn stub_and_in_tx_paths_agree_on_denial_for_non_local_principal() {
    let (_tempdir, db) = open_test_db().await;
    let local_uid = 501;
    // Same enum variant as the allowed principal, different uid: a non-local principal.
    let principal = Principal::local(local_uid + 1);
    let resource = ResourceSpec::default();
    let client = IdentityClient::from_db(&db, local_uid);
    let expected = AuditDecision::Deny {
        reason: "non-local uid".to_owned(),
    };

    // Stub path: records a Deny row, then returns an error.
    let stub_result = client.authorize(&principal, Action::Spawn, &resource).await;
    assert!(
        stub_result.is_err(),
        "stub path must deny a non-local principal"
    );

    // In-transaction path: records a Deny row via the passed connection, then
    // returns an error derived from the same decision.
    let mut conn = db
        .identity_pool()
        .acquire()
        .await
        .expect("acquire identity connection");
    let in_tx_result = client
        .authorize_in_tx(&mut conn, &principal, Action::Spawn, &resource)
        .await;
    drop(conn);
    assert!(
        in_tx_result.is_err(),
        "in-tx path must deny a non-local principal"
    );

    // Both paths recorded exactly one row, and both decisions are identical.
    let rows = client
        .audit_sink()
        .query_audit(AuditFilters::default())
        .await
        .expect("query audit rows");
    assert_eq!(rows.len(), 2, "stub and in-tx paths each record one row");
    for row in &rows {
        assert_eq!(
            row.decision, expected,
            "stub and in-tx paths must record the identical decision"
        );
        assert_eq!(row.denial_reason.as_deref(), Some("non-local uid"));
    }
}
