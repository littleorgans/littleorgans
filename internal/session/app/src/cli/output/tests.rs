use std::path::PathBuf;
use std::time::Duration;

use chrono::Utc;
use lilo_common::id::{MessageId, SessionId};
use lilo_session_core::{
    MailDeliveryStatus, MailIntent, MailNotifyStatus, MailSendResult, MailStatus, MessageView,
    Namespace, RecipientSummary, RuntimeKind, SenderView, Session, SessionState,
};

use super::{MessageTableStream, ShortSessionIdSet, format_duration_age, render_table};

#[test]
fn render_table_aligns_columns_and_preserves_last_column_text() {
    let rows = vec![
        vec![
            "pm".to_string(),
            "ok".to_string(),
            "skipped".to_string(),
            "what are you saying?".to_string(),
        ],
        vec![
            "reviewer".to_string(),
            "err".to_string(),
            "skipped".to_string(),
            "mail denied".to_string(),
        ],
    ];

    assert_eq!(
        render_table(&["RECIPIENT", "MAIL", "NOTIFY", "CONTENT"], &rows),
        concat!(
            "RECIPIENT  MAIL  NOTIFY   CONTENT\n",
            "pm         ok    skipped  what are you saying?\n",
            "reviewer   err   skipped  mail denied\n",
        )
    );
}

#[test]
fn message_row_places_recipient_session_id_after_recipient() {
    let recipient_id = SessionId::from_uuid(uuid::Uuid::from_u128(1));
    let view = test_message(recipient_id, "pm", "testing");

    let (cells, _detail) = super::message_row(&view);

    // Columns: SENDER, RECIPIENT, RECIPIENT-ID, CONTEXT, INTENT, STATUS, AGE
    assert_eq!(cells[0], "system");
    assert_eq!(cells[1], "pm");
    assert_eq!(cells[2], recipient_id.to_string());
    assert_eq!(cells[3], "testing");
    assert_eq!(cells[4], "inform");
    assert_eq!(cells[5], "unread");
    assert_eq!(cells.len(), 7);
}

#[test]
fn mail_send_summary_cells_use_short_recipient_id_order() {
    let recipient = test_session("12345678-1234-4234-9234-123456789abc");
    let short_ids = ShortSessionIdSet::from_sessions(std::slice::from_ref(&recipient));
    let message = test_message(recipient.id, "engineer", "handoff");
    let result = MailSendResult {
        recipient: message.recipient.clone(),
        mail: MailDeliveryStatus::Ok,
        notify: MailNotifyStatus::Skipped,
        message: Some(message),
        error: None,
    };

    let rows = vec![super::mail_send_cells(&result, false, Some(&short_ids))];

    assert_eq!(
        render_table(
            &[
                "RECIPIENT-ID",
                "ROLE",
                "CONTEXT",
                "INTENT",
                "NOTIFY",
                "MAIL"
            ],
            &rows,
        ),
        concat!(
            "RECIPIENT-ID  ROLE      CONTEXT  INTENT  NOTIFY   MAIL\n",
            "1234567       engineer  handoff  inform  skipped  ok\n",
        )
    );
}

#[test]
fn message_table_stream_freezes_header_widths_and_short_id_set() {
    let known = test_session("12345678-1234-4234-9234-123456789abc");
    let late_id = SessionId::from_uuid(
        uuid::Uuid::parse_str("89abcdef-1234-4234-9234-123456789abc").expect("uuid parses"),
    );
    let mut stream = MessageTableStream::new(ShortSessionIdSet::from_sessions(
        std::slice::from_ref(&known),
    ));

    let first = stream.render(&[test_message(known.id, "engineer", "ctx-1")]);
    let second = stream.render(&[test_message(late_id, "late", "ctx-2")]);
    let output = format!("{first}{second}");
    let lines = output.lines().collect::<Vec<_>>();
    let context_column = lines[0].find("CONTEXT").expect("header has CONTEXT");

    assert_eq!(output.matches("SENDER").count(), 1);
    assert!(lines[3].contains(&late_id.short()));
    assert!(!lines[3].contains(&late_id.to_string()));
    assert_eq!(lines[3].find("ctx-2"), Some(context_column));
}

#[test]
fn short_session_ids_widen_past_forced_collision() {
    let first = test_session("12345678-1234-4234-9234-123456789abc");
    let second = test_session("12345679-1234-4234-9234-123456789abc");
    let short_ids = ShortSessionIdSet::from_sessions(&[first.clone(), second.clone()]);

    assert_eq!(short_ids.render(&first.id), "12345678");
    assert_eq!(short_ids.render(&second.id), "12345679");
}

#[test]
fn short_session_ids_use_bounded_short_id_for_non_members() {
    let known = test_session("12345678-1234-4234-9234-123456789abc");
    let late_id = SessionId::from_uuid(
        uuid::Uuid::parse_str("12345679-1234-4234-9234-123456789abc").expect("uuid parses"),
    );
    let short_ids = ShortSessionIdSet::from_sessions(&[known]);

    assert_eq!(short_ids.render(&late_id), late_id.short());
}

#[test]
fn format_duration_age_uses_compact_resource_units() {
    assert_eq!(format_duration_age(Duration::from_secs(0)), "0s");
    assert_eq!(format_duration_age(Duration::from_secs(59)), "59s");
    assert_eq!(format_duration_age(Duration::from_mins(1)), "1m");
    assert_eq!(format_duration_age(Duration::from_hours(1)), "1h");
    assert_eq!(format_duration_age(Duration::from_hours(24)), "1d");
}

fn test_session(id: &str) -> Session {
    let now = Utc::now();
    Session {
        id: lilo_common::id::SessionId::from_uuid(uuid::Uuid::parse_str(id).expect("uuid parses")),
        runtime: RuntimeKind::Claude,
        role: "engineer".to_string(),
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
    }
}

fn test_message(recipient_id: SessionId, role: &str, context_id: &str) -> MessageView {
    MessageView {
        id: MessageId::from_uuid(uuid::Uuid::from_u128(2)),
        content: "what are we working on?".to_string(),
        sent_at: Utc::now(),
        read_at: None,
        status: MailStatus::Unread,
        sender: SenderView::System,
        recipient: RecipientSummary {
            session_id: recipient_id,
            role: role.to_string(),
            display_label: role.to_string(),
            namespace: Namespace::default(),
        },
        context_id: context_id.to_string(),
        intent: MailIntent::Inform,
    }
}
