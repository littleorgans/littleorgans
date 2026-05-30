mod common;

use common::OrPanic as _;
use std::process::{Command, Output};

#[test]
fn top_level_help_describes_visible_commands() {
    let stdout = help(&["--help"]);
    for description in [
        "Imperatively run a session through the session-matters daemon",
        "Create namespace and session records",
        "Manage session-matters user configuration",
        "Inspect sessions and namespaces",
        "Delete sessions and namespaces",
        "Report session-matters daemon health",
        "Send and read durable session mail",
        "Add or remove labels on selected sessions",
        "Bridge MCP stdio to the session-matters daemon",
    ] {
        assert!(
            stdout.contains(description),
            "top-level help missing {description:?}\n{stdout}"
        );
    }
}

#[test]
fn daemon_command_is_not_a_visible_surface() {
    let stdout = help(&["--help"]);
    assert!(
        !stdout
            .lines()
            .any(|line| line.trim_start().starts_with("daemon ")),
        "{stdout}"
    );
    let removed_command = ["sm", "daemon"].join(" ");
    assert!(!stdout.contains(&removed_command), "{stdout}");

    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["daemon", "--help"])
        .output()
        .or_panic("removed daemon help executes");
    assert!(
        !output.status.success(),
        "removed daemon help unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'daemon'"),
        "removed daemon help should be rejected as an unknown subcommand\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_help_describes_every_flag() {
    let stdout = help(&["run", "--help"]);
    for description in [
        "Runtime to launch",
        "Role label recorded on the session",
        "Filesystem directory used as the runtime's working directory",
        "Namespace slug for the session",
        "Session label as key=value",
        "Agent config name resolved under",
        "~/.lilo/config/session/agents/<name>/agent.toml",
        "claude_config_dir",
        "[env]",
        "Runtime isolation policy",
        "host, docker, or docker:PROFILE",
        "Container image for docker isolated runtimes",
        "Runtime target",
        "Preempt an occupied tmux pane",
    ] {
        assert!(
            stdout.contains(description),
            "run help missing {description:?}\n{stdout}"
        );
    }
}

#[test]
fn retained_leaf_commands_print_help_on_bare_invocation() {
    for args in [
        ["create", "namespace"].as_slice(),
        ["create", "session"].as_slice(),
        ["config", "set-context"].as_slice(),
        ["delete", "session"].as_slice(),
        ["delete", "namespace"].as_slice(),
        ["mail", "send"].as_slice(),
        ["mail", "read"].as_slice(),
        ["mail", "check"].as_slice(),
        ["mail", "stop-check"].as_slice(),
        ["label"].as_slice(),
        ["logs"].as_slice(),
        ["capture"].as_slice(),
        ["wait"].as_slice(),
        ["nudge"].as_slice(),
        ["run"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("sm {} executes: {error}", args.join(" ")));
        let rendered_help = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            rendered_help.contains("Usage:"),
            "sm {} did not print help\n{stdout}",
            args.join(" "),
            stdout = rendered_help
        );
        if args.len() == 1
            && matches!(
                args[0],
                "label" | "logs" | "capture" | "wait" | "nudge" | "run"
            )
        {
            let expected_usage = format!("Usage: sm {}", args[0]);
            assert!(
                rendered_help.contains(&expected_usage),
                "sm {} usage dropped parent prefix\n{stdout}",
                args.join(" "),
                stdout = rendered_help
            );
        }
    }
}

#[test]
fn full_retained_help_tree_has_no_blank_descriptions() {
    for args in retained_help_nodes() {
        let stdout = help(args);
        assert!(
            !stdout.contains("--json\n          \n")
                && !stdout.contains("<SLUG>  \n")
                && !stdout.contains("<NAMESPACE>  \n"),
            "sm {} has a blank help description\n{stdout}",
            args.join(" ")
        );
        assert!(
            stdout.contains("Usage:"),
            "sm {} does not render a usage line\n{stdout}",
            args.join(" ")
        );
    }
}

#[test]
fn label_help_describes_positionals_and_selector_grammar() {
    let stdout = help(&["label", "--help"]);

    for expected in [
        "<SELECTOR>",
        "Session selector used for matching sessions to label.",
        "Namespace scope for resolving session selectors",
        "<MUTATION>",
        "Label mutation as key=value or key-.",
        "Grammar:",
        "all",
        "<uuid>",
        "id:<uuid>",
        "Examples:",
        "019e44f9-...",
        "role:engineer",
    ] {
        assert!(
            stdout.contains(expected),
            "label help missing {expected:?}\n{stdout}"
        );
    }
    assert!(
        !stdout.contains("Grammar: all,"),
        "selector grammar should render vertically\n{stdout}"
    );
}

