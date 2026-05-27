use anyhow::Result;
use lilo_session_core::SmPaths;

use crate::cli::cli_def::McpArgs;

pub async fn run(_args: McpArgs) -> Result<()> {
    crate::mcp::server::run_stdio_bridge(SmPaths::from_env()?).await
}
