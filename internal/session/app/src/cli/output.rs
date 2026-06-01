use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use lilo_session_core::{Label, MailCountView, MailSendResult, MessageView, SenderView, Session};
use uuid::Uuid;

const CONTENT_PREVIEW_MAX_CHARS: usize = 120;
const PREVIEW_ELLIPSIS: &str = "...";

pub fn print_session_line(session: &Session, show_labels: bool) {
    println!("{}", session_cells(session, show_labels).join(" "));
}

pub fn print_session_table(sessions: &[Session], show_labels: bool) {
    let headers = if show_labels {
        vec![
            "ID",
            "RUNTIME",
            "NAMESPACE",
            "ROLE",
            "TMUX",
            "STATUS",
            "AGE",
            "LABELS",
        ]
    } else {
        vec![
            "ID",
            "RUNTIME",
            "NAMESPACE",
            "ROLE",
            "TMUX",
            "STATUS",
            "AGE",
        ]
    };
    let rows = sessions
        .iter()
        .map(|session| session_cells(session, show_labels))
        .collect::<Vec<_>>();
    print_table(&headers, &rows);
}

fn format_labels(labels: &[Label]) -> String {
    if labels.is_empty() {
        return "-".to_string();
    }

    labels
        .iter()
        .map(|label| format!("{}={}", label.key, label.value))
        .collect::<Vec<_>>()
        .join(",")
}

pub fn print_messages(messages: &[MessageView]) {
    print_message_table(messages);
}

pub fn print_conversation_overview(messages: &[MessageView]) {
    if messages.is_empty() {
        println!("No conversations.");
        return;
    }
    let rows = conversation_summaries(messages)
        .iter()
        .map(conversation_cells)
        .collect::<Vec<_>>();
    print_table(
        &[
            "CONTEXT",
            "MESSAGES",
            "RECIPIENTS",
            "UPDATED",
            "INTENT",
            "LAST_CONTENT",
        ],
        &rows,
    );
}

pub fn print_message_table(messages: &[MessageView]) {
    if messages.is_empty() {
        return;
    }
    let rows = messages.iter().map(message_row).collect::<Vec<_>>();
    print_table_with_details(
        &["SENDER", "RECIPIENT", "CONTEXT", "INTENT", "STATUS", "AGE"],
        &rows,
    );
}

pub fn print_mail_send_summary(results: &[MailSendResult]) {
    if results.is_empty() {
        println!("No recipients matched.");
        return;
    }
    let include_error = results.iter().any(|result| result.error.is_some());
    let headers = if include_error {
        vec!["RECIPIENT", "MAIL", "NOTIFY", "CONTEXT", "INTENT", "ERROR"]
    } else {
        vec!["RECIPIENT", "MAIL", "NOTIFY", "CONTEXT", "INTENT"]
    };
    let rows = results
        .iter()
        .map(|result| mail_send_cells(result, include_error))
        .collect::<Vec<_>>();
    print_table(&headers, &rows);
}

pub fn print_mail_counts(total: usize, counts: &[MailCountView]) {
    println!("{total} unread total");
    if counts.is_empty() {
        return;
    }
    let rows = counts.iter().map(mail_count_cells).collect::<Vec<_>>();
    print_table(&["MAILBOX", "UNREAD"], &rows);
}

fn sender_display_label(sender: &SenderView) -> &str {
    match sender {
        SenderView::Session { display_label, .. } | SenderView::Operator { display_label, .. } => {
            display_label
        }
        SenderView::System => "system",
    }
}

fn session_cells(session: &Session, show_labels: bool) -> Vec<String> {
    let mut cells = vec![
        session.id.to_string(),
        session.runtime.to_string(),
        session.namespace.to_string(),
        session.role.clone(),
        session.tmux_pane.as_deref().unwrap_or("-").to_string(),
        session.state.to_string(),
        format_age(session.created_at.into()),
    ];
    if show_labels {
        cells.push(format_labels(&session.labels));
    }
    cells
}

fn message_row(item: &MessageView) -> (Vec<String>, String) {
    let cells = vec![
        sender_display_label(&item.sender).to_string(),
        item.recipient.display_label.clone(),
        item.context_id.clone(),
        item.intent.to_string(),
        item.status.to_string(),
        format_age(item.sent_at.into()),
    ];
    (cells, content_preview(&item.content))
}

fn mail_send_cells(result: &MailSendResult, include_error: bool) -> Vec<String> {
    let message = result.message.as_ref();
    let mut cells = vec![
        result.recipient.display_label.clone(),
        result.mail.to_string(),
        result.notify.to_string(),
        message
            .map_or("-", |message| message.context_id.as_str())
            .to_string(),
        message.map_or_else(|| "-".to_string(), |message| message.intent.to_string()),
    ];
    if include_error {
        cells.push(result.error.as_deref().unwrap_or("-").to_string());
    }
    cells
}

struct ConversationSummary<'a> {
    context_id: &'a str,
    message_ids: BTreeSet<Uuid>,
    recipient_ids: BTreeSet<Uuid>,
    latest: Option<&'a MessageView>,
}

