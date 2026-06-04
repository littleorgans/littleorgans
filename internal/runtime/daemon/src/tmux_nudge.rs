use std::time::{Duration, Instant};

use anyhow::Result;
use lilo_rm_core::{CaptureError, NudgeMode, PaneSnapshot, RuntimeKind, TmuxAddress};

use crate::tmux::{
    TmuxGateway, build_nudge_send_keys_steps, copy_mode_cancel_step, pane_in_mode, send_keys,
};

const WAIT_TIMING: NudgeTiming = NudgeTiming {
    poll_interval: Duration::from_secs(1),
    timeout: Duration::from_mins(2),
    idle_probes_required: 2,
};

const STEER_TIMING: NudgeTiming = NudgeTiming {
    poll_interval: Duration::from_millis(250),
    timeout: Duration::from_secs(5),
    idle_probes_required: 2,
};

/// After sending a nudge payload, confirm the submit actually landed by probing
/// for the busy marker. The submit keystroke can be silently dropped while the
/// agent is in a transitional state (loading MCP on its first message, or
/// just-interrupted by steer), leaving the payload typed but unsubmitted.
/// `SUBMIT_CONFIRM_PROBES` probes spaced `SUBMIT_CONFIRM_POLL` apart give the
/// turn time to surface before we decide the submit was dropped and re-send it.
const SUBMIT_CONFIRM_PROBES: usize = 3;
const SUBMIT_CONFIRM_POLL: Duration = Duration::from_millis(250);
const MAX_SUBMIT_RESENDS: usize = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum NudgeSendOutcome {
    Delivered,
    PaneDead,
    AgentBusyTimeout,
}

#[derive(Clone, Copy)]
struct NudgeTiming {
    poll_interval: Duration,
    timeout: Duration,
    idle_probes_required: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PolicyOutcome {
    Ready,
    BusyTimeout,
}

trait NudgeOps {
    async fn is_alive(&mut self) -> Result<bool>;

    async fn pane_in_mode(&mut self) -> Result<bool>;

    async fn cancel_copy_mode(&mut self) -> Result<()>;

    async fn capture_pane(&mut self) -> std::result::Result<PaneSnapshot, CaptureError>;

    async fn interrupt(&mut self) -> Result<()>;

    async fn send_payload(&mut self) -> Result<()>;

    /// Re-send only the submit keystroke (`Enter`). Used when the payload was
    /// typed into the composer but the submit was dropped; the content is
    /// already buffered, so re-sending it would duplicate it.
    async fn resubmit(&mut self) -> Result<()>;

    async fn sleep(&mut self, duration: Duration);

    fn now(&self) -> Instant;
}

struct RealNudgeOps<'a> {
    server_label: Option<&'a str>,
    tmux_pane: &'a TmuxAddress,
    content: &'a str,
}

impl NudgeOps for RealNudgeOps<'_> {
    async fn is_alive(&mut self) -> Result<bool> {
        TmuxGateway::is_alive(self.server_label, self.tmux_pane).await
    }

    async fn pane_in_mode(&mut self) -> Result<bool> {
        pane_in_mode(self.server_label, self.tmux_pane).await
    }

    async fn cancel_copy_mode(&mut self) -> Result<()> {
        send_keys(self.server_label, self.tmux_pane, &copy_mode_cancel_step()).await
    }

    async fn capture_pane(&mut self) -> std::result::Result<PaneSnapshot, CaptureError> {
        TmuxGateway::capture_pane(self.server_label, self.tmux_pane, Some(20)).await
    }

    async fn interrupt(&mut self) -> Result<()> {
        send_keys(self.server_label, self.tmux_pane, &[String::from("Escape")]).await
    }

    async fn send_payload(&mut self) -> Result<()> {
        for trailing in build_nudge_send_keys_steps(self.content) {
            send_keys(self.server_label, self.tmux_pane, &trailing).await?;
        }
        Ok(())
    }

    async fn resubmit(&mut self) -> Result<()> {
        send_keys(self.server_label, self.tmux_pane, &[String::from("Enter")]).await
    }

    async fn sleep(&mut self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }

    fn now(&self) -> Instant {
        Instant::now()
    }
}

