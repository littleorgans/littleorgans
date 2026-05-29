use crate::common::{LOCAL_UID, TestDaemon, local_context};
use lilo_im_core::{Action, AuditDecision, Principal};
use lilo_session_core::{IdentityAuditRequest, IdentityWhoamiRequest, RpcResponse, SessionRpc};

#[tokio::test]
pub(crate) async fn identity_whoami_uses_request_context_principal() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();

    let whoami = daemon
        .state
        .handle(
            context,
            SessionRpc::IdentityWhoami {
                request: IdentityWhoamiRequest::default(),
            },
        )
        .await;
    let RpcResponse::IdentityWhoami { response } = whoami.response else {
        panic!("expected identity whoami response");
    };

    assert_eq!(response.principal, Principal::Local(LOCAL_UID));
    let rows = daemon.audit_rows().await;
    assert_eq!(rows[0].action, Action::Read);
    assert_eq!(rows[0].decision, AuditDecision::Allow);
}

#[tokio::test]
pub(crate) async fn identity_audit_authorizes_and_returns_audit_rows() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    daemon
        .state
        .handle(
            context.clone(),
            SessionRpc::IdentityWhoami {
                request: IdentityWhoamiRequest::default(),
            },
        )
        .await;

    let audit = daemon
        .state
        .handle(
            context,
            SessionRpc::IdentityAudit {
                request: IdentityAuditRequest {
                    principal: Some(Principal::Local(LOCAL_UID)),
                    action: Some(Action::Read),
                    since: None,
                    limit: Some(10),
                },
            },
        )
        .await;
    let RpcResponse::IdentityAudit { response } = audit.response else {
        panic!("expected identity audit response");
    };

    assert_eq!(response.rows.len(), 2);
    assert!(response.rows.iter().all(|row| row.action == Action::Read));
    assert!(
        response
            .rows
            .iter()
            .all(|row| row.principal == Principal::Local(LOCAL_UID))
    );
}

#[tokio::test]
pub(crate) async fn identity_whoami_denies_non_local_principal() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let denied = daemon
        .state
        .handle(
            lilo_session_daemon::identity_client::RequestContext::new(Principal::Local(
                LOCAL_UID + 1,
            )),
            SessionRpc::IdentityWhoami {
                request: IdentityWhoamiRequest::default(),
            },
        )
        .await;

    let RpcResponse::Error { message } = denied.response else {
        panic!("expected error response");
    };
    assert!(message.contains("unknown principal"), "{message}");
    let rows = daemon.audit_rows().await;
    assert_eq!(
        rows[0].decision,
        AuditDecision::Deny {
            reason: "non-local uid".to_owned(),
        }
    );
}
