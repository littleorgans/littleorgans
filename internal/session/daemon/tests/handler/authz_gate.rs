use std::collections::HashMap;

use crate::common::{LOCAL_UID, TestDaemon};
use lilo_im_core::{Action, AuditDecision, Principal};
use lilo_session_core::{
    ListRequest, MailCheckRequest, MailStopCheckRequest, NamespaceCreateRequest,
    NamespaceGetRequest, NamespaceListRequest, RpcResponse, Selector, SessionRpc, WaitCondition,
    WaitRequest,
};
use lilo_session_daemon::identity_client::RequestContext;

#[tokio::test]
pub(crate) async fn newly_door_gated_verbs_reject_unknown_principal() {
    let daemon = TestDaemon::new(LOCAL_UID).await;

    for (name, rpc) in newly_gated_rpc_cases() {
        let response = daemon.state.handle(unknown_context(), rpc).await;
        let RpcResponse::Error { message } = response.response else {
            panic!("{name} should reject unknown principal");
        };
        assert!(
            message.contains("unknown principal"),
            "{name} returned unexpected error: {message}"
        );
    }

    let rows = daemon.audit_rows().await;
    assert_eq!(rows.len(), 7);
    assert!(rows.iter().all(|row| {
        row.decision
            == AuditDecision::Deny {
                reason: "unknown principal".to_string(),
            }
    }));
    assert_eq!(action_counts(&rows), expected_action_counts());
}

fn newly_gated_rpc_cases() -> Vec<(&'static str, SessionRpc)> {
    vec![
        (
            "list",
            SessionRpc::List {
                request: ListRequest::default(),
            },
        ),
        (
            "namespace create",
            SessionRpc::NamespaceCreate {
                request: NamespaceCreateRequest {
                    slug: "team".to_string(),
                },
            },
        ),
        (
            "namespace get",
            SessionRpc::NamespaceGet {
                request: NamespaceGetRequest {
                    slug: "team".to_string(),
                },
            },
        ),
        (
            "namespace list",
            SessionRpc::NamespaceList {
                request: NamespaceListRequest::default(),
            },
        ),
        (
            "mail check",
            SessionRpc::MailCheck {
                request: MailCheckRequest {
                    selector: Selector::All,
                },
            },
        ),
        (
            "mail stop check",
            SessionRpc::MailStopCheck {
                request: MailStopCheckRequest {
                    selector: Selector::All,
                },
            },
        ),
        (
            "wait",
            SessionRpc::Wait {
                request: WaitRequest {
                    selector: Selector::All,
                    condition: WaitCondition::Running,
                    timeout_secs: 0,
                },
            },
        ),
    ]
}

fn unknown_context() -> RequestContext {
    RequestContext::new(Principal::Unknown {
        kind: "remote".to_string(),
        raw: serde_json::json!({ "uid": 7 }),
    })
}

fn action_counts(rows: &[lilo_im_core::AuditRow]) -> HashMap<Action, usize> {
    let mut counts = HashMap::new();
    for row in rows {
        *counts.entry(row.action).or_default() += 1;
    }
    counts
}

fn expected_action_counts() -> HashMap<Action, usize> {
    HashMap::from([
        (Action::List, 2),
        (Action::Read, 2),
        (Action::MailRead, 2),
        (Action::Kill, 1),
    ])
}