pub(crate) async fn nudge(
    server_label: Option<&str>,
    tmux_pane: &TmuxAddress,
    content: &str,
    mode: NudgeMode,
    timeout_ms: Option<u64>,
    runtime: &RuntimeKind,
) -> Result<NudgeSendOutcome> {
    let mut ops = RealNudgeOps {
        server_label,
        tmux_pane,
        content,
    };
    execute_nudge(&mut ops, mode, timeout_ms, runtime).await
}

async fn execute_nudge(
    ops: &mut impl NudgeOps,
    mode: NudgeMode,
    timeout_ms: Option<u64>,
    runtime: &RuntimeKind,
) -> Result<NudgeSendOutcome> {
    if !ops.is_alive().await? {
        return Ok(NudgeSendOutcome::PaneDead);
    }
    if ops.pane_in_mode().await? {
        let _ = ops.cancel_copy_mode().await;
    }

    let policy = match mode {
        NudgeMode::Immediate => PolicyOutcome::Ready,
        NudgeMode::Wait => wait_for_idle(ops, runtime, wait_timing(timeout_ms)).await?,
        NudgeMode::Steer => steer_if_busy(ops, runtime, STEER_TIMING).await?,
    };
    if policy == PolicyOutcome::BusyTimeout {
        return Ok(NudgeSendOutcome::AgentBusyTimeout);
    }

    send_nudge_payload(ops, runtime).await
}

fn wait_timing(timeout_ms: Option<u64>) -> NudgeTiming {
    let Some(timeout_ms) = timeout_ms else {
        return WAIT_TIMING;
    };
    let timeout = Duration::from_millis(timeout_ms);
    let probe_slots =
        u32::try_from(WAIT_TIMING.idle_probes_required.saturating_add(1)).unwrap_or(u32::MAX);
    let scaled_poll = (timeout / probe_slots).max(Duration::from_nanos(1));
    NudgeTiming {
        poll_interval: WAIT_TIMING.poll_interval.min(scaled_poll),
        timeout,
        idle_probes_required: WAIT_TIMING.idle_probes_required,
    }
}

async fn send_nudge_payload(
    ops: &mut impl NudgeOps,
    runtime: &RuntimeKind,
) -> Result<NudgeSendOutcome> {
    if ops.pane_in_mode().await? {
        let _ = ops.cancel_copy_mode().await;
    }
    if let Some(outcome) = send_or_pane_dead(ops, SendStep::Payload).await? {
        return Ok(outcome);
    }
    // The submit keystroke can be silently dropped while the agent is in a
    // transitional state (loading MCP on its first message, or just-interrupted
    // by steer), leaving the payload typed but unsubmitted. Confirm the submit
    // landed (the agent started a turn) and re-send the submit if it did not.
    let mut resends = 0;
    loop {
        if submission_confirmed(ops, runtime).await {
            return Ok(NudgeSendOutcome::Delivered);
        }
        if resends >= MAX_SUBMIT_RESENDS {
            tracing::warn!(
                resends,
                "nudge payload submit unconfirmed after resends; treating as delivered best-effort"
            );
            return Ok(NudgeSendOutcome::Delivered);
        }
        if let Some(outcome) = send_or_pane_dead(ops, SendStep::Resubmit).await? {
            return Ok(outcome);
        }
        resends += 1;
    }
}

#[derive(Clone, Copy)]
enum SendStep {
    Payload,
    Resubmit,
}

/// Run a send step, mapping a send failure on a now-dead pane to `PaneDead`.
async fn send_or_pane_dead(
    ops: &mut impl NudgeOps,
    step: SendStep,
) -> Result<Option<NudgeSendOutcome>> {
    let result = match step {
        SendStep::Payload => ops.send_payload().await,
        SendStep::Resubmit => ops.resubmit().await,
    };
    match result {
        Ok(()) => Ok(None),
        Err(error) => {
            if !ops.is_alive().await.unwrap_or(false) {
                return Ok(Some(NudgeSendOutcome::PaneDead));
            }
            Err(error)
        }
    }
}

