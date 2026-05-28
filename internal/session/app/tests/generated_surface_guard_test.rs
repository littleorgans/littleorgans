mod common;

use common::OrPanic as _;
use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn generated_help_surface_has_no_agent_help_constants() {
    for path in tool_source_paths() {
        assert_absent(&path, "AGENT_");
    }
    assert_absent(
        &app_root().join("src/cli/generated_help.rs"),
        "pub const AGENT_",
    );
}

#[test]
fn generated_docs_do_not_reference_removed_cli_forms() {
    let forbidden = [
        "sm get agent",
        "sm get agents",
        "sm delete agent",
        "sm init namespace",
        ".sm/namespace",
    ];
    for path in removed_surface_guard_paths() {
        for token in forbidden {
            assert_absent(&path, token);
        }
    }
}

#[test]
fn selector_help_sources_match_cli_shape_matrix() {
    let cases = [
        (
            "tools/session.toml",
            "[tools.session_list]",
            "cli_flag        = \"--selector\"",
        ),
        (
            "tools/session.toml",
            "[tools.session_delete]",
            "cli_flag        = \"selector\"",
        ),
        (
            "tools/label.toml",
            "[tools.session_label]",
            "cli_flag        = \"selector\"",
        ),
        (
            "tools/logs.toml",
            "[tools.logs]",
            "cli_flag        = \"selector\"",
        ),
        (
            "tools/wait.toml",
            "[tools.wait]",
            "cli_flag        = \"selector\"",
        ),
        (
            "tools/capture.toml",
            "[tools.session_capture]",
            "name            = \"id\"",
        ),
        (
            "tools/mail.toml",
            "[tools.mail_send]",
            "cli_flag        = \"--to\"",
        ),
        (
            "tools/mail.toml",
            "[tools.mail_read]",
            "cli_flag        = \"--selector\"",
        ),
        (
            "tools/mail.toml",
            "[tools.mail_check]",
            "cli_flag        = \"--selector\"",
        ),
        (
            "tools/mail.toml",
            "[tools.mail_stop_check]",
            "cli_flag        = \"--selector\"",
        ),
        (
            "tools/nudge.toml",
            "[tools.nudge]",
            "cli_flag        = \"--to\"",
        ),
    ];
    for (path, table, flag) in cases {
        let content = read_repo_file(path);
        let table_body = content
            .split(table)
            .nth(1)
            .unwrap_or_else(|| panic!("{path} missing table {table}"));
        assert!(
            table_body.contains(flag),
            "{path} {table} missing expected selector shape {flag}"
        );
    }
}

#[test]
fn generated_help_constants_have_source_consumers() {
    let constants = generated_help_constants();
    let source = consumer_source();

    for constant in constants {
        let reference = format!("generated_help::{constant}");
        assert!(
            source.contains(&reference),
            "generated_help constant {constant} has no source consumer"
        );
    }
}

fn removed_surface_guard_paths() -> Vec<PathBuf> {
    let mut paths = app_paths([
        "src/tool_docs.rs",
        "README.md",
        "src/mcp/generated_instructions.rs",
    ]);
    paths.extend(generated_schema_paths());
    paths
}

fn generated_help_constants() -> Vec<String> {
    let generated_help = read_app_file("src/cli/generated_help.rs");
    generated_help
        .lines()
        .filter_map(|line| line.strip_prefix("pub const "))
        .map(|line| {
            line.split_once(':')
                .unwrap_or_else(|| panic!("generated help const line has type separator: {line}"))
                .0
                .to_string()
        })
        .collect()
}

fn consumer_source() -> String {
    consumer_source_paths()
        .into_iter()
        .filter(|path| !path.ends_with("src/cli/generated_help.rs"))
        .map(|path| read_file(&path))
        .collect::<Vec<_>>()
        .join("\n")
}

fn consumer_source_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    collect_rust_sources(&app_root().join("src/cli"), &mut paths);
    collect_rust_sources(&app_root().join("src/mcp"), &mut paths);
    paths
}

fn collect_rust_sources(dir: &Path, paths: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
    {
        let path = entry.or_panic("source entry reads").path();
        if path.is_dir() {
            collect_rust_sources(&path, paths);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            paths.push(path);
        }
    }
}

fn tool_source_paths() -> Vec<PathBuf> {
    let tools_dir = app_root().join("tools");
    fs::read_dir(&tools_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", tools_dir.display()))
        .map(|entry| entry.or_panic("tool source entry reads").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect()
}

fn generated_schema_paths() -> Vec<PathBuf> {
    let schema_dir = app_root().join("src/mcp/generated_schema");
    fs::read_dir(&schema_dir)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", schema_dir.display()))
        .map(|entry| entry.or_panic("generated schema entry reads").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect()
}

fn app_paths<const N: usize>(paths: [&str; N]) -> Vec<PathBuf> {
    paths
        .into_iter()
        .map(|path| app_root().join(path))
        .collect()
}

fn app_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn assert_absent(path: &Path, token: &str) {
    let content = read_file(path);
    assert!(
        !content.contains(token),
        "{} contains forbidden token {token:?}",
        path.display()
    );
}

fn read_repo_file(path: &str) -> String {
    read_app_file(path)
}

fn read_app_file(path: &str) -> String {
    read_file(&app_root().join(path))
}

fn read_file(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
}
