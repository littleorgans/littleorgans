use std::path::PathBuf;

use chrono::Utc;
use lilo_common::id::SessionId;
use lilo_db::test_support::{TestDb, now_micros};
use lilo_session_core::{Label, LabelOp, Namespace, Selector};

use super::super::test_support::LOST_EVIDENCE_VARIANTS;
use crate::test_support::{ErrOrPanic as _, OrPanic as _};

use super::*;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn inserts_and_lists_sessions() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let now = now_micros();
    let session = Session {
        id: SessionId::from_uuid(uuid::Uuid::now_v7()),
        runtime: RuntimeKind::Claude,
        role: "general".to_string(),
        workspace: "test".to_string(),
        namespace: Namespace::default(),
        dir: PathBuf::from("test"),
        state: SessionState::Running,
        runtime_pid: 42,
        runtime_session: None,
        transcript_path: None,
        tmux_pane: None,
        agent_config: None,
        created_at: now,
        started_at: now,
        terminated_at: None,
        exit_code: None,
        updated_at: now,
        labels: Vec::new(),
    };

    store
        .insert_session(&session)
        .await
        .or_panic("session inserts");

    assert_eq!(
        store.list_sessions(None).await.or_panic("sessions list"),
        vec![session]
    );
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn marks_session_terminated() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let session = test_session("general", "test", Vec::new());
    store
        .insert_session(&session)
        .await
        .or_panic("session inserts");

    let terminated_at = now_micros();
    let terminated = store
        .mark_session_terminated(&session.id, Some(137), terminated_at)
        .await
        .or_panic("session updates")
        .or_panic("session exists");

    assert_eq!(terminated.state, SessionState::Terminated);
    assert_eq!(terminated.exit_code, Some(137));
    assert_eq!(terminated.terminated_at, Some(terminated_at));
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn records_transcript_path_without_runtime_session() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let session = test_session("engineer", "test", Vec::new());
    store
        .insert_session(&session)
        .await
        .or_panic("session inserts");
    let transcript = std::path::Path::new("/tmp/rtmd-stdout.log");

    let recorded_at = now_micros();
    let updated = store
        .record_transcript_path(&session.id, transcript, recorded_at)
        .await
        .or_panic("transcript records")
        .or_panic("session exists");

    assert_eq!(updated.runtime_session, None);
    assert_eq!(updated.transcript_path.as_deref(), Some(transcript));
    assert_eq!(updated.updated_at, recorded_at);

    let later = recorded_at + chrono::Duration::seconds(30);
    let unchanged = store
        .record_transcript_path(&session.id, transcript, later)
        .await
        .or_panic("transcript no-ops")
        .or_panic("session exists");

    assert_eq!(unchanged.updated_at, recorded_at);
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn selector_queries_return_sessions_with_labels() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let auth_pm = test_session(
        "pm",
        "test",
        vec![
            Label {
                key: "area".to_string(),
                value: "auth".to_string(),
            },
            Label {
                key: "pri".to_string(),
                value: "high".to_string(),
            },
        ],
    );
    let auth_engineer = test_session(
        "engineer",
        "test",
        vec![Label {
            key: "area".to_string(),
            value: "auth".to_string(),
        }],
    );
    let ui_engineer = test_session(
        "engineer",
        "test",
        vec![Label {
            key: "area".to_string(),
            value: "ui".to_string(),
        }],
    );
    for session in [&auth_pm, &auth_engineer, &ui_engineer] {
        store
            .insert_session(session)
            .await
            .or_panic("session inserts");
    }

    let engineers = store
        .list_sessions_by_selector(&Selector::Role {
            name: "engineer".to_string(),
        })
        .await
        .or_panic("role selector resolves");
    assert_eq!(
        session_ids(&engineers),
        vec![auth_engineer.id, ui_engineer.id]
    );

    let auth_area = store
        .list_sessions_by_selector(&Selector::Label {
            key: "area".to_string(),
            op: LabelOp::Eq {
                value: "auth".to_string(),
            },
        })
        .await
        .or_panic("label selector resolves");
    assert_eq!(session_ids(&auth_area), vec![auth_pm.id, auth_engineer.id]);
    assert_eq!(
        auth_area[0].labels,
        vec![
            Label {
                key: "area".to_string(),
                value: "auth".to_string(),
            },
            Label {
                key: "pri".to_string(),
                value: "high".to_string(),
            },
        ]
    );

    let in_area = store
        .list_sessions_by_selector(&Selector::Label {
            key: "area".to_string(),
            op: LabelOp::In {
                values: vec!["auth".to_string(), "ui".to_string()],
            },
        })
        .await
        .or_panic("label in selector resolves");
    assert_eq!(
        session_ids(&in_area),
        vec![auth_pm.id, auth_engineer.id, ui_engineer.id]
    );
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn selector_queries_filter_by_namespace_dir_and_scope() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let alpha = Namespace::new("alpha").or_panic("namespace");
    let beta = Namespace::new("beta").or_panic("namespace");
    let mut alpha_engineer = test_session("engineer", "/tmp/alpha", Vec::new());
    alpha_engineer.namespace = alpha.clone();
    let mut alpha_pm = test_session("pm", "/tmp/alpha", Vec::new());
    alpha_pm.namespace = alpha.clone();
    let mut beta_engineer = test_session("engineer", "/tmp/beta", Vec::new());
    beta_engineer.namespace = beta.clone();
    for namespace in [&alpha, &beta] {
        store
            .create_namespace(namespace, Utc::now())
            .await
            .or_panic("namespace creates");
    }
    for session in [&alpha_engineer, &alpha_pm, &beta_engineer] {
        store
            .insert_session(session)
            .await
            .or_panic("session inserts");
    }

    let alpha_sessions = store
        .list_sessions_by_selector(&Selector::Namespace { namespace: alpha })
        .await
        .or_panic("namespace selector resolves");
    assert_eq!(
        session_ids(&alpha_sessions),
        vec![alpha_engineer.id, alpha_pm.id]
    );

    let beta_dir_sessions = store
        .list_sessions_by_selector(&Selector::Dir {
            path: PathBuf::from("/tmp/beta"),
        })
        .await
        .or_panic("dir selector resolves");
    assert_eq!(session_ids(&beta_dir_sessions), vec![beta_engineer.id]);

    let scoped_engineers = store
        .list_sessions_by_selector(&Selector::And {
            selectors: vec![
                Selector::Namespace { namespace: beta },
                Selector::Role {
                    name: "engineer".to_string(),
                },
            ],
        })
        .await
        .or_panic("scoped selector resolves");
    assert_eq!(session_ids(&scoped_engineers), vec![beta_engineer.id]);
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn selector_prefix_resolves_unique_and_none() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let first = test_session_with_id(
        "12345678-1234-4234-9234-123456789abc",
        "engineer",
        "/tmp/first",
    );
    let second = test_session_with_id("22345678-1234-4234-9234-123456789abc", "pm", "/tmp/second");
    for session in [&first, &second] {
        store
            .insert_session(session)
            .await
            .or_panic("session inserts");
    }

    let matched = store
        .list_sessions_by_selector(&Selector::Prefix {
            prefix: "1234".to_string(),
        })
        .await
        .or_panic("prefix selector resolves");
    assert_eq!(session_ids(&matched), vec![first.id]);

    let missing = store
        .list_sessions_by_selector(&Selector::Prefix {
            prefix: "ffff".to_string(),
        })
        .await
        .or_panic("missing prefix resolves");
    assert!(missing.is_empty());
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn selector_prefix_rejects_ambiguous_and_invalid_prefixes() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());
    let first = test_session_with_id(
        "12345678-1234-4234-9234-123456789abc",
        "engineer",
        "/tmp/first",
    );
    let second = test_session_with_id("12345679-1234-4234-9234-123456789abc", "pm", "/tmp/second");
    for session in [&first, &second] {
        store
            .insert_session(session)
            .await
            .or_panic("session inserts");
    }

    let ambiguous = store
        .list_sessions_by_selector(&Selector::Prefix {
            prefix: "1234567".to_string(),
        })
        .await
        .err_or_panic("ambiguous prefix fails");
    match ambiguous {
        SessionRowError::Ambiguous { prefix, candidates } => {
            assert_eq!(prefix, "1234567");
            assert_eq!(
                candidates,
                vec![first.id.to_string(), second.id.to_string()]
            );
        }
        other => panic!("unexpected error: {other:?}"),
    }

    let too_short = store
        .list_sessions_by_selector(&Selector::Prefix {
            prefix: "123".to_string(),
        })
        .await
        .err_or_panic("short prefix fails");
    assert!(matches!(too_short, SessionRowError::PrefixTooShort { .. }));

    let invalid = store
        .list_sessions_by_selector(&Selector::Prefix {
            prefix: "%".repeat(4),
        })
        .await
        .err_or_panic("wildcard prefix fails");
    assert!(matches!(invalid, SessionRowError::InvalidPrefix { .. }));
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn persists_sessions_across_store_handles() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let session = test_session("general", "test", Vec::new());
    {
        let store = SessionStore::from_db(testdb.db());
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");
    }

    let store = SessionStore::from_db(testdb.db());
    let sessions = store.list_sessions(None).await.or_panic("sessions list");

    assert_eq!(sessions, vec![session]);
    testdb.cleanup().await.or_panic("test db cleans up");
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn round_trips_lost_sessions_for_every_evidence_variant() {
    let testdb = TestDb::create().await.or_panic("test db creates");
    let store = SessionStore::from_db(testdb.db());

    for evidence in LOST_EVIDENCE_VARIANTS {
        let mut session = test_session("general", "test", Vec::new());
        session.state = SessionState::Lost { evidence };
        store
            .insert_session(&session)
            .await
            .or_panic("session inserts");

        let loaded = store
            .get_session(&session.id)
            .await
            .or_panic("session loads")
            .or_panic("session exists");
        assert_eq!(loaded.state, SessionState::Lost { evidence });
    }

    testdb.cleanup().await.or_panic("test db cleans up");
}

fn test_session(role: &str, workspace: &str, labels: Vec<Label>) -> Session {
    let now = now_micros();
    Session {
        id: SessionId::from_uuid(uuid::Uuid::now_v7()),
        runtime: RuntimeKind::Claude,
        role: role.to_string(),
        workspace: workspace.to_string(),
        namespace: Namespace::default(),
        dir: PathBuf::from(workspace),
        state: SessionState::Running,
        runtime_pid: 42,
        runtime_session: None,
        transcript_path: None,
        tmux_pane: None,
        agent_config: None,
        created_at: now,
        started_at: now,
        terminated_at: None,
        exit_code: None,
        updated_at: now,
        labels,
    }
}

fn test_session_with_id(id: &str, role: &str, workspace: &str) -> Session {
    let mut session = test_session(role, workspace, Vec::new());
    session.id = SessionId::from_uuid(uuid::Uuid::parse_str(id).or_panic("uuid parses"));
    session
}

fn session_ids(sessions: &[Session]) -> Vec<SessionId> {
    sessions.iter().map(|session| session.id).collect()
}