/// Probe whether the agent started a turn after the payload was sent, which is
/// how we know the submit keystroke landed. Wait/Steer deliver only to an idle
/// agent, so a transition to busy is an unambiguous "submit landed" signal. A
/// capture failure degrades to best-effort (assume delivered) rather than
/// re-sending blindly.
async fn submission_confirmed(ops: &mut impl NudgeOps, runtime: &RuntimeKind) -> bool {
    for probe in 0..SUBMIT_CONFIRM_PROBES {
        match probe_agent_busy(ops, runtime).await {
            Ok(false) => {}
            // Busy: the submit landed and started a turn. Capture error: degrade
            // to best-effort and assume delivered rather than re-send blindly.
            Ok(true) | Err(_) => return true,
        }
        if probe + 1 < SUBMIT_CONFIRM_PROBES {
            ops.sleep(SUBMIT_CONFIRM_POLL).await;
        }
    }
    false
}

async fn wait_for_idle(
    ops: &mut impl NudgeOps,
    runtime: &RuntimeKind,
    timing: NudgeTiming,
) -> Result<PolicyOutcome> {
    let deadline = ops.now() + timing.timeout;
    let mut consecutive_idle = 0;

    loop {
        match probe_agent_busy(ops, runtime).await {
            Ok(true) => consecutive_idle = 0,
            Ok(false) => {
                consecutive_idle += 1;
                if consecutive_idle >= timing.idle_probes_required {
                    return Ok(PolicyOutcome::Ready);
                }
            }
            Err(_) => return Ok(PolicyOutcome::Ready),
        }

        if ops.now() >= deadline {
            return Ok(PolicyOutcome::BusyTimeout);
        }
        let sleep_for = timing
            .poll_interval
            .min(deadline.saturating_duration_since(ops.now()));
        ops.sleep(sleep_for).await;
    }
}

async fn steer_if_busy(
    ops: &mut impl NudgeOps,
    runtime: &RuntimeKind,
    timing: NudgeTiming,
) -> Result<PolicyOutcome> {
    match probe_agent_busy(ops, runtime).await {
        Ok(true) => {
            ops.interrupt().await?;
            wait_for_idle(ops, runtime, timing).await
        }
        Ok(false) | Err(_) => Ok(PolicyOutcome::Ready),
    }
}

