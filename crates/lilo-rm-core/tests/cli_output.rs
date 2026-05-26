mod support;

use lilo_rm_core::{
    CliOutput, Lifecycle, LogAvailability, LogsUnavailableReason, LostEvidence, RuntimeKind,
    TmuxAddress,
};
use support::{ready, session_id};

#[test]
fn human_status_renders_tmux_log_availability() {
    let mut lifecycle = Lifecycle::forking(session_id(), RuntimeKind::Claude);
    lifecycle.mark_running(ready(session_id()));
    lifecycle.log_availability = Some(LogAvailability::Unavailable {
        reason: LogsUnavailableReason::PaneUnavailable,
    });

    let mut output = String::new();
    vec![lifecycle].render_human(&mut output).expect("render");

    assert!(output.contains("tmux_pane=rtm:0.1"), "{output}");
    assert!(
        output.contains("log_availability=unavailable:pane_unavailable"),
        "{output}"
    );
}

#[test]
fn human_status_renders_terminal_tmux_lifecycle_state() {
    let mut lifecycle = Lifecycle::forking(session_id(), RuntimeKind::Claude);
    lifecycle.mark_running(ready(session_id()));
    lifecycle.mark_lost(LostEvidence::ShimDiedBeforeReport);
    lifecycle.tmux_pane = Some(TmuxAddress {
        session: "rtm".to_owned(),
        window: 0,
        pane: 1,
    });
    lifecycle.log_availability = Some(LogAvailability::TmuxPaneSnapshot);

    let mut output = String::new();
    vec![lifecycle].render_human(&mut output).expect("render");

    assert!(
        output.contains("state=Lost(ShimDiedBeforeReport)"),
        "{output}"
    );
    assert!(
        output.contains("log_availability=tmux_pane_snapshot"),
        "{output}"
    );
}
