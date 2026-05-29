use anyhow::{Context, Result, bail};
use lilo_rm_core::{CaptureError, LaunchEnv, PaneSnapshot, TmuxAddress};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::process::Command;

pub struct TmuxGateway;

impl TmuxGateway {
    pub async fn version() -> Result<Option<String>> {
        let Some(output) = tmux_output(["-V"]).await? else {
            return Ok(None);
        };
        if !output.status.success() {
            return Ok(None);
        }
        Ok(Some(stdout(&output).trim().to_owned()))
    }

    pub async fn nudge(
        server_label: Option<&str>,
        tmux_pane: &TmuxAddress,
        content: &str,
    ) -> Result<bool> {
        if !Self::is_alive(server_label, tmux_pane).await? {
            return Ok(false);
        }
        for trailing in build_nudge_send_keys_steps(content) {
            send_keys(server_label, tmux_pane, &trailing).await?;
        }
        Ok(true)
    }

    pub async fn respawn_pane(
        server_label: Option<&str>,
        tmux_pane: &TmuxAddress,
        argv: &[String],
        env: &[LaunchEnv],
    ) -> Result<()> {
        let respawn_args = build_respawn_pane_args(tmux_pane, argv, env)?;
        let output = tmux_output_owned_with_label(server_label, respawn_args)
            .await?
            .context("tmux is not installed")?;
        ensure_success(&output, "tmux respawn-pane").map(|_| ())
    }

    pub async fn is_alive(server_label: Option<&str>, tmux_pane: &TmuxAddress) -> Result<bool> {
        let has_session = tmux_output_owned_with_label(
            server_label,
            vec![
                "has-session".to_owned(),
                "-t".to_owned(),
                tmux_pane.session.clone(),
            ],
        )
        .await?;
        let Some(has_session) = has_session else {
            return Ok(false);
        };
        if !has_session.status.success() {
            return Ok(false);
        }

        let target = tmux_pane.to_string();
        let panes = tmux_output_owned_with_label(
            server_label,
            vec![
                "list-panes".to_owned(),
                "-t".to_owned(),
                target.clone(),
                "-F".to_owned(),
                "#S:#I.#P".to_owned(),
            ],
        )
        .await?
        .context("tmux is not installed")?;
        ensure_success(&panes, "tmux list-panes")
            .map(|stdout| stdout.lines().any(|line| line.trim() == target))
    }

    pub async fn capture_pane(
        server_label: Option<&str>,
        tmux_pane: &TmuxAddress,
        scrollback_lines: u32,
    ) -> std::result::Result<PaneSnapshot, CaptureError> {
        let content_output = tmux_capture_output(server_label, tmux_pane, scrollback_lines)
            .await?
            .ok_or(CaptureError::TmuxNotAvailable)?;
        if !content_output.status.success() {
            return Err(CaptureError::CapturePaneFailed {
                stderr: stderr(&content_output),
            });
        }
        let history_output = tmux_history_output(server_label, tmux_pane)
            .await?
            .ok_or(CaptureError::TmuxNotAvailable)?;
        if !history_output.status.success() {
            return Err(CaptureError::CapturePaneFailed {
                stderr: stderr(&history_output),
            });
        }
        let content = stdout(&content_output);
        let pane_history_lines = parse_history_lines(&stdout(&history_output))?;
        let scrollback_lines_included = content.lines().count().try_into().unwrap_or(u32::MAX);
        Ok(PaneSnapshot {
            content,
            captured_at_ms: unix_epoch_ms(),
            scrollback_lines_requested: scrollback_lines,
            scrollback_lines_included,
            pane_history_lines,
        })
    }
}

async fn tmux_output<const N: usize>(args: [&str; N]) -> Result<Option<std::process::Output>> {
    tmux_output_owned(args.into_iter().map(str::to_owned).collect()).await
}

