use std::path::PathBuf;

use chrono::Utc;
use lilo_session_core::{DEFAULT_NAMESPACE, Label, LabelOp, Namespace, Selector};
use rusqlite::params;

use crate::test_support::OrPanic as _;

use super::*;

#[test]
fn inserts_and_lists_sessions() {
    let store = SqliteStore::open_in_memory().or_panic("store opens");
    let now = Utc::now();
    let session = Session {
        id: Uuid::now_v7(),
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

    store.insert_session(&session).or_panic("session inserts");

    assert_eq!(
        store.list_sessions(None).or_panic("sessions list"),
        vec![session]
    );
}

#[test]
fn marks_session_terminated() {
    let store = SqliteStore::open_in_memory().or_panic("store opens");
    let session = test_session("general", "test", Vec::new());
    store.insert_session(&session).or_panic("session inserts");

    let terminated_at = Utc::now();
    let terminated = store
        .mark_session_terminated(&session.id, Some(137), terminated_at)
        .or_panic("session updates")
        .or_panic("session exists");

    assert_eq!(terminated.state, SessionState::Terminated);
    assert_eq!(terminated.exit_code, Some(137));
    assert_eq!(terminated.terminated_at, Some(terminated_at));
}

#[test]
fn records_transcript_path_without_runtime_session() {
    let store = SqliteStore::open_in_memory().or_panic("store opens");
    let session = test_session("engineer", "test", Vec::new());
    store.insert_session(&session).or_panic("session inserts");
    let transcript = std::path::Path::new("/tmp/rtmd-stdout.log");

    let recorded_at = Utc::now();
    let updated = store
        .record_transcript_path(&session.id, transcript, recorded_at)
        .or_panic("transcript records")
        .or_panic("session exists");

    assert_eq!(updated.runtime_session, None);
    assert_eq!(updated.transcript_path.as_deref(), Some(transcript));
    assert_eq!(updated.updated_at, recorded_at);

    let later = recorded_at + chrono::Duration::seconds(30);
    let unchanged = store
        .record_transcript_path(&session.id, transcript, later)
        .or_panic("transcript no-ops")
        .or_panic("session exists");

    assert_eq!(unchanged.updated_at, recorded_at);
}

#[test]
fn selector_queries_return_sessions_with_labels() {
    let store = SqliteStore::open_in_memory().or_panic("store opens");
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
        store.insert_session(session).or_panic("session inserts");
    }

    let engineers = store
        .list_sessions_by_selector(&Selector::Role {
            name: "engineer".to_string(),
        })
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
        .or_panic("label in selector resolves");
    assert_eq!(
        session_ids(&in_area),
        vec![auth_pm.id, auth_engineer.id, ui_engineer.id]
    );
}

#[test]
fn selector_queries_filter_by_namespace_dir_and_scope() {
    let store = SqliteStore::open_in_memory().or_panic("store opens");
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
            .or_panic("namespace creates");
    }
    for session in [&alpha_engineer, &alpha_pm, &beta_engineer] {
        store.insert_session(session).or_panic("session inserts");
    }

    let alpha_sessions = store
        .list_sessions_by_selector(&Selector::Namespace { namespace: alpha })
        .or_panic("namespace selector resolves");
    assert_eq!(
        session_ids(&alpha_sessions),
        vec![alpha_engineer.id, alpha_pm.id]
    );

    let beta_dir_sessions = store
        .list_sessions_by_selector(&Selector::Dir {
            path: PathBuf::from("/tmp/beta"),
        })
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
        .or_panic("scoped selector resolves");
    assert_eq!(session_ids(&scoped_engineers), vec![beta_engineer.id]);
}

#[test]
fn migrates_pass1_schema() {
    let path = std::env::temp_dir().join(format!("lilo-session-store-{}.db", Uuid::now_v7()));
    let created_at = Utc::now().to_rfc3339();
    {
        let connection = rusqlite::Connection::open(&path).or_panic("v0 database opens");
        connection
            .execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY NOT NULL,
                    runtime TEXT NOT NULL,
                    role TEXT NOT NULL,
                    workspace TEXT NOT NULL,
                    state TEXT NOT NULL,
                    runtime_pid INTEGER NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )
            .or_panic("v0 schema creates");
        connection
            .execute(
                "INSERT INTO sessions
                    (id, runtime, role, workspace, state, runtime_pid, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    Uuid::now_v7().to_string(),
                    RuntimeKind::Claude.to_string(),
                    "general",
                    "test",
                    SessionState::Running.to_string(),
                    42,
                    created_at,
                    created_at,
                ],
            )
            .or_panic("v0 row inserts");
    }

    let store = SqliteStore::open(&path).or_panic("store migrates");
    let sessions = store.list_sessions(None).or_panic("sessions list");

    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].started_at, sessions[0].created_at);
    assert_eq!(sessions[0].terminated_at, None);
    assert_eq!(sessions[0].exit_code, None);
    assert_eq!(sessions[0].runtime_session, None);
    assert_eq!(sessions[0].transcript_path, None);
    assert_eq!(sessions[0].agent_config, None);
    assert_eq!(
        store.list_namespaces().or_panic("namespaces list")[0].namespace,
        Namespace::default()
    );
    let session_namespace = store
        .get_session_namespace(&sessions[0].id)
        .or_panic("session namespace loads")
        .or_panic("session namespace exists");
    assert_eq!(session_namespace.namespace.as_str(), DEFAULT_NAMESPACE);
    assert_eq!(session_namespace.dir.to_string_lossy(), "test");
    let _ = std::fs::remove_file(path);
}

fn test_session(role: &str, workspace: &str, labels: Vec<Label>) -> Session {
    let now = Utc::now();
    Session {
        id: Uuid::now_v7(),
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

fn session_ids(sessions: &[Session]) -> Vec<Uuid> {
    sessions.iter().map(|session| session.id).collect()
}
