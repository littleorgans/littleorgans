use lilo_db::LiloDb;
use lilo_identity_service::IdentityClient;
use lilo_im_core::{Action, AuditDecision, Principal, ResourceSpec};
use lilo_im_store::AuditFilters;
use lilo_paths::{LiloHome, LiloPaths};

#[tokio::test]
async fn client_from_db_authorizes_and_records_an_audit_row() {
    let tempdir = tempfile::tempdir().expect("create tempdir");
    let home = LiloHome::from_path(tempdir.path().join("lilo")).expect("home path");
    let db = LiloDb::open(&LiloPaths::new(home))
        .await
        .expect("open lilo db");
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
