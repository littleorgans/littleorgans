use std::path::PathBuf;

use anyhow::{Result, bail};
use lilo_rm_core::{McpBridgeRequest, RuntimeResponse, RuntimeRpc};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_stdio(socket_path: PathBuf) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let mut stdout = tokio::io::stdout();
    let mut line = String::new();

    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        let request_line = line.trim_end();
        if request_line.is_empty() {
            continue;
        }

        if let Some(response_line) = bridge_line(&socket_path, request_line).await? {
            stdout.write_all(response_line.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
}

async fn bridge_line(socket_path: &std::path::Path, line: &str) -> Result<Option<String>> {
    let response = crate::shared::request(
        socket_path,
        RuntimeRpc::McpBridge {
            request: McpBridgeRequest {
                line: line.to_owned(),
            },
        },
    )
    .await?;

    match response {
        RuntimeResponse::McpBridge(payload) => Ok(payload.response.line),
        other => bail!("unexpected MCP bridge response: {other:?}"),
    }
}
