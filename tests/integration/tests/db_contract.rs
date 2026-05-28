use anyhow::Result;
use lilo_integration_tests::{IntegrationFixture, count_all};

#[tokio::test]
async fn lilo_db_applies_shared_sqlite_pragmas() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let pool = fixture.db.session_pool();

    let journal_mode: String = sqlx::query_scalar("PRAGMA journal_mode")
        .fetch_one(pool)
        .await?;
    let busy_timeout: i64 = sqlx::query_scalar("PRAGMA busy_timeout")
        .fetch_one(pool)
        .await?;
    let synchronous: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(pool)
        .await?;

    assert_eq!(journal_mode.to_ascii_lowercase(), "wal");
    assert_eq!(busy_timeout, 5_000);
    assert_eq!(synchronous, 1);
    Ok(())
}

#[tokio::test]
async fn concurrent_substrate_writes_share_pool_without_sqlite_busy() -> Result<()> {
    let fixture = IntegrationFixture::open().await?;
    let identity_pool = fixture.db.identity_pool().clone();
    let runtime_pool = fixture.db.runtime_pool().clone();
    let session_pool = fixture.db.session_pool().clone();

    let identity = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO identity_audit
                 (id, timestamp, principal, action, resource, decision)
                 VALUES (?, ?, ?, ?, ?, ?)",
            )
            .bind(format!("audit-{index}"))
            .bind("2026-05-28T00:00:00Z")
            .bind("local:0")
            .bind("daemon")
            .bind(format!("runtime:{index}"))
            .bind("allow")
            .execute(&identity_pool)
            .await?;
        }
        Result::<()>::Ok(())
    });
    let runtime = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO runtime_metadata (key, value, updated_at)
                 VALUES (?, ?, ?)",
            )
            .bind(format!("runtime-key-{index}"))
            .bind("ok")
            .bind("2026-05-28T00:00:00Z")
            .execute(&runtime_pool)
            .await?;
        }
        Result::<()>::Ok(())
    });
    let session = tokio::spawn(async move {
        for index in 0..24 {
            sqlx::query(
                "INSERT INTO session_namespaces (slug, created_at)
                 VALUES (?, ?)",
            )
            .bind(format!("ns-{index}"))
            .bind("2026-05-28T00:00:00Z")
            .execute(&session_pool)
            .await?;
        }
        Result::<()>::Ok(())
    });

    identity.await??;
    runtime.await??;
    session.await??;

    assert_eq!(
        count_all(
            fixture.db.identity_pool(),
            "SELECT COUNT(*) FROM identity_audit"
        )
        .await?,
        24
    );
    assert_eq!(
        count_all(
            fixture.db.runtime_pool(),
            "SELECT COUNT(*) FROM runtime_metadata"
        )
        .await?,
        24
    );
    assert_eq!(
        count_all(
            fixture.db.session_pool(),
            "SELECT COUNT(*) FROM session_namespaces WHERE slug <> 'default'",
        )
        .await?,
        24
    );
    Ok(())
}
