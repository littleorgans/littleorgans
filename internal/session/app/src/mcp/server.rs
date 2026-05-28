use anyhow::{Result, bail};
use lilo_session_core::{McpBridgeRequest, RpcResponse, SessionRpc};
use tokio::io::{self, AsyncBufReadExt, BufReader};

use crate::mcp::transport::write_line;

pub async fn run_stdio_bridge() -> Result<()> {
    let stdin = BufReader::new(io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let response = crate::cli::client::send_request(&SessionRpc::McpBridge {
            request: McpBridgeRequest {
                line,
                caller_session_id: std::env::var("HELIOY_SESSION_ID").ok(),
            },
        })
        .await?;

        match response {
            RpcResponse::McpBridge { response } => {
                if let Some(line) = response.line {
                    write_line(&mut stdout, &line).await?;
                }
            }
            RpcResponse::Error { message } => bail!(message),
            other => bail!(
                "unexpected daemon response: {} (please report)",
                other.kind()
            ),
        }
    }

    Ok(())
}