#[test]
fn capture_help_targets_one_session_id() {
    let stdout = help(&["capture", "--help"]);

    for expected in [
        "Usage: sm capture [OPTIONS] <SESSION_ID>",
        "<SESSION_ID>",
        "Exact session id to capture.",
        "--scrollback-lines",
        "runtime-matters uses its default capture depth",
    ] {
        assert!(
            stdout.contains(expected),
            "capture help missing {expected:?}\n{stdout}"
        );
    }
    assert!(!stdout.contains("--json"), "{stdout}");
    assert!(!stdout.contains("--selector"), "{stdout}");
    assert!(!stdout.contains("role:<name>"), "{stdout}");
}

#[test]
fn create_and_delete_resource_help_uses_current_lifecycle_copy() {
    let create = help(&["create", "--help"]);
    assert!(create.contains("Create namespace and session records"));
    assert!(create.contains("Create a namespace before running sessions into it"));
    assert!(create.contains("Declaratively create a headless session record"));

    let run = help(&["run", "--help"]);
    assert!(run.contains("Imperatively run a session through the session-matters daemon"));

    let config = help(&["config", "--help"]);
    assert!(config.contains("Set the user namespace context used by CLI commands"));

    let delete = help(&["delete", "--help"]);
    assert!(delete.contains("Terminate daemon owned sessions by selector"));
    assert!(delete.contains("Delete a namespace, terminate its sessions"));

    let create_namespace = help(&["create", "namespace", "--help"]);
    assert!(create_namespace.contains("Namespace slug to create."));

    let create_session = help(&["create", "session", "--help"]);
    assert!(create_session.contains("Runtime to launch."));
    assert!(create_session.contains("Role label recorded on the session."));

    let config_context = help(&["config", "set-context", "--help"]);
    assert!(config_context.contains("Namespace slug to use as the user context."));

    let delete_namespace = help(&["delete", "namespace", "--help"]);
    assert!(delete_namespace.contains("Namespace slug to delete."));
}

#[test]
fn get_help_collapses_resources_to_singular_with_plural_aliases() {
    let get = help(&["get", "--help"]);
    assert!(get.contains("List session records, or get one session record by id."));
    assert!(get.contains("List namespace records, or get one namespace record by slug."));
    assert!(!get.contains("List session records known to the session-matters daemon."));
    assert!(!get.contains("List namespace records\n"));

    let session = help(&["get", "sessions", "--help"]);
    assert!(session.contains("Optional session id to load instead of listing."));
    assert!(session.contains("--selector"));
    assert!(!session.contains("Render output as JSON."));
    assert!(session.contains("--show-labels"));
    assert!(session.contains("JSON output already includes labels."));

    let namespace = help(&["get", "namespaces", "--help"]);
    assert!(namespace.contains("Optional namespace slug to load instead of listing."));
    assert!(!namespace.contains("Render output as JSON."));
    assert!(!namespace.contains("--selector"));
}

#[test]
fn labels_are_not_crud_resources() {
    for args in [
        ["create", "label", "--help"].as_slice(),
        ["get", "label", "--help"].as_slice(),
        ["delete", "label", "--help"].as_slice(),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_sm"))
            .args(args)
            .output()
            .unwrap_or_else(|error| panic!("sm {} executes: {error}", args.join(" ")));
        assert!(
            !output.status.success(),
            "sm {} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn link_command_is_not_a_visible_surface() {
    let stdout = help(&["--help"]);
    assert!(!stdout.contains(" link "), "{stdout}");
    assert!(!stdout.contains("sm link"), "{stdout}");

    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(["link", "--help"])
        .output()
        .or_panic("sm link --help executes");
    assert!(
        !output.status.success(),
        "sm link --help unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unrecognized subcommand 'link'"),
        "sm link --help should be rejected as an unknown subcommand\nstderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn help(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_sm"))
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("sm {} executes: {error}", args.join(" ")));
    assert_success(&format!("sm {}", args.join(" ")), &output);
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn retained_help_nodes() -> Vec<&'static [&'static str]> {
    vec![
        &["--help"],
        &["run", "--help"],
        &["create", "--help"],
        &["create", "namespace", "--help"],
        &["create", "session", "--help"],
        &["config", "--help"],
        &["config", "set-context", "--help"],
        &["get", "--help"],
        &["get", "session", "--help"],
        &["get", "sessions", "--help"],
        &["get", "namespace", "--help"],
        &["get", "namespaces", "--help"],
        &["delete", "--help"],
        &["delete", "session", "--help"],
        &["delete", "sessions", "--help"],
        &["delete", "namespace", "--help"],
        &["doctor", "--help"],
        &["mail", "--help"],
        &["mail", "send", "--help"],
        &["mail", "read", "--help"],
        &["mail", "check", "--help"],
        &["mail", "stop-check", "--help"],
        &["label", "--help"],
        &["logs", "--help"],
        &["capture", "--help"],
        &["wait", "--help"],
        &["nudge", "--help"],
        &["mcp", "--help"],
    ]
}

fn assert_success(command: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