async fn tmux_capture_output(
    server_label: Option<&str>,
    tmux_pane: &TmuxAddress,
    scrollback_lines: u32,
) -> std::result::Result<Option<std::process::Output>, CaptureError> {
    tmux_output_owned_with_label(
        server_label,
        build_capture_pane_args(tmux_pane, scrollback_lines),
    )
    .await
    .map_err(|error| CaptureError::CapturePaneFailed {
        stderr: error.to_string(),
    })
}

fn build_capture_pane_args(tmux_pane: &TmuxAddress, scrollback_lines: u32) -> Vec<String> {
    vec![
        "capture-pane".to_owned(),
        "-p".to_owned(),
        "-e".to_owned(),
        "-S".to_owned(),
        format!("-{scrollback_lines}"),
        "-t".to_owned(),
        tmux_pane.to_string(),
    ]
}

async fn tmux_history_output(
    server_label: Option<&str>,
    tmux_pane: &TmuxAddress,
) -> std::result::Result<Option<std::process::Output>, CaptureError> {
    tmux_output_owned_with_label(
        server_label,
        vec![
            "display-message".to_owned(),
            "-p".to_owned(),
            "-t".to_owned(),
            tmux_pane.to_string(),
            "#{history_size}".to_owned(),
        ],
    )
    .await
    .map_err(|error| CaptureError::CapturePaneFailed {
        stderr: error.to_string(),
    })
}

async fn send_keys(
    server_label: Option<&str>,
    tmux_pane: &TmuxAddress,
    trailing: &[String],
) -> Result<()> {
    let target = tmux_pane.to_string();
    let mut args = vec!["send-keys".to_owned(), "-t".to_owned(), target.clone()];
    args.extend(trailing.iter().cloned());
    let output = tmux_output_owned_with_label(server_label, args)
        .await?
        .context("tmux is not installed")?;
    ensure_success(&output, "tmux send-keys").map(|_| ())
}

async fn tmux_output_owned(args: Vec<String>) -> Result<Option<std::process::Output>> {
    tmux_output_owned_with_label(None, args).await
}

async fn tmux_output_owned_with_label(
    server_label: Option<&str>,
    args: Vec<String>,
) -> Result<Option<std::process::Output>> {
    let mut command = Command::new("tmux");
    if let Some(label) = server_label {
        command.arg("-L").arg(label);
    }
    match command.args(args).output().await {
        Ok(output) => Ok(Some(output)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).context("failed to run tmux"),
    }
}

