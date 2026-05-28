use anyhow::Result;

use crate::cli::cli_def::McpArgs;

pub async fn run(_args: McpArgs) -> Result<()> {
    crate::mcp::server::run_stdio_bridge().await
}
