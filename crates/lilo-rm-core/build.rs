use std::process::Command;

fn main() {
    // Use `git rev-parse --git-path HEAD` so this resolves correctly in
    // both the main repo (.git is a directory) and a worktree (.git is a
    // pointer file). Hardcoding ../../.git/HEAD made cargo flag the file as
    // missing in worktrees, forcing every dependent crate to recompile on
    // every cargo invocation.
    if let Some(head_path) = Command::new("git")
        .args(["rev-parse", "--git-path", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .filter(|s| std::path::Path::new(s).exists())
    {
        println!("cargo:rerun-if-changed={head_path}");
    }
    let sha = Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=RTM_GIT_SHA={sha}");
}
