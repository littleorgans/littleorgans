use lilo_im_core::Action;
use lilo_session_core::SessionRpc;

/// Where a verb's authorization decision is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AuthzPlan {
    /// Coarse decision at the door, before dispatch.
    AtDoor { action: Action },
    /// Verb authorizes itself after resolving its resource.
    Downstream,
}

/// Classify each session RPC by its authorization boundary.
///
/// This match is exhaustive by design. Do not add a `_` arm. A new
/// `SessionRpc` variant must fail to compile until its authorization boundary
/// is declared here.
pub(crate) fn authz_plan(rpc: &SessionRpc) -> AuthzPlan {
    use AuthzPlan::{AtDoor, Downstream};

    match rpc {
        SessionRpc::List { .. } | SessionRpc::NamespaceList { .. } => AtDoor {
            action: Action::List,
        },
        SessionRpc::NamespaceGet { .. } | SessionRpc::Wait { .. } => AtDoor {
            action: Action::Read,
        },
        SessionRpc::MailCheck { .. } | SessionRpc::MailStopCheck { .. } => AtDoor {
            action: Action::MailRead,
        },
        SessionRpc::NamespaceCreate { .. } => AtDoor {
            action: Action::Kill,
        },
        SessionRpc::Spawn { .. }
        | SessionRpc::NamespaceDelete { .. }
        | SessionRpc::Delete { .. }
        | SessionRpc::MailSend { .. }
        | SessionRpc::MailRead { .. }
        | SessionRpc::Nudge { .. }
        | SessionRpc::Label { .. }
        | SessionRpc::Logs { .. }
        | SessionRpc::Capture { .. }
        | SessionRpc::Doctor { .. }
        | SessionRpc::McpBridge { .. }
        | SessionRpc::Shutdown => Downstream,
    }
}

#[cfg(test)]
mod tests {
    use lilo_im_core::Action;
    use lilo_session_core::{
        CaptureRequest, DeleteRequest, DoctorRequest, IsolationPolicy, LabelMutation, LabelRequest,
        ListRequest, LogsRequest, MailCheckRequest, MailReadRequest, MailSendRequest,
        MailStopCheckRequest, McpBridgeRequest, Namespace, NamespaceCreateRequest,
        NamespaceDeleteRequest, NamespaceGetRequest, NamespaceListRequest, NudgeRequest,
        RuntimeKind, Selector, SessionRpc, SpawnRequest, WaitCondition, WaitRequest,
    };
    use uuid::Uuid;

    use super::{AuthzPlan, authz_plan};

    #[test]
    fn newly_gated_verbs_authorize_at_the_door() {
        for (rpc, action) in at_door_cases() {
            assert_eq!(authz_plan(&rpc), AuthzPlan::AtDoor { action });
        }
    }

    #[test]
    fn remaining_verbs_authorize_downstream() {
        for rpc in downstream_cases() {
            assert_eq!(authz_plan(&rpc), AuthzPlan::Downstream);
        }
    }

    fn at_door_cases() -> Vec<(SessionRpc, Action)> {
        vec![
            (
                SessionRpc::List {
                    request: ListRequest::default(),
                },
                Action::List,
            ),
            (
                SessionRpc::NamespaceList {
                    request: NamespaceListRequest::default(),
                },
                Action::List,
            ),
            (
                SessionRpc::NamespaceGet {
                    request: NamespaceGetRequest {
                        slug: "team".to_string(),
                    },
                },
                Action::Read,
            ),
            (
                SessionRpc::MailCheck {
                    request: MailCheckRequest {
                        selector: Selector::All,
                    },
                },
                Action::MailRead,
            ),
            (
                SessionRpc::MailStopCheck {
                    request: MailStopCheckRequest {
                        selector: Selector::All,
                    },
                },
                Action::MailRead,
            ),
            (
                SessionRpc::Wait {
                    request: WaitRequest {
                        selector: Selector::All,
                        condition: WaitCondition::Running,
                        timeout_secs: 0,
                    },
                },
                Action::Read,
            ),
            (
                SessionRpc::NamespaceCreate {
                    request: NamespaceCreateRequest {
                        slug: "team".to_string(),
                    },
                },
                Action::Kill,
            ),
        ]
    }

    fn downstream_cases() -> Vec<SessionRpc> {
        vec![
            SessionRpc::Spawn {
                request: Box::new(SpawnRequest {
                    runtime: RuntimeKind::Claude,
                    role: "general".to_string(),
                    workspace: "/tmp".to_string(),
                    dir: None,
                    namespace: None,
                    target: "headless".to_string(),
                    agent_config: None,
                    isolation: IsolationPolicy::default(),
                    image: None,
                    env: Vec::new(),
                    mounts: Vec::new(),
                    shell_resume: None,
                    labels: Vec::new(),
                    force: false,
                }),
            },
            SessionRpc::NamespaceDelete {
                request: NamespaceDeleteRequest {
                    namespace: Namespace::new("team").expect("valid namespace"),
                },
            },
            SessionRpc::Delete {
                request: DeleteRequest {
                    selector: Selector::All,
                    signal: "SIGTERM".to_string(),
                    grace_secs: 5,
                },
            },
            SessionRpc::MailSend {
                request: MailSendRequest {
                    from: None,
                    to: Selector::All,
                    content: "hello".to_string(),
                },
            },
            SessionRpc::MailRead {
                request: MailReadRequest {
                    selector: Selector::All,
                    peek: false,
                },
            },
            SessionRpc::Nudge {
                request: NudgeRequest {
                    to: Selector::All,
                    content: "ping".to_string(),
                },
            },
            SessionRpc::Label {
                request: LabelRequest {
                    selector: Selector::All,
                    mutation: LabelMutation::Remove {
                        key: "scope".to_string(),
                    },
                },
            },
            SessionRpc::Logs {
                request: LogsRequest {
                    selector: Selector::All,
                    max_bytes: None,
                },
            },
            SessionRpc::Capture {
                request: CaptureRequest {
                    session_id: Uuid::nil(),
                    scrollback_lines: None,
                },
            },
            SessionRpc::Doctor {
                request: DoctorRequest::default(),
            },
            SessionRpc::McpBridge {
                request: McpBridgeRequest {
                    line: "{}".to_string(),
                    caller_session_id: None,
                },
            },
            SessionRpc::Shutdown,
        ]
    }
}
