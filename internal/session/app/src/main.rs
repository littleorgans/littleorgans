#[tokio::main]
async fn main() -> anyhow::Result<()> {
    lilo_session_app::run().await
}
