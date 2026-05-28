use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use lilo_rm_core::tool_contracts::{ToolContract, ToolRegistry, contract_registry};

fn main() {
    emit_cli_version();
    println!("cargo:rerun-if-changed=../../../crates/lilo-rm-core/tools.toml");

    let manifest_dir = PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR set")),
    );
    let repo_root = manifest_dir
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("{} has a repo root parent", manifest_dir.display()));
    let registry = contract_registry();

    write_generated_sources(&manifest_dir, registry);
    write_readme(&repo_root.join("crates/lilo-rm-core/README.md"), registry);
}

fn emit_cli_version() {
    emit_git_rerun_directives();
    println!("cargo:rerun-if-env-changed=LILO_GIT_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=LILO_VERSION_INCLUDE_GIT_SHA");

    let package_version = std::env::var("CARGO_PKG_VERSION")
        .unwrap_or_else(|error| panic!("CARGO_PKG_VERSION set: {error}"));
    let version = match (include_git_sha(), build_git_sha()) {
        (true, Some(sha)) => format!("{package_version}+{sha}"),
        _ => package_version,
    };
    println!("cargo:rustc-env=RTM_CLI_VERSION={version}");
}

fn emit_git_rerun_directives() {
    // Resolve git directives via `git rev-parse --git-path` so the paths
    // work in both the main repo (.git is a directory) and a worktree (.git
    // is a pointer file at <main>/.git/worktrees/<name>/). Only emit
    // `cargo:rerun-if-changed` for paths that currently exist; otherwise
    // cargo flags the build script as stale on every invocation and forces
    // every downstream crate to recompile. Packed-refs and unpacked-ref
    // files toggle existence based on git GC state, so we tolerate either.
    let Some(head_path) = git_path("HEAD") else {
        return;
    };
    emit_rerun_if_path_exists(Path::new(&head_path));

    if let Some(packed_refs) = git_path("packed-refs") {
        emit_rerun_if_path_exists(Path::new(&packed_refs));
    }

    let Ok(head) = fs::read_to_string(&head_path) else {
        return;
    };
    if let Some(ref_path) = head.trim().strip_prefix("ref: ")
        && let Some(resolved) = git_path(ref_path)
    {
        emit_rerun_if_path_exists(Path::new(&resolved));
    }
}

fn emit_rerun_if_path_exists(path: &Path) {
    if path.exists() {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn git_path(rel: &str) -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--git-path", rel])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn include_git_sha() -> bool {
    matches!(
        std::env::var("LILO_VERSION_INCLUDE_GIT_SHA").as_deref(),
        Ok("1" | "true" | "yes")
    )
}

fn build_git_sha() -> Option<String> {
    explicit_git_sha().or_else(git_head_sha)
}

fn explicit_git_sha() -> Option<String> {
    std::env::var("LILO_GIT_SHA")
        .ok()
        .and_then(|sha| short_sha(&sha))
        .or_else(|| {
            std::env::var("GITHUB_SHA")
                .ok()
                .and_then(|sha| short_sha(&sha))
        })
}

fn git_head_sha() -> Option<String> {
    Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|stdout| short_sha(&stdout))
}

