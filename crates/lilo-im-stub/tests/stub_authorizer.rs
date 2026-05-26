use std::sync::Mutex;

use async_trait::async_trait;
use lilo_im_core::{
    Action, AuditDecision, AuditError, AuditRow, AuditSink, Authorized, Authorizer, AuthzError,
    Principal, ResourceSpec,
};
use lilo_im_stub::StubAuthorizer;

#[derive(Default)]
struct MockAuditSink {
    rows: Mutex<Vec<AuditRow>>,
}

impl MockAuditSink {
    fn rows(&self) -> Vec<AuditRow> {
        self.rows.lock().expect("audit rows lock poisoned").clone()
    }
}

#[async_trait]
impl AuditSink for MockAuditSink {
    async fn record(&self, row: AuditRow) -> Result<(), AuditError> {
        self.rows
            .lock()
            .expect("audit rows lock poisoned")
            .push(row);
        Ok(())
    }
}

#[tokio::test]
async fn authorizes_local_uid_and_audits_both_decisions_with_mock_sink() {
    let mock = MockAuditSink::default();
    let process_uid = nix::unistd::getuid().as_raw();
    authorize_both_decisions(&mock, process_uid).await;
    let rows = mock.rows();

    assert_audited_both_decisions(&rows, process_uid);
    insta::assert_snapshot!(
        "stub_authorizer_audit_decisions",
        format_stub_audit_rows(&rows, process_uid)
    );
}

async fn authorize_both_decisions<S>(audit_sink: &S, process_uid: u32)
where
    S: AuditSink + ?Sized,
{
    let authorizer = StubAuthorizer::new(audit_sink, process_uid);
    let resource = ResourceSpec::default();

    let allowed = authorizer
        .authorize(&Principal::Local(process_uid), Action::Spawn, &resource)
        .await;

    assert_eq!(
        allowed,
        Ok(Authorized {
            principal: Principal::Local(process_uid),
            role: "admin".to_owned(),
            capabilities: Vec::new(),
        })
    );

    let denied = authorizer
        .authorize(&Principal::Local(process_uid + 1), Action::Spawn, &resource)
        .await;

    assert_eq!(denied, Err(AuthzError::UnknownPrincipal));
}

fn assert_audited_both_decisions(rows: &[AuditRow], process_uid: u32) {
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].decision, AuditDecision::Allow);
    assert_eq!(rows[0].action, Action::Spawn);
    assert_eq!(rows[0].principal, Principal::Local(process_uid));
    assert_eq!(
        rows[1].decision,
        AuditDecision::Deny {
            reason: "non-local uid".to_owned(),
        }
    );
    assert_eq!(rows[1].denial_reason.as_deref(), Some("non-local uid"));
}

fn format_stub_audit_rows(rows: &[AuditRow], process_uid: u32) -> String {
    rows.iter()
        .map(|row| {
            format!(
                "principal={} action={:?} decision={:?} denial_reason={}",
                principal_label(&row.principal, process_uid),
                row.action,
                row.decision,
                row.denial_reason.as_deref().unwrap_or("<none>")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn principal_label(principal: &Principal, process_uid: u32) -> &'static str {
    match principal {
        Principal::Local(uid) if *uid == process_uid => "Local(uid=N)",
        Principal::Local(_) => "Local(uid=other)",
        Principal::Unknown { .. } => "Unknown",
    }
}
