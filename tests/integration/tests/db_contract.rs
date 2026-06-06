use anyhow::Result;
use lilo_integration_tests::{IntegrationFixture, count_all};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn lilo_db_exposes_a_single_live_pool() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let pool = fixture.db.pool();

    let value: i32 = sqlx::query_scalar("SELECT 1").fetch_one(pool).await?;
    assert_eq!(value, 1);

    fixture.cleanup().await
}

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn concurrent_substrate_writes_share_one_pool() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let identity_writer = fixture.db.pool().clone();
    let runtime_writer = fixture.db.pool().clone();
    let session_writer = fixture.db.pool().clone();

    let identity = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO identity_audit
                 (id, timestamp, principal, action, resource, decision)
                 VALUES ($1, $2::timestamptz, $3, $4, $5, $6)",
            )
            .bind(format!("audit-{index}"))
            .bind("2026-05-28T00:00:00Z")
            .bind("local:0")
            .bind("daemon")
            .bind(format!("runtime:{index}"))
            .bind("allow")
            .execute(&identity_writer)
            .await?;
        }
        Result::<()>::Ok(())
    });
    let runtime = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO runtime_metadata (key, value, updated_at)
                 VALUES ($1, $2, $3::timestamptz)",
            )
            .bind(format!("runtime-key-{index}"))
            .bind("ok")
            .bind("2026-05-28T00:00:00Z")
            .execute(&runtime_writer)
            .await?;
        }
        Result::<()>::Ok(())
    });
    let session = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO session_namespaces (slug, created_at)
                 VALUES ($1, $2::timestamptz)",
            )
            .bind(format!("ns-{index}"))
            .bind("2026-05-28T00:00:00Z")
            .execute(&session_writer)
            .await?;
        }
        Result::<()>::Ok(())
    });

    identity.await??;
    runtime.await??;
    session.await??;

    assert_eq!(
        count_all(fixture.db.pool(), "SELECT COUNT(*) FROM identity_audit").await?,
        24
    );
    assert_eq!(
        count_all(fixture.db.pool(), "SELECT COUNT(*) FROM runtime_metadata").await?,
        24
    );
    assert_eq!(
        count_all(
            fixture.db.pool(),
            "SELECT COUNT(*) FROM session_namespaces WHERE slug <> 'default'",
        )
        .await?,
        24
    );

    fixture.cleanup().await
}
