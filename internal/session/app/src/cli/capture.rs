use anyhow::{Result, bail};
use lilo_rm_core::{CaptureError, CaptureResponse};

use lilo_session_core::{
    CaptureRequest, RpcRequest, RpcResponse, SmEndpoint, humanize_capture_error,
};

use crate::cli::cli_def::CaptureArgs;

pub async fn run(args: CaptureArgs) -> Result<()> {
    let endpoint = SmEndpoint::from_env()?;
    let response = lilo_session_daemon::send_request(
        &endpoint,
        &RpcRequest::Capture {
            request: CaptureRequest {
                session_id: args.session_id,
                scrollback_lines: args.scrollback_lines,
            },
        },
    )
    .await?;

    match response {
        RpcResponse::Capture { response } if args.json => {
            println!("{}", serde_json::to_string_pretty(&response)?);
            Ok(())
        }
        RpcResponse::Capture { response } => print_capture(response.capture),
        RpcResponse::Error { message } => bail!(message),
        other => bail!(
            "unexpected daemon response: {} (please report)",
            other.kind()
        ),
    }
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

fn capture_exit_code(error: &CaptureError) -> i32 {
    match error {
        CaptureError::NotATmuxTarget => 2,
        CaptureError::PaneUnavailable | CaptureError::SessionMissing => 3,
        _ => 4,
    }
}
