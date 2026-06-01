use lilo_session_core::{Label, MailCountView, MailSendResult, MessageView, SenderView, Session};

pub fn print_session_line(session: &Session, show_labels: bool) {
    print!(
        "{} {} {} {} {} {} {} {}",
        session.id,
        session.runtime,
        session.role,
        session.namespace,
        session.dir.display(),
        session.state,
        session.runtime_pid,
        session.tmux_pane.as_deref().unwrap_or("-"),
    );
    if show_labels {
        print!(" {}", format_labels(&session.labels));
    }
    println!();
}

pub fn print_session_table(sessions: &[Session], show_labels: bool) {
    if show_labels {
        println!("ID RUNTIME ROLE NAMESPACE DIR STATE PID TMUX LABELS");
    } else {
        println!("ID RUNTIME ROLE NAMESPACE DIR STATE PID TMUX");
    }
    for session in sessions {
        print_session_line(session, show_labels);
    }
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

pub fn print_message_table(messages: &[MessageView]) {
    if messages.is_empty() {
        return;
    }
    println!("SENT_AT SENDER RECIPIENT STATUS INTENT CONTENT");
    for item in messages {
        println!(
            "{} {} {} {} {} {}",
            item.sent_at.to_rfc3339(),
            sender_display_label(&item.sender),
            item.recipient.display_label,
            item.status,
            item.intent,
            item.content
        );
    }
}

pub fn print_mail_send_summary(results: &[MailSendResult]) {
    if results.is_empty() {
        println!("No recipients matched.");
        return;
    }
    println!("RECIPIENT MAIL NOTIFY CONTEXT INTENT");
    for result in results {
        let message = result.message.as_ref();
        let intent = message.map_or_else(|| "-".to_string(), |message| message.intent.to_string());
        println!(
            "{} {} {} {} {}",
            result.recipient.display_label,
            result.mail,
            result.notify,
            message.map_or("-", |message| message.context_id.as_str()),
            intent,
        );
    }
}

pub fn print_mail_counts(total: usize, counts: &[MailCountView]) {
    println!("{total} unread total");
    if counts.is_empty() {
        return;
    }
    println!("MAILBOX UNREAD");
    for count in counts {
        println!("{} {}", count.display_label, count.unread);
    }
}

fn sender_display_label(sender: &SenderView) -> &str {
    match sender {
        SenderView::Session { display_label, .. } | SenderView::Operator { display_label, .. } => {
            display_label
        }
        SenderView::System => "system",
    }
}
