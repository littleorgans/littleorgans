use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, SystemTime};

use lilo_common::id::{MessageId, SessionId};
use lilo_session_core::{Label, MailSendResult, MessageView, SenderView, Session};

const CONTENT_PREVIEW_MAX_CHARS: usize = 120;
const PREVIEW_ELLIPSIS: &str = "...";
const MESSAGE_HEADERS: [&str; 7] = [
    "SENDER",
    "RECIPIENT",
    "RECIPIENT-ID",
    "CONTEXT",
    "INTENT",
    "STATUS",
    "AGE",
];

#[derive(Debug, Clone)]
pub struct ShortSessionIdSet {
    full_ids: Vec<String>,
}

impl ShortSessionIdSet {
    #[must_use]
    pub fn from_sessions(sessions: &[Session]) -> Self {
        Self {
            full_ids: sessions
                .iter()
                .map(|session| session.id.to_string())
                .collect(),
        }
    }

    #[must_use]
    pub fn render(&self, id: &SessionId) -> String {
        let full_id = id.to_string();
        if !self.full_ids.iter().any(|candidate| candidate == &full_id) {
            return id.short();
        }
        id.short_with(|prefix| self.match_count(prefix) == 1)
    }

    fn match_count(&self, prefix: &str) -> usize {
        self.full_ids
            .iter()
            .filter(|candidate| candidate.starts_with(prefix))
            .count()
    }
}

#[derive(Debug, Clone)]
pub struct MessageTableStream {
    short_ids: ShortSessionIdSet,
    widths: Option<Vec<usize>>,
}

impl MessageTableStream {
    #[must_use]
    pub fn new(short_ids: ShortSessionIdSet) -> Self {
        Self {
            short_ids,
            widths: None,
        }
    }

    pub fn print(&mut self, messages: &[MessageView]) {
        print!("{}", self.render(messages));
    }

    fn render(&mut self, messages: &[MessageView]) -> String {
        if messages.is_empty() {
            return String::new();
        }
        let rows = message_rows_short_ids(messages, &self.short_ids);
        let table_rows = rows.iter().map(|row| row.0.clone()).collect::<Vec<_>>();
        let include_header = self.widths.is_none();
        let widths = self
            .widths
            .get_or_insert_with(|| column_widths(&MESSAGE_HEADERS, &table_rows));
        render_table_rows_with_details(&rows, widths, include_header)
    }
}

pub fn print_session_line(session: &Session, show_labels: bool) {
    println!("{}", session_cells(session, show_labels).join(" "));
}

pub fn print_session_line_short_id(
    session: &Session,
    show_labels: bool,
    short_ids: &ShortSessionIdSet,
) {
    println!(
        "{}",
        session_cells_short_id(session, show_labels, short_ids).join(" ")
    );
}

pub fn print_session_table(sessions: &[Session], show_labels: bool) {
    print_session_table_with_rows(sessions, show_labels, session_cells);
}

pub fn print_session_table_short_ids(
    sessions: &[Session],
    show_labels: bool,
    short_ids: &ShortSessionIdSet,
) {
    print_session_table_with_rows(sessions, show_labels, |session, show_labels| {
        session_cells_short_id(session, show_labels, short_ids)
    });
}