impl<'a> ConversationSummary<'a> {
    fn new(context_id: &'a str) -> Self {
        Self {
            context_id,
            message_ids: BTreeSet::new(),
            recipient_ids: BTreeSet::new(),
            latest: None,
        }
    }

    fn observe(&mut self, message: &'a MessageView) {
        self.message_ids.insert(message.id);
        self.recipient_ids.insert(message.recipient.session_id);
        if self
            .latest
            .is_none_or(|latest| message_is_newer(message, latest))
        {
            self.latest = Some(message);
        }
    }
}

fn conversation_summaries(messages: &[MessageView]) -> Vec<ConversationSummary<'_>> {
    let mut by_context = BTreeMap::new();
    for message in messages {
        by_context
            .entry(message.context_id.as_str())
            .or_insert_with(|| ConversationSummary::new(&message.context_id))
            .observe(message);
    }
    let mut summaries = by_context.into_values().collect::<Vec<_>>();
    summaries.sort_by(|left, right| {
        let left = left.latest.expect("summary has at least one message");
        let right = right.latest.expect("summary has at least one message");
        right
            .sent_at
            .cmp(&left.sent_at)
            .then_with(|| right.id.cmp(&left.id))
    });
    summaries
}

fn conversation_cells(summary: &ConversationSummary<'_>) -> Vec<String> {
    let latest = summary.latest.expect("summary has at least one message");
    vec![
        summary.context_id.to_string(),
        summary.message_ids.len().to_string(),
        summary.recipient_ids.len().to_string(),
        format_age(latest.sent_at.into()),
        latest.intent.to_string(),
        content_preview(&latest.content),
    ]
}

fn message_is_newer(candidate: &MessageView, latest: &MessageView) -> bool {
    candidate.sent_at > latest.sent_at
        || (candidate.sent_at == latest.sent_at && candidate.id > latest.id)
}

fn mail_count_cells(count: &MailCountView) -> Vec<String> {
    vec![count.display_label.clone(), count.unread.to_string()]
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    print!("{}", render_table(headers, rows));
}

fn print_table_with_details(headers: &[&str], rows: &[(Vec<String>, String)]) {
    print!("{}", render_table_with_details(headers, rows));
}

fn render_table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let widths = column_widths(headers, rows);
    let mut output = String::new();
    append_table_row(
        &mut output,
        &headers
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
        &widths,
    );
    for row in rows {
        append_table_row(&mut output, row, &widths);
    }
    output
}

fn render_table_with_details(headers: &[&str], rows: &[(Vec<String>, String)]) -> String {
    let table_rows = rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>();
    let widths = column_widths(headers, &table_rows);
    let mut output = String::new();
    append_table_row(
        &mut output,
        &headers
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
        &widths,
    );
    for (cells, detail) in rows {
        append_table_row(&mut output, cells, &widths);
        append_detail_line(&mut output, detail);
    }
    output
}

fn append_detail_line(output: &mut String, detail: &str) {
    output.push_str("  ");
    output.push_str(detail);
    output.push('\n');
}

fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths = headers
        .iter()
        .map(|header| display_width(header))
        .collect::<Vec<_>>();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(cell));
        }
    }
    widths
}

fn append_table_row(output: &mut String, cells: &[String], widths: &[usize]) {
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            output.push_str("  ");
        }
        let cell = cells.get(index).map_or("", String::as_str);
        output.push_str(cell);
        if index + 1 < widths.len() {
            for _ in display_width(cell)..*width {
                output.push(' ');
            }
        }
    }
    output.push('\n');
}

fn display_width(value: &str) -> usize {
    value.chars().count()
}

fn format_age(created_at: SystemTime) -> String {
    let elapsed = SystemTime::now()
        .duration_since(created_at)
        .unwrap_or_else(|_| Duration::from_secs(0));
    format_duration_age(elapsed)
}

fn format_duration_age(duration: Duration) -> String {
    let seconds = duration.as_secs();
    match seconds {
        0..=59 => format!("{seconds}s"),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

fn content_preview(content: &str) -> String {
    let normalized = content.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "-".to_string();
    }
    truncate_preview(&normalized, CONTENT_PREVIEW_MAX_CHARS)
}

fn truncate_preview(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let retained = max_chars.saturating_sub(PREVIEW_ELLIPSIS.chars().count());
    let mut preview = content.chars().take(retained).collect::<String>();
    preview.push_str(PREVIEW_ELLIPSIS);
    preview
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{format_duration_age, render_table};

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
    fn format_duration_age_uses_compact_resource_units() {
        assert_eq!(format_duration_age(Duration::from_secs(0)), "0s");
        assert_eq!(format_duration_age(Duration::from_secs(59)), "59s");
        assert_eq!(format_duration_age(Duration::from_mins(1)), "1m");
        assert_eq!(format_duration_age(Duration::from_hours(1)), "1h");
        assert_eq!(format_duration_age(Duration::from_hours(24)), "1d");
    }
}
