use anyhow::Result;
use lilo_db::LiloDb;
use lilo_runtime_store::StoreConfig;

pub async fn run() -> Result<()> {
    let config = StoreConfig::from_env()?;
    let path = config.db_path.clone();
    LiloDb::open_path(&path).await?;
    println!("rtm db initialized at {}", path.display());
    Ok(())
}
