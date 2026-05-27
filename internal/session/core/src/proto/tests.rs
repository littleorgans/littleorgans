use lilo_rm_core::{IsolationPolicy, MountSpec};

use super::{DeleteRequest, MailSendRequest, NudgeRequest, SessionRpc, SpawnRequest};
use crate::test_support::OrPanic as _;
use crate::{RuntimeKind, Selector};

#[test]
fn spawn_request_round_trips_as_tagged_json() {
    let request = SessionRpc::Spawn {
        request: Box::new(SpawnRequest {
            runtime: RuntimeKind::Claude,
            role: "general".to_string(),
            workspace: "test".to_string(),
            dir: None,
            namespace: None,
            target: "headless".to_string(),
            agent_config: None,
            isolation: IsolationPolicy::Docker(lilo_rm_core::IsolationProfile::default()),
            image: Some("runtime-matters-claude:local".to_string()),
            env: Vec::new(),
            mounts: vec![MountSpec {
                source: "/host/config".into(),
                target: "/container/config".into(),
                read_only: true,
            }],
            shell_resume: None,
            labels: Vec::new(),
            force: true,
        }),
    };

    let json = serde_json::to_string(&request).or_panic("serializes request");
    let decoded: SessionRpc = serde_json::from_str(&json).or_panic("decodes request");

    assert_eq!(decoded, request);
}

#[test]
fn spawn_request_decodes_legacy_payload_without_new_fields() {
    let json = r#"{
        "type": "spawn",
        "request": {
            "runtime": "claude",
            "role": "general",
            "workspace": "/tmp/project"
        }
    }"#;

    let decoded: SessionRpc = serde_json::from_str(json).or_panic("decodes legacy request");
    let SessionRpc::Spawn { request } = decoded else {
        panic!("expected spawn request");
    };
    assert_eq!(request.workspace, "/tmp/project");
    assert_eq!(request.dir, None);
    assert_eq!(request.namespace, None);
    assert_eq!(request.target, "headless");
    assert_eq!(request.isolation, IsolationPolicy::Host);
    assert_eq!(request.image, None);
    assert_eq!(request.mounts, Vec::new());
    assert!(!request.force);
}

#[test]
fn spawn_request_decodes_new_payload_without_legacy_workspace() {
    let json = r#"{
        "type": "spawn",
        "request": {
            "runtime": "claude",
            "role": "general",
            "dir": "/tmp/project",
            "namespace": "alpha"
        }
    }"#;

    let decoded: SessionRpc = serde_json::from_str(json).or_panic("decodes new request");
    let SessionRpc::Spawn { request } = decoded else {
        panic!("expected spawn request");
    };
    assert_eq!(request.workspace, "");
    assert_eq!(request.dir.as_deref(), Some("/tmp/project"));
    assert_eq!(
        request.namespace.or_panic("expected value").as_str(),
        "alpha"
    );
    assert_eq!(request.target, "headless");
    assert!(!request.force);
}

#[test]
fn delete_request_round_trips_as_tagged_json() {
    let request = SessionRpc::Delete {
        request: DeleteRequest {
            selector: Selector::Id {
                id: "019e32e3-0000-7000-8000-000000000000"
                    .parse()
                    .or_panic("expected value"),
            },
            signal: "SIGTERM".to_string(),
            grace_secs: 5,
        },
    };

    let json = serde_json::to_string(&request).or_panic("serializes request");
    let decoded: SessionRpc = serde_json::from_str(&json).or_panic("decodes request");

    assert_eq!(decoded, request);
}

#[test]
fn mail_request_round_trips_as_tagged_json() {
    let request = SessionRpc::MailSend {
        request: MailSendRequest {
            from: Some("019e32e3-0000-7000-8000-000000000000".to_string()),
            to: Selector::Id {
                id: "019e32e3-0000-7000-8000-000000000001"
                    .parse()
                    .or_panic("expected value"),
            },
            content: "review the spec".to_string(),
        },
    };

    let json = serde_json::to_string(&request).or_panic("serializes request");
    let decoded: SessionRpc = serde_json::from_str(&json).or_panic("decodes request");

    assert_eq!(decoded, request);
}

#[test]
fn nudge_request_round_trips_as_tagged_json() {
    let request = SessionRpc::Nudge {
        request: NudgeRequest {
            to: Selector::Id {
                id: "019e32e3-0000-7000-8000-000000000001"
                    .parse()
                    .or_panic("expected value"),
            },
            content: "ping".to_string(),
        },
    };

    let json = serde_json::to_string(&request).or_panic("serializes request");
    let decoded: SessionRpc = serde_json::from_str(&json).or_panic("decodes request");

    assert_eq!(decoded, request);
}
