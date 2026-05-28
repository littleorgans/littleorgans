use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

#[path = "src/tool_contracts.rs"]
mod tool_contracts;
#[path = "src/tool_docs.rs"]
mod tool_docs;
#[path = "src/tool_examples.rs"]
mod tool_examples;
#[path = "../core/src/tool_sources.rs"]
mod tool_sources;

use tool_contracts::ToolContractRegistry;
use tool_docs::{render_generated_instructions_rs, render_readme_md, render_server_instructions};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=tools");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/tool_contracts.rs");
    println!("cargo:rerun-if-changed=src/tool_docs.rs");
    println!("cargo:rerun-if-changed=src/tool_examples.rs");
    println!("cargo:rerun-if-changed=../core/src/tool_sources.rs");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")?;
    let tools_dir = Path::new(&manifest_dir).join("tools");
    let tool_paths = tool_sources::ordered_tool_source_paths(&tools_dir)?;
    for path in &tool_paths {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let content = tool_sources::read_tool_sources(&tool_paths)?;
    let registry = ToolContractRegistry::from_toml_str(&content)?;

    write_schema_outputs(&manifest_dir, &registry)?;
    write_docs_outputs(&manifest_dir, &registry)?;
    emit_cli_version()?;
    Ok(())
}

fn emit_cli_version() -> Result<(), Box<dyn Error>> {
    emit_git_rerun_directives();
    println!("cargo:rerun-if-env-changed=LILO_GIT_SHA");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=LILO_VERSION_INCLUDE_GIT_SHA");

    let package_version = std::env::var("CARGO_PKG_VERSION")?;
    let version = match (include_git_sha(), build_git_sha()) {
        (true, Some(sha)) => format!("{package_version}+{sha}"),
        _ => package_version,
    };
    println!("cargo:rustc-env=SM_CLI_VERSION={version}");
    Ok(())
}

fn emit_git_rerun_directives() {
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
        Ok("1" | "true")
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

fn short_sha(sha: &str) -> Option<String> {
    let trimmed = sha.trim();
    if trimmed.len() < 7 {
        return None;
    }
    Some(trimmed[..7].to_string())
}

fn write_schema_outputs(
    manifest_dir: &str,
    registry: &ToolContractRegistry,
) -> Result<(), Box<dyn Error>> {
    let (schema_rs, schema_files) = generate_mcp_schema(registry)?;
    write_if_changed(
        &Path::new(manifest_dir).join("src/mcp/generated_schema.rs"),
        &schema_rs,
    )?;

    let schema_dir = Path::new(manifest_dir).join("src/mcp/generated_schema");
    fs::create_dir_all(&schema_dir)?;
    let mut expected = HashSet::new();
    for (file_name, content) in &schema_files {
        expected.insert(file_name.as_str());
        write_if_changed(&schema_dir.join(file_name), content)?;
    }
    remove_stale_generated_files(&schema_dir, &expected)?;
    Ok(())
}

fn write_docs_outputs(
    manifest_dir: &str,
    registry: &ToolContractRegistry,
) -> Result<(), Box<dyn Error>> {
    let instructions =
        render_server_instructions(registry.skill(), registry.shared(), registry.tools());
    let instructions_rs = render_generated_instructions_rs(&instructions);
    write_if_changed(
        &Path::new(manifest_dir).join("src/mcp/generated_instructions.rs"),
        &instructions_rs,
    )?;

    write_if_changed(
        &Path::new(manifest_dir).join("src/cli/generated_help.rs"),
        &generate_cli_help(registry),
    )?;

    write_if_changed(
        &Path::new(manifest_dir).join("README.md"),
        &render_readme_md(registry.skill(), registry.shared(), registry.tools()),
    )?;
    Ok(())
}

fn generate_mcp_schema(
    registry: &ToolContractRegistry,
) -> Result<(String, Vec<(String, String)>), serde_json::Error> {
    let mut include_lines = Vec::new();
    let mut schema_files = Vec::new();
    for tool in registry.tools() {
        let file_name = tool.artifacts.mcp_schema_file.clone();
        let tool_entry = tool.tool_entry_value(registry.shared());
        let json = serde_json::to_string_pretty(&tool_entry)?;
        include_lines.push(format!(
            "        generated_tool_schema(include_str!(\"generated_schema/{file_name}\"), \"{}\"),",
            tool.name
        ));
        schema_files.push((file_name, format!("{json}\n")));
    }

    let mut schema_rs = String::new();
    schema_rs.push_str("// AUTO-GENERATED by build.rs from tools/*.toml - do not edit\n");
    schema_rs.push_str("#![allow(clippy::all)]\n\n");
    schema_rs.push_str("#[rustfmt::skip]\n");
    schema_rs.push_str("pub fn generated_tool_list() -> serde_json::Value {\n");
    schema_rs.push_str("    let tools: Vec<serde_json::Value> = vec![\n");
    for line in include_lines {
        schema_rs.push_str(&line);
        schema_rs.push('\n');
    }
    schema_rs.push_str("    ];\n");
    schema_rs.push_str("    serde_json::json!({ \"tools\": tools })\n");
    schema_rs.push_str("}\n");
    schema_rs.push('\n');
    schema_rs.push_str("fn generated_tool_schema(json: &str, name: &str) -> serde_json::Value {\n");
    schema_rs.push_str("    serde_json::from_str(json).unwrap_or_else(|error| {\n");
    schema_rs.push_str("        panic!(\"generated schema for {name} is valid JSON: {error}\");\n");
    schema_rs.push_str("    })\n");
    schema_rs.push_str("}\n");
    Ok((schema_rs, schema_files))
}

fn generate_cli_help(registry: &ToolContractRegistry) -> String {
    let mut lines = vec![
        "// AUTO-GENERATED by build.rs from tools/*.toml - do not edit".to_string(),
        "#![allow(clippy::all)]".to_string(),
    ];
    for tool in registry.tools() {
        if !tool.artifacts.render_cli_help {
            continue;
        }
        let prefix = &tool.artifacts.cli_help_prefix;
        lines.push("#[rustfmt::skip]".to_string());
        lines.push(format!(
            "pub const {prefix}_ABOUT: &str = \"{}\";",
            rust_escape(&tool.cli.about)
        ));
        for param in &tool.params {
            if let Some(help) = &param.cli_help {
                let help = if param.selector {
                    format!("{help}\n\n{}", render_selector_help(registry))
                } else {
                    help.clone()
                };
                lines.push("#[rustfmt::skip]".to_string());
                lines.push(format!(
                    "pub const {prefix}_{}_HELP: &str = \"{}\";",
                    tool_contracts::rust_const_name(&param.name),
                    rust_escape(&help)
                ));
            }
        }
        lines.push(String::new());
    }
    lines.join("\n")
}

fn render_selector_help(registry: &ToolContractRegistry) -> String {
    match tool_contracts::render_selector_grammar_block(registry.shared()) {
        Some(grammar) => grammar,
        None => panic!("shared.selector_grammar exists for selector CLI params"),
    }
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    if let Ok(existing) = fs::read_to_string(path)
        && existing == content
    {
        return Ok(());
    }
    fs::write(path, content)
}

fn remove_stale_generated_files(dir: &Path, expected: &HashSet<&str>) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.extension().and_then(|ext| ext.to_str()) == Some("json")
            && !expected.contains(file_name)
        {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

fn rust_escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
