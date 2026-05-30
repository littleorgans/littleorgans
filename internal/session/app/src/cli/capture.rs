use anyhow::{Result, anyhow, bail};
use lilo_rm_core::{CaptureError, CaptureResponse, strip_ansi_escapes};

use lilo_session_core::{CaptureRequest, RpcResponse, SessionRpc, humanize_capture_error};

use crate::cli::cli_def::CaptureArgs;

pub async fn run(args: CaptureArgs, json: bool) -> Result<()> {
    let response = crate::cli::client::send_request(&SessionRpc::Capture {
        request: CaptureRequest {
            session_id: args.session_id,
            scrollback_lines: args.scrollback_lines,
        },
    })
    .await?;

    match response {
        RpcResponse::Capture { response } if json => {
            let response = sanitize_capture_response_for_json(response);
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        RpcResponse::Capture { response } => print_capture(response.capture),
        RpcResponse::Error { message } => bail!(message),
        other => Err(unexpected_daemon_response(&other)),
    }
}

pub(super) fn unexpected_daemon_response(response: &RpcResponse) -> anyhow::Error {
    anyhow!(
        "unexpected daemon response: {} (please report)",
        response.kind()
    )
}

fn print_capture(response: CaptureResponse) -> Result<()> {
    match response {
        CaptureResponse::Captured(snapshot) => {
            print!("{}", snapshot.content);
            Ok(())
        }
        CaptureResponse::Failed(error) => {
            eprintln!("{}", humanize_capture_error(&error));
            std::process::exit(capture_exit_code(&error));
        }
        _ => bail!("unsupported capture response"),
    }
}

fn sanitize_capture_response_for_json(
    mut response: lilo_session_core::CaptureResponse,
) -> lilo_session_core::CaptureResponse {
    response.capture = strip_json_capture_content(response.capture);
    response
}

fn strip_json_capture_content(response: CaptureResponse) -> CaptureResponse {
    match response {
        CaptureResponse::Captured(mut snapshot) => {
            snapshot.content = strip_ansi_escapes(&snapshot.content);
            CaptureResponse::Captured(snapshot)
        }
        other => other,
    }
}

fn capture_exit_code(error: &CaptureError) -> i32 {
    match error {
        CaptureError::NotATmuxTarget => 2,
        CaptureError::PaneUnavailable | CaptureError::SessionMissing => 3,
        _ => 4,
    }
}

#[cfg(test)]
mod tests {
    use lilo_rm_core::{CaptureResponse, PaneSnapshot};

    use super::strip_json_capture_content;

    #[test]
    fn json_capture_content_strips_ansi_escape_bytes() {
        let response = strip_json_capture_content(captured_response(
            "\u{1b}[31mred\u{1b}[0m\n\u{1b}]8;;https://example.test\u{7}link\u{1b}]8;;\u{1b}\\\n",
        ));
        let content = captured_content(response);

        assert_eq!(content, "red\nlink\n");
        assert!(!content.as_bytes().contains(&0x1b));
    }

    #[test]
    fn text_capture_content_keeps_ansi_before_json_sanitization() {
        let content = captured_content(captured_response("\u{1b}[31mred\u{1b}[0m\n"));

        assert_eq!(content, "\u{1b}[31mred\u{1b}[0m\n");
        assert!(content.as_bytes().contains(&0x1b));
    }

    fn captured_response(content: &str) -> CaptureResponse {
        CaptureResponse::Captured(PaneSnapshot {
            content: content.to_owned(),
            captured_at_ms: 1,
            scrollback_lines_requested: 3,
            scrollback_lines_included: content.lines().count().try_into().unwrap_or(u32::MAX),
            pane_history_lines: 42,
        })
    }

    fn captured_content(response: CaptureResponse) -> String {
        match response {
            CaptureResponse::Captured(snapshot) => snapshot.content,
            CaptureResponse::Failed(error) => panic!("unexpected capture error: {error:?}"),
            _ => panic!("unexpected capture response"),
        }
    }
}
