use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result, bail};
use lilo_paths::LiloPaths;
use lilo_session_core::Namespace;

use crate::cli::cli_def::{ConfigAction, ConfigArgs};

pub async fn run(args: ConfigArgs) -> Result<()> {
    match args.action {
        ConfigAction::SetContext(args) => set_context(args.namespace).await,
    }
}

async fn set_context(namespace: Namespace) -> Result<()> {
    ensure_namespace_exists(&namespace).await?;
    let paths = crate::cli::client::paths_from_env()?;
    write_namespace_binding(&paths, &namespace)?;
    match namespace_exists(&namespace).await {
        Ok(true) => {
            println!("current namespace: {namespace}");
            Ok(())
        }
        Ok(false) => {
            let _ = fs::remove_file(paths.namespace_binding());
            bail!("namespace deleted during bind: {namespace}")
        }
        Err(error) => {
            let _ = fs::remove_file(paths.namespace_binding());
            Err(error).context("failed to verify namespace binding after write")
        }
    }
}

async fn ensure_namespace_exists(namespace: &Namespace) -> Result<()> {
    if namespace_exists(namespace).await? {
        return Ok(());
    }

    bail!("unknown namespace: {namespace}")
}
async fn namespace_exists(namespace: &Namespace) -> Result<bool> {
    Ok(
        crate::cli::namespace::get_namespace_record(namespace.as_str().to_string())
            .await?
            .is_some(),
    )
}

fn write_namespace_binding(paths: &LiloPaths, namespace: &Namespace) -> Result<()> {
    let binding = paths.namespace_binding();
    let parent = binding
        .parent()
        .context("namespace binding path has no parent")?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    atomic_write_line(&binding, namespace.as_str())
}

fn atomic_write_line(path: &Path, value: &str) -> Result<()> {
    let temp_path = path.with_file_name(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("namespace"),
        std::process::id(),
        unique_write_suffix()
    ));
    let mut temp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    writeln!(temp, "{value}")
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    temp.sync_all()
        .with_context(|| format!("failed to sync {}", temp_path.display()))?;
    drop(temp);
    fs::rename(&temp_path, path).with_context(|| {
        let _ = fs::remove_file(&temp_path);
        format!("failed to replace {}", path.display())
    })
}

fn unique_write_suffix() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}
