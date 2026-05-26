use std::path::{Path, PathBuf};

fn main() {
    emit_cli_version();
}

fn emit_cli_version() {
    emit_git_rerun_directives();
    println!("cargo:rerun-if-env-changed=LILO_GIT_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=LILO_VERSION_INCLUDE_GIT_SHA");

    let package_version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION set");
    let version = match (include_git_sha(), build_git_sha()) {
        (true, Some(sha)) => format!("{package_version}+{sha}"),
        _ => package_version,
    };
    println!("cargo:rustc-env=LILO_CLI_VERSION={version}");
}

fn include_git_sha() -> bool {
    matches!(
        std::env::var("LILO_VERSION_INCLUDE_GIT_SHA").as_deref(),
        Ok("1" | "true" | "TRUE" | "yes")
    )
}

fn build_git_sha() -> Option<String> {
    std::env::var("LILO_GIT_SHA")
        .ok()
        .and_then(|sha| short_sha(&sha))
        .or_else(|| {
            std::env::var("GITHUB_SHA")
                .ok()
                .and_then(|sha| short_sha(&sha))
        })
        .or_else(git_head_sha)
}

fn short_sha(sha: &str) -> Option<String> {
    let trimmed = sha.trim();
    if trimmed.len() < 7 {
        return None;
    }
    Some(trimmed[..7].to_string())
}

fn emit_git_rerun_directives() {
    emit_rerun_if_path_exists(&workspace_git_path());

    let Some(git_dir) = resolve_git_dir() else {
        return;
    };

    let head_path = git_dir.join("HEAD");
    emit_rerun_if_path_exists(&head_path);

    let Ok(head) = std::fs::read_to_string(&head_path) else {
        return;
    };
    if let Some(ref_path) = head.trim().strip_prefix("ref: ") {
        emit_rerun_if_path_exists(&git_dir.join(ref_path));
        if let Some(common_dir) = resolve_common_git_dir(&git_dir) {
            emit_rerun_if_path_exists(&common_dir.join(ref_path));
            emit_rerun_if_path_exists(&common_dir.join("packed-refs"));
        }
    }
}

/// Print a `cargo:rerun-if-changed` directive only when the path actually
/// exists on disk. Git refs may be packed (file absent) or unpacked (file
/// present), and the workspace's .git layout in a worktree (.git is a
/// pointer file, real data under <main>/.git/worktrees/<name>/) means many
/// of the canonical paths cargo would otherwise track are missing. When
/// cargo encounters a `rerun-if-changed` target that does not exist it
/// flags the build script as stale on every invocation, forcing every
/// downstream crate to recompile.
fn emit_rerun_if_path_exists(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_head_sha() -> Option<String> {
    let git_dir = resolve_git_dir()?;
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let trimmed = head.trim();
    if let Some(ref_path) = trimmed.strip_prefix("ref: ") {
        for dir in git_lookup_dirs(&git_dir) {
            let ref_file = dir.join(ref_path);
            if let Ok(sha) = std::fs::read_to_string(&ref_file) {
                return short_sha(sha.trim());
            }
        }
        for dir in git_lookup_dirs(&git_dir) {
            if let Ok(packed) = std::fs::read_to_string(dir.join("packed-refs")) {
                for line in packed.lines() {
                    if let Some((sha, name)) = line.split_once(' ')
                        && name == ref_path
                    {
                        return short_sha(sha);
                    }
                }
            }
        }
        None
    } else {
        short_sha(trimmed)
    }
}

fn workspace_git_path() -> PathBuf {
    PathBuf::from("../../.git")
}

fn resolve_git_dir() -> Option<PathBuf> {
    let git_path = workspace_git_path();
    if git_path.is_dir() {
        return Some(git_path);
    }

    let git_file = std::fs::read_to_string(&git_path).ok()?;
    let git_dir = git_file.trim().strip_prefix("gitdir: ")?;
    let git_dir = PathBuf::from(git_dir);
    if git_dir.is_absolute() {
        Some(git_dir)
    } else {
        Some(
            git_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(git_dir),
        )
    }
}

fn resolve_common_git_dir(git_dir: &Path) -> Option<PathBuf> {
    let common_dir = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common_dir = PathBuf::from(common_dir.trim());
    if common_dir.is_absolute() {
        Some(common_dir)
    } else {
        Some(git_dir.join(common_dir))
    }
}

fn git_lookup_dirs(git_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![git_dir.to_path_buf()];
    if let Some(common_dir) = resolve_common_git_dir(git_dir)
        && common_dir != git_dir
    {
        dirs.push(common_dir);
    }
    dirs
}