fn print_session_table_with_rows<F>(sessions: &[Session], show_labels: bool, row: F)
where
    F: Fn(&Session, bool) -> Vec<String>,
{
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
        .map(|session| row(session, show_labels))
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

pub fn print_messages_short_ids(messages: &[MessageView], short_ids: &ShortSessionIdSet) {
    print_message_table_short_ids(messages, short_ids);
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
    print_message_table_with_rows(messages, message_row);
}

pub fn print_message_table_short_ids(messages: &[MessageView], short_ids: &ShortSessionIdSet) {
    print_message_table_with_rows(messages, |message| message_row_short_id(message, short_ids));
}

fn print_message_table_with_rows<F>(messages: &[MessageView], row: F)
where
    F: Fn(&MessageView) -> (Vec<String>, String),
{
    if messages.is_empty() {
        return;
    }
    let rows = messages.iter().map(row).collect::<Vec<_>>();
    print_table_with_details(&MESSAGE_HEADERS, &rows);
}

pub fn print_mail_send_summary(results: &[MailSendResult]) {
    print_mail_send_summary_with_ids(results, None);
}

pub fn print_mail_send_summary_short_ids(
    results: &[MailSendResult],
    short_ids: &ShortSessionIdSet,
) {
    print_mail_send_summary_with_ids(results, Some(short_ids));
}

fn print_mail_send_summary_with_ids(
    results: &[MailSendResult],
    short_ids: Option<&ShortSessionIdSet>,
) {
    if results.is_empty() {
        println!("No recipients matched.");
        return;
    }
    let include_error = results.iter().any(|result| result.error.is_some());
    let headers = if include_error {
        vec![
            "RECIPIENT-ID",
            "ROLE",
            "CONTEXT",
            "INTENT",
            "NOTIFY",
            "MAIL",
            "ERROR",
        ]
    } else {
        vec![
            "RECIPIENT-ID",
            "ROLE",
            "CONTEXT",
            "INTENT",
            "NOTIFY",
            "MAIL",
        ]
    };
    let rows = results
        .iter()
        .map(|result| mail_send_cells(result, include_error, short_ids))
        .collect::<Vec<_>>();
    print_table(&headers, &rows);
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
    session_cells_with_id(session, show_labels, session.id.to_string())
}

fn session_cells_short_id(
    session: &Session,
    show_labels: bool,
    short_ids: &ShortSessionIdSet,
) -> Vec<String> {
    session_cells_with_id(session, show_labels, short_ids.render(&session.id))
}

fn session_cells_with_id(session: &Session, show_labels: bool, id: String) -> Vec<String> {
    let mut cells = vec![
        id,
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
    message_row_with_recipient_id(item, item.recipient.session_id.to_string())
}

fn message_row_short_id(
    item: &MessageView,
    short_ids: &ShortSessionIdSet,
) -> (Vec<String>, String) {
    message_row_with_recipient_id(item, short_ids.render(&item.recipient.session_id))
}

fn message_rows_short_ids(
    messages: &[MessageView],
    short_ids: &ShortSessionIdSet,
) -> Vec<(Vec<String>, String)> {
    messages
        .iter()
        .map(|message| message_row_short_id(message, short_ids))
        .collect()
}

fn message_row_with_recipient_id(
    item: &MessageView,
    recipient_id: String,
) -> (Vec<String>, String) {
    let cells = vec![
        sender_display_label(&item.sender).to_string(),
        item.recipient.display_label.clone(),
        recipient_id,
        item.context_id.clone(),
        item.intent.to_string(),
        item.status.to_string(),
        format_age(item.sent_at.into()),
    ];
    (cells, content_preview(&item.content))
}

fn mail_send_cells(
    result: &MailSendResult,
    include_error: bool,
    short_ids: Option<&ShortSessionIdSet>,
) -> Vec<String> {
    let message = result.message.as_ref();
    let recipient_id = short_ids.map_or_else(
        || result.recipient.session_id.to_string(),
        |short_ids| short_ids.render(&result.recipient.session_id),
    );
    let mut cells = vec![
        recipient_id,
        result.recipient.role.clone(),
        message
            .map_or("-", |message| message.context_id.as_str())
            .to_string(),
        message.map_or_else(|| "-".to_string(), |message| message.intent.to_string()),
        result.notify.to_string(),
        result.mail.to_string(),
    ];
    if include_error {
        cells.push(result.error.as_deref().unwrap_or("-").to_string());
    }
    cells
}

struct ConversationSummary<'a> {
    context_id: &'a str,
    message_ids: BTreeSet<MessageId>,
    recipient_ids: BTreeSet<SessionId>,
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
    append_table_header(&mut output, headers, &widths);
    append_table_rows_with_details(&mut output, rows, &widths);
    output
}

fn render_table_rows_with_details(
    rows: &[(Vec<String>, String)],
    widths: &[usize],
    include_header: bool,
) -> String {
    let mut output = String::new();
    if include_header {
        append_table_header(&mut output, &MESSAGE_HEADERS, widths);
    }
    append_table_rows_with_details(&mut output, rows, widths);
    output
}

fn append_table_header(output: &mut String, headers: &[&str], widths: &[usize]) {
    append_table_row(
        output,
        &headers
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
        widths,
    );
}

fn append_table_rows_with_details(
    output: &mut String,
    rows: &[(Vec<String>, String)],
    widths: &[usize],
) {
    for (cells, detail) in rows {
        append_table_row(output, cells, widths);
        append_detail_line(output, detail);
    }
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
mod tests;
