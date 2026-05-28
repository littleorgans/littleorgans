use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GITHUB_SHA_ENV: &str = "GITHUB_SHA";
const LILO_GIT_SHA_ENV: &str = "LILO_GIT_SHA";
const VERSION_INCLUDE_GIT_SHA_ENV: &str = "LILO_VERSION_INCLUDE_GIT_SHA";

pub fn emit_cli_version(version_env: &str) {
    emit_git_rerun_directives();
    emit_git_sha_env_rerun_directives();

    let package_version = env::var("CARGO_PKG_VERSION")
        .unwrap_or_else(|error| panic!("CARGO_PKG_VERSION set: {error}"));
    let version = package_version_with_optional_git_sha(&package_version);
    println!("cargo:rustc-env={version_env}={version}");
}

pub fn package_version_with_optional_git_sha(package_version: &str) -> String {
    match (include_git_sha(), build_git_sha()) {
        (true, Some(sha)) => format!("{package_version}+{sha}"),
        _ => package_version.to_owned(),
    }
}

pub fn emit_git_sha_env_rerun_directives() {
    println!("cargo:rerun-if-env-changed={LILO_GIT_SHA_ENV}");
    println!("cargo:rerun-if-env-changed={GITHUB_SHA_ENV}");
    println!("cargo:rerun-if-env-changed={VERSION_INCLUDE_GIT_SHA_ENV}");
}

pub fn emit_git_rerun_directives() {
    let Some(head_path) = git_path("HEAD") else {
        return;
    };
    emit_rerun_if_path_exists(&head_path);

    if let Some(packed_refs) = git_path("packed-refs") {
        emit_rerun_if_path_exists(&packed_refs);
    }

    let Ok(head) = fs::read_to_string(&head_path) else {
        return;
    };
    if let Some(ref_path) = head.trim().strip_prefix("ref: ")
        && let Some(resolved) = git_path(ref_path)
    {
        emit_rerun_if_path_exists(&resolved);
    }
}

pub fn emit_rerun_if_path_exists(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

pub fn git_path(rel: &str) -> Option<PathBuf> {
    Command::new("git")
        .args(["rev-parse", "--git-path", rel])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

pub fn include_git_sha() -> bool {
    env::var(VERSION_INCLUDE_GIT_SHA_ENV).is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

pub fn build_git_sha() -> Option<String> {
    explicit_git_sha().or_else(git_head_sha)
}

pub fn explicit_git_sha() -> Option<String> {
    env::var(LILO_GIT_SHA_ENV)
        .ok()
        .and_then(|sha| short_sha(&sha))
        .or_else(|| {
            env::var(GITHUB_SHA_ENV)
                .ok()
                .and_then(|sha| short_sha(&sha))
        })
}

pub fn git_head_sha() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|stdout| short_sha(&stdout))
}

pub fn short_sha(value: &str) -> Option<String> {
    let sha = value.trim().chars().take(7).collect::<String>();
    let has_seven_hex_chars =
        sha.chars().count() == 7 && sha.chars().all(|ch| ch.is_ascii_hexdigit());
    has_seven_hex_chars.then_some(sha)
}
