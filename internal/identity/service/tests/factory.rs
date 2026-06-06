use lilo_db::test_support::TestDb;
use lilo_identity_service::IdentityClient;
use lilo_im_core::{Action, AuditDecision, Principal, ResourceSpec};
use lilo_im_store::AuditFilters;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn client_from_db_authorizes_and_records_an_audit_row() {
    let testdb = TestDb::create().await.expect("create test db");
    let local_uid = 501;
    let principal = Principal::local(local_uid);
    let resource = ResourceSpec::default();

    let client = IdentityClient::from_db(testdb.db(), local_uid);

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

    testdb.cleanup().await.expect("cleanup test db");
}

/// The stub authorizer path (`IdentityClient::authorize` -> `StubAuthorizer`)
/// and the in-transaction path (`IdentityClient::authorize_in_tx`) must reach
/// the same decision for the same principal. Before both paths shared
/// `AuditDecision::evaluate_local`, the in-tx path re-derived allow/deny on its
/// own and could drift from the audit decision it had just recorded. This test
/// pins them together: a non-local principal is denied with the identical
/// decision and reason on both paths, so they cannot silently diverge again.
#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn stub_and_in_tx_paths_agree_on_denial_for_non_local_principal() {
    let testdb = TestDb::create().await.expect("create test db");
    let local_uid = 501;
    // Same enum variant as the allowed principal, different uid: a non-local principal.
    let principal = Principal::local(local_uid + 1);
    let resource = ResourceSpec::default();
    let client = IdentityClient::from_db(testdb.db(), local_uid);
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
    let mut tx = testdb.db().pool().begin().await.expect("begin identity tx");
    let in_tx_result = client
        .authorize_in_tx(&mut tx, &principal, Action::Spawn, &resource)
        .await;
    lilo_db::commit_or_rollback(tx, Ok::<(), sqlx::Error>(()))
        .await
        .expect("commit identity audit tx");
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

    testdb.cleanup().await.expect("cleanup test db");
}
