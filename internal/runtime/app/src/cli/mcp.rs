use anyhow::Result;

pub async fn run() -> Result<()> {
    let socket_path = crate::shared::socket_path()?;
    crate::mcp::run_stdio(socket_path).await
}