fn ensure_success(output: &std::process::Output, label: &'static str) -> Result<String> {
    if output.status.success() {
        return Ok(stdout(output));
    }
    bail!("{label} failed: {}", stderr(output))
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

fn parse_history_lines(stdout: &str) -> std::result::Result<u32, CaptureError> {
    stdout
        .trim()
        .parse()
        .map_err(|error| CaptureError::CapturePaneFailed {
            stderr: format!("invalid tmux history_size output: {error}"),
        })
}

fn unix_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Build the per-step argv trailers for the three send-keys invocations used
/// to deliver a nudge.
///
/// Mirrors nancy's pattern: send the literal payload first so bracketed-paste
/// mode does not eat shell metacharacters, then a hex CR (`0d`) to flush any
/// terminal paste buffer, then a real `Enter` to submit. Without the final
/// `Enter`, agents like Claude Code see the payload typed but never submitted.
fn build_nudge_send_keys_steps(content: &str) -> [Vec<String>; 3] {
    [
        vec!["-l".to_owned(), content.to_owned()],
        vec!["-H".to_owned(), "0d".to_owned()],
        vec!["Enter".to_owned()],
    ]
}

fn build_respawn_pane_args(
    tmux_pane: &TmuxAddress,
    argv: &[String],
    env: &[LaunchEnv],
) -> Result<Vec<String>> {
    if argv.is_empty() {
        bail!("tmux respawn-pane requires argv");
    }
    let mut respawn_args = vec![
        "respawn-pane".to_owned(),
        "-k".to_owned(),
        "-t".to_owned(),
        tmux_pane.to_string(),
    ];
    for entry in env {
        respawn_args.push("-e".to_owned());
        respawn_args.push(format!("{}={}", entry.key, entry.value));
    }
    respawn_args.push("--".to_owned());
    respawn_args.extend(argv.iter().cloned());
    Ok(respawn_args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane() -> TmuxAddress {
        "rtm:0.1".parse().expect("pane parse")
    }

    #[test]
    fn respawn_pane_args_only_carry_provided_env() {
        let env = vec![LaunchEnv::new("LILO_SOCKET_PATH", "/tmp/lilod.sock")];
        let command_argv = vec!["rtm".to_owned(), "__shim".to_owned()];
        let respawn_args = build_respawn_pane_args(&pane(), &command_argv, &env).expect("args");

        let mut e_flags = Vec::new();
        let mut iter = respawn_args.iter();
        while let Some(arg) = iter.next() {
            if arg == "-e" {
                e_flags.push(iter.next().expect("paired -e value").clone());
            }
        }
        assert_eq!(e_flags, vec!["LILO_SOCKET_PATH=/tmp/lilod.sock".to_owned()]);
    }

    #[test]
    fn respawn_pane_args_never_leak_runtime_env() {
        // Regression guard: the daemon hands tmux only the bootstrap socket var.
        // Runtime env (secrets, PATH, etc.) must travel over the post-spawn UDS
        // ShimLaunch handoff, never via tmux's -e flag or argv.
        let env = vec![LaunchEnv::new("LILO_SOCKET_PATH", "/tmp/lilod.sock")];
        let command_argv = vec!["rtm".to_owned(), "__shim".to_owned()];
        let respawn_args = build_respawn_pane_args(&pane(), &command_argv, &env).expect("args");

        let e_values: Vec<&str> = respawn_args
            .windows(2)
            .filter(|pair| pair[0] == "-e")
            .map(|pair| pair[1].as_str())
            .collect();
        assert_eq!(e_values, vec!["LILO_SOCKET_PATH=/tmp/lilod.sock"]);

        for forbidden in [
            "HELIOY_PAT=",
            "ANTHROPIC_API_KEY=",
            "PATH=",
            "CLAUDE_CODE_SESSION_ID=",
        ] {
            assert!(
                !e_values.iter().any(|v| v.starts_with(forbidden)),
                "respawn-pane -e values leaked {forbidden}: {e_values:?}"
            );
        }
    }

    #[test]
    fn nudge_steps_are_literal_then_cr_then_enter() {
        let steps = build_nudge_send_keys_steps("hello world");
        assert_eq!(steps[0], vec!["-l".to_owned(), "hello world".to_owned()]);
        assert_eq!(steps[1], vec!["-H".to_owned(), "0d".to_owned()]);
        assert_eq!(steps[2], vec!["Enter".to_owned()]);
    }

    #[test]
    fn nudge_steps_preserve_special_chars_verbatim() {
        // -l means literal: payload bytes must be sent unmodified including
        // backticks, dollar signs, quotes. Submission is the separate Enter
        // step, not parsed from the payload.
        let steps = build_nudge_send_keys_steps("echo \"$PWD\" && ls -la");
        assert_eq!(steps[0][1], "echo \"$PWD\" && ls -la");
    }

    #[test]
    fn respawn_pane_args_reject_empty_argv() {
        let err =
            build_respawn_pane_args(&pane(), &[], &[]).expect_err("empty argv should be rejected");
        assert!(
            err.to_string().contains("requires argv"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn capture_pane_args_include_ansi_and_scrollback() {
        assert_eq!(
            build_capture_pane_args(&pane(), 250),
            vec![
                "capture-pane".to_owned(),
                "-p".to_owned(),
                "-e".to_owned(),
                "-S".to_owned(),
                "-250".to_owned(),
                "-t".to_owned(),
                "rtm:0.1".to_owned()
            ]
        );
    }

    #[test]
    fn parse_history_lines_rejects_invalid_tmux_output() {
        let err = parse_history_lines("not-a-number\n").expect_err("invalid history should fail");
        assert!(
            matches!(err, CaptureError::CapturePaneFailed { .. }),
            "unexpected error: {err:?}"
        );
    }
}
