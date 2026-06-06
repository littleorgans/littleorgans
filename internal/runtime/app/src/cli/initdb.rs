use anyhow::Result;
use lilo_db::LiloDb;

pub async fn run() -> Result<()> {
    LiloDb::open_postgres_resolved().await?;
    println!("rtm db initialized");
    Ok(())
}
