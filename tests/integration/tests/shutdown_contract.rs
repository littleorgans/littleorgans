use std::time::Duration;

use anyhow::Result;
use lilo_integration_tests::{IntegrationFixture, runtime_config};
use lilo_runtime_daemon::{RuntimeService, RuntimeServiceContext};

#[tokio::test]
async fn runtime_shutdown_drains_before_shared_db_close() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let service = RuntimeService::build(RuntimeServiceContext::new(
        runtime_config(&fixture.paths),
        fixture.db.clone(),
    ))
    .await?;

    tokio::time::timeout(Duration::from_millis(100), service.shutdown()).await??;
    tokio::time::timeout(Duration::from_millis(100), service.shutdown()).await??;
    fixture.db.close().await;
    Ok(())
}
