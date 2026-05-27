use lilo_session_core::{Label, Mail, Session};

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

pub fn print_mail(mail: &[Mail]) {
    for item in mail {
        println!(
            "{} {} {} {} {}",
            item.id,
            item.sender_id,
            item.recipient_id,
            item.status(),
            item.content
        );
    }
}
