use lilo_identity_service::{IdentityConfig, IdentityService};
use lilo_im_core::{Action, AuditDecision, Principal, ResourceSpec};
use lilo_im_store::AuditFilters;

#[tokio::test]
async fn factory_authorizes_and_records_an_audit_row() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let db_path = tempdir.path().join("identity.db");
    let local_uid = 501;
    let principal = Principal::local(local_uid);
    let resource = ResourceSpec::default();

    let service = IdentityService::build(IdentityConfig::new(db_path, local_uid))
        .await
        .expect("build identity service");

    let authorized = service
        .authorize(&principal, Action::Spawn, &resource)
        .await
        .expect("authorize local principal");
    assert_eq!(authorized.principal, principal);

    let rows = service
        .query_audit(AuditFilters::default())
        .await
        .expect("query audit rows");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].principal, principal);
    assert_eq!(rows[0].action, Action::Spawn);
    assert_eq!(rows[0].resource, resource);
    assert_eq!(rows[0].decision, AuditDecision::Allow);
}