fn short_sha(value: &str) -> Option<String> {
    let sha = value.trim().chars().take(7).collect::<String>();
    (!sha.is_empty() && sha.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(sha)
}

fn write_generated_sources(manifest_dir: &Path, registry: &ToolRegistry) {
    let generated_dir = manifest_dir.join("src/generated");
    fs::create_dir_all(&generated_dir).unwrap_or_else(|error| {
        panic!("create generated dir {}: {error}", generated_dir.display())
    });
    write_if_changed(
        &generated_dir.join("mod.rs"),
        "pub mod cli_help;\npub mod contracts;\npub mod mcp_tools;\n",
    );
    write_if_changed(&generated_dir.join("cli_help.rs"), &cli_help(registry));
    write_if_changed(&generated_dir.join("contracts.rs"), &contracts(registry));
    write_if_changed(&generated_dir.join("mcp_tools.rs"), &mcp_tools(registry));
}

fn write_readme(readme_path: &Path, registry: &ToolRegistry) {
    let readme = fs::read_to_string(readme_path)
        .unwrap_or_else(|error| panic!("read {}: {error}", readme_path.display()));
    let section = format!(
        "<!-- rtm-admin-tools:start -->\n{}<!-- rtm-admin-tools:end -->",
        registry.admin_tools_markdown()
    );
    let next = replace_or_append(&readme, &section);
    write_if_changed(readme_path, &next);
}

fn cli_help(registry: &ToolRegistry) -> String {
    let mut output = String::from("// @generated by rtm-cli/build.rs\n\n");
    output.push_str("pub const MCP_ABOUT: &str = \"Run the admin MCP stdio bridge.\";\n");
    for contract in registry.tools.values() {
        writeln!(
            output,
            "pub const {}_ABOUT: &str = {};",
            const_name(&contract.cli_name),
            rust_string(&contract.cli_about)
        )
        .unwrap_or_else(|error| panic!("write generated CLI help: {error}"));
    }
    output
}

fn contracts(registry: &ToolRegistry) -> String {
    let mut output = String::from("// @generated by rtm-cli/build.rs\n\n");
    for (name, contract) in &registry.tools {
        output.push_str(&contract_aliases(name, contract));
    }
    format!("{}\n", output.trim_end())
}

fn mcp_tools(registry: &ToolRegistry) -> String {
    let tool_list = serde_json::to_string_pretty(&registry.tool_list_value())
        .unwrap_or_else(|error| panic!("tool list serializes: {error}"));
    let names = registry
        .tools
        .keys()
        .map(|name| format!("    {},", rust_string(name)))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "// @generated by rtm-cli/build.rs\n\n\
         pub const TOOL_LIST_JSON: &str = r#\"{tool_list}\"#;\n\
         pub const TOOL_NAMES: &[&str] = &[\n{names}\n];\n"
    )
}

fn contract_aliases(name: &str, contract: &ToolContract) -> String {
    match name {
        "rtm_kill_by_pid" => format!(
            "pub type {} = lilo_rm_core::KillByPidRequest;\n\
             pub type {} = lilo_rm_core::KillByPidResponse;\n\n",
            contract.args_type, contract.response_type
        ),
        "rtm_status" => format!(
            "pub type {} = lilo_rm_core::StatusFilter;\n\
             pub type {} = lilo_rm_core::StatusResponse;\n\n",
            contract.args_type, contract.response_type
        ),
        "rtm_version" => format!(
            "#[derive(Clone, Debug, Default, Eq, PartialEq)]\n\
             pub struct {};\n\
             pub type {} = lilo_rm_core::VersionInfo;\n\n",
            contract.args_type, contract.response_type
        ),
        "rtm_watchers" => format!(
            "#[derive(Clone, Debug, Default, Eq, PartialEq)]\n\
             pub struct {};\n\
             pub type {} = lilo_rm_core::WatcherCounts;\n\n",
            contract.args_type, contract.response_type
        ),
        other => panic!("unsupported tool contract {other}"),
    }
}

fn replace_or_append(readme: &str, section: &str) -> String {
    let start = "<!-- rtm-admin-tools:start -->";
    let end = "<!-- rtm-admin-tools:end -->";
    let Some(start_index) = readme.find(start) else {
        return format!("{}\n\n{}\n", readme.trim_end(), section);
    };
    let end_index = readme[start_index..].find(end).map_or_else(
        || panic!("README admin tools section end marker"),
        |index| start_index + index + end.len(),
    );
    format!(
        "{}{}{}",
        &readme[..start_index],
        section,
        &readme[end_index..]
    )
}

fn const_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_uppercase()
            } else {
                '_'
            }
        })
        .collect()
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn write_if_changed(path: &Path, contents: &str) {
    if fs::read_to_string(path).is_ok_and(|existing| existing == contents) {
        return;
    }
    fs::write(path, contents).unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}