async fn probe_agent_busy(
    ops: &mut impl NudgeOps,
    runtime: &RuntimeKind,
) -> std::result::Result<bool, CaptureError> {
    let snapshot = ops.capture_pane().await?;
    Ok(crate::tmux_busy::agent_is_busy(runtime, &snapshot.content))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    const BUSY_CODEX: &str = "working esc to interrupt\n";
    const IDLE_CODEX: &str = "ready\n";

    enum CaptureFixture {
        Content(&'static str),
        Error,
    }

    struct FakeOps {
        alive: bool,
        copy_modes: VecDeque<bool>,
        captures: VecDeque<CaptureFixture>,
        default_capture: &'static str,
        actions: Vec<&'static str>,
        now: Instant,
        payload_error_pane_dead: bool,
    }

    impl FakeOps {
        fn new(captures: Vec<CaptureFixture>) -> Self {
            Self {
                alive: true,
                copy_modes: VecDeque::new(),
                captures: captures.into(),
                default_capture: IDLE_CODEX,
                actions: Vec::new(),
                now: Instant::now(),
                payload_error_pane_dead: false,
            }
        }

        fn with_busy_default(mut self) -> Self {
            self.default_capture = BUSY_CODEX;
            self
        }

        fn with_copy_modes(mut self, copy_modes: Vec<bool>) -> Self {
            self.copy_modes = copy_modes.into();
            self
        }

        fn with_payload_error_pane_dead(mut self) -> Self {
            self.payload_error_pane_dead = true;
            self
        }
    }

    impl NudgeOps for FakeOps {
        async fn is_alive(&mut self) -> Result<bool> {
            self.actions.push("alive");
            Ok(self.alive)
        }

        async fn pane_in_mode(&mut self) -> Result<bool> {
            self.actions.push("pane_in_mode");
            Ok(self.copy_modes.pop_front().unwrap_or(false))
        }

        async fn cancel_copy_mode(&mut self) -> Result<()> {
            self.actions.push("cancel_copy_mode");
            Ok(())
        }

        async fn capture_pane(&mut self) -> std::result::Result<PaneSnapshot, CaptureError> {
            self.actions.push("capture");
            match self
                .captures
                .pop_front()
                .unwrap_or(CaptureFixture::Content(self.default_capture))
            {
                CaptureFixture::Content(content) => Ok(PaneSnapshot {
                    content: content.to_owned(),
                    captured_at_ms: 0,
                    scrollback_lines_requested: 20,
                    scrollback_lines_included: content.lines().count().try_into().unwrap_or(0),
                    pane_history_lines: 0,
                }),
                CaptureFixture::Error => Err(CaptureError::TmuxNotAvailable),
            }
        }

        async fn interrupt(&mut self) -> Result<()> {
            self.actions.push("interrupt");
            Ok(())
        }

        async fn send_payload(&mut self) -> Result<()> {
            self.actions.push("payload");
            if self.payload_error_pane_dead {
                self.alive = false;
                return Err(anyhow::anyhow!("pane gone"));
            }
            Ok(())
        }

        async fn resubmit(&mut self) -> Result<()> {
            self.actions.push("resubmit");
            Ok(())
        }

        async fn sleep(&mut self, duration: Duration) {
            self.actions.push("sleep");
            self.now += duration.max(Duration::from_millis(1));
        }

        fn now(&self) -> Instant {
            self.now
        }
    }

    fn fast_timing() -> NudgeTiming {
        NudgeTiming {
            poll_interval: Duration::from_millis(1),
            timeout: Duration::from_millis(3),
            idle_probes_required: 2,
        }
    }

    #[tokio::test]
    async fn wait_timeout_returns_busy_without_payload() {
        let mut ops = FakeOps::new(vec![]).with_busy_default();

        let outcome = wait_for_idle(&mut ops, &RuntimeKind::Codex, fast_timing())
            .await
            .expect("wait succeeds");

        assert_eq!(outcome, PolicyOutcome::BusyTimeout);
        assert!(!ops.actions.contains(&"payload"));
    }

    #[tokio::test]
    async fn execute_wait_timeout_returns_agent_busy_without_payload() {
        let mut ops = FakeOps::new(vec![]).with_busy_default();

        let outcome = execute_nudge(&mut ops, NudgeMode::Wait, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::AgentBusyTimeout);
        assert!(!ops.actions.contains(&"payload"));
    }

    #[tokio::test]
    async fn execute_wait_uses_timeout_override() {
        let mut ops = FakeOps::new(vec![]).with_busy_default();
        let started_at = ops.now;

        let outcome = execute_nudge(&mut ops, NudgeMode::Wait, Some(3), &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::AgentBusyTimeout);
        let captures = ops
            .actions
            .iter()
            .filter(|action| **action == "capture")
            .count();
        assert!(captures >= WAIT_TIMING.idle_probes_required);
        assert_eq!(ops.now.duration_since(started_at), Duration::from_millis(3));
        assert!(!ops.actions.contains(&"payload"));
    }

    #[test]
    fn wait_timing_scales_short_timeout_poll_without_changing_default() {
        let default = wait_timing(None);
        assert_eq!(default.poll_interval, WAIT_TIMING.poll_interval);
        assert_eq!(default.timeout, WAIT_TIMING.timeout);
        assert_eq!(
            default.idle_probes_required,
            WAIT_TIMING.idle_probes_required
        );

        let short = wait_timing(Some(1));

        assert!(short.poll_interval > Duration::ZERO);
        assert!(short.poll_interval < WAIT_TIMING.poll_interval);
        assert!(short.poll_interval <= short.timeout);
        assert_eq!(short.timeout, Duration::from_millis(1));
        assert_eq!(short.idle_probes_required, WAIT_TIMING.idle_probes_required);
    }

    #[tokio::test]
    async fn steer_timeout_interrupts_once_without_payload() {
        let mut ops = FakeOps::new(vec![]).with_busy_default();

        let outcome = execute_nudge(&mut ops, NudgeMode::Steer, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::AgentBusyTimeout);
        assert_eq!(
            ops.actions
                .iter()
                .filter(|action| **action == "interrupt")
                .count(),
            1
        );
        assert!(!ops.actions.contains(&"payload"));
    }

    #[tokio::test]
    async fn debounce_requires_two_consecutive_idle_probes() {
        let mut ops = FakeOps::new(vec![
            CaptureFixture::Content(BUSY_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(BUSY_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
        ]);

        let outcome = wait_for_idle(
            &mut ops,
            &RuntimeKind::Codex,
            NudgeTiming {
                timeout: Duration::from_millis(20),
                ..fast_timing()
            },
        )
        .await
        .expect("wait succeeds");

        assert_eq!(outcome, PolicyOutcome::Ready);
        assert_eq!(
            ops.actions
                .iter()
                .filter(|action| **action == "capture")
                .count(),
            5
        );
    }

    #[tokio::test]
    async fn steer_capture_failure_best_effort_delivers_without_escape() {
        let mut ops = FakeOps::new(vec![CaptureFixture::Error]);

        let outcome = execute_nudge(&mut ops, NudgeMode::Steer, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert!(!ops.actions.contains(&"interrupt"));
        assert!(ops.actions.contains(&"payload"));
    }

    #[tokio::test]
    async fn copy_mode_cancel_precedes_policy_probe_and_payload_guard() {
        let mut ops = FakeOps::new(vec![
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(BUSY_CODEX),
        ])
        .with_copy_modes(vec![true, true]);

        let outcome = execute_nudge(&mut ops, NudgeMode::Wait, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert_eq!(
            ops.actions,
            vec![
                "alive",
                "pane_in_mode",
                "cancel_copy_mode",
                "capture",
                "sleep",
                "capture",
                "pane_in_mode",
                "cancel_copy_mode",
                "payload",
                "capture"
            ]
        );
    }

    #[tokio::test]
    async fn payload_send_failure_after_liveness_maps_to_pane_dead() {
        let mut ops = FakeOps::new(vec![]).with_payload_error_pane_dead();

        let outcome = execute_nudge(&mut ops, NudgeMode::Immediate, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::PaneDead);
    }

    #[tokio::test]
    async fn other_runtime_without_marker_does_not_wait_until_timeout() {
        let mut ops = FakeOps::new(vec![
            CaptureFixture::Content("custom prompt\n"),
            CaptureFixture::Content("custom prompt\n"),
        ]);

        let outcome = execute_nudge(
            &mut ops,
            NudgeMode::Wait,
            None,
            &RuntimeKind::Other("custom".to_owned()),
        )
        .await
        .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert!(ops.actions.contains(&"payload"));
    }

    #[tokio::test]
    async fn dropped_submit_is_resent_until_the_agent_starts_a_turn() {
        // First confirm round sees the agent still idle (submit dropped); after
        // one resend the agent goes busy, proving the resent submit landed.
        let mut ops = FakeOps::new(vec![
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(IDLE_CODEX),
            CaptureFixture::Content(BUSY_CODEX),
        ]);

        let outcome = execute_nudge(&mut ops, NudgeMode::Immediate, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert_eq!(ops.actions.iter().filter(|a| **a == "payload").count(), 1);
        assert_eq!(ops.actions.iter().filter(|a| **a == "resubmit").count(), 1);
    }

    #[tokio::test]
    async fn confirmed_submit_does_not_resend() {
        let mut ops = FakeOps::new(vec![CaptureFixture::Content(BUSY_CODEX)]);

        let outcome = execute_nudge(&mut ops, NudgeMode::Immediate, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert!(!ops.actions.contains(&"resubmit"));
    }

    #[tokio::test]
    async fn unconfirmed_submit_delivers_best_effort_after_max_resends() {
        // Agent never starts a turn: bounded resends, then best-effort Delivered
        // rather than a silent drop or an unbounded resend loop.
        let mut ops = FakeOps::new(vec![]);

        let outcome = execute_nudge(&mut ops, NudgeMode::Immediate, None, &RuntimeKind::Codex)
            .await
            .expect("nudge succeeds");

        assert_eq!(outcome, NudgeSendOutcome::Delivered);
        assert_eq!(
            ops.actions.iter().filter(|a| **a == "resubmit").count(),
            MAX_SUBMIT_RESENDS
        );
    }
}
