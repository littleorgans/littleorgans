mod common;
use common::OrPanic as _;

use std::path::{Path, PathBuf};

#[test]
fn namespace_scope_applies_to_selector_consuming_cli_surfaces() {
    let fixture = scoped_sessions();

    assert_get_scope(&fixture);
    assert_namespace_selector_scope(&fixture);
    assert_mail_and_nudge_scope(&fixture);
    assert_label_and_delete_scope(&fixture);
}

struct ScopeFixture {
    daemon: common::DaemonFixture,
    alpha_dir: PathBuf,
    beta_dir: PathBuf,
    alpha_id: String,
    beta_id: String,
}

fn scoped_sessions() -> ScopeFixture {
    let runtime_path = common::fake_runtime_path("claude");
    let daemon = common::DaemonFixture::start_with_runtime_path(runtime_path.path());
    let alpha_dir = daemon.dir.path().join("alpha");
    let beta_dir = daemon.dir.path().join("beta");
    std::fs::create_dir_all(&alpha_dir).or_panic("alpha dir");
    std::fs::create_dir_all(&beta_dir).or_panic("beta dir");
    create_namespace(&daemon, "alpha");
    create_namespace(&daemon, "beta");

    let alpha_id = run_session(&daemon, "alpha", &alpha_dir);
    let beta_id = run_session(&daemon, "beta", &beta_dir);
    set_context(&daemon, "alpha");

    ScopeFixture {
        daemon,
        alpha_dir,
        beta_dir,
        alpha_id,
        beta_id,
    }
}

fn assert_get_scope(fixture: &ScopeFixture) {
    let daemon = &fixture.daemon;
    let alpha_dir = fixture.alpha_dir.as_path();
    let alpha_id = fixture.alpha_id.as_str();
    let beta_id = fixture.beta_id.as_str();

    let default_scoped = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["get", "sessions"])
        .output()
        .or_panic("sm get sessions executes");
    assert_success("sm get sessions", &default_scoped);
    assert_contains_only(&default_scoped, alpha_id, beta_id);

    let explicit_namespace = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["get", "sessions", "--namespace", "beta"])
        .output()
        .or_panic("sm get sessions --namespace executes");
    assert_success("sm get sessions --namespace beta", &explicit_namespace);
    assert_contains_only(&explicit_namespace, beta_id, alpha_id);

    let all_namespaces = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["get", "sessions", "-A"])
        .output()
        .or_panic("sm get sessions -A executes");
    assert_success("sm get sessions -A", &all_namespaces);
    assert_contains(&all_namespaces, alpha_id);
    assert_contains(&all_namespaces, beta_id);

    let namespace_selector = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["get", "sessions", "--selector", "namespace:beta", "-A"])
        .output()
        .or_panic("sm get sessions namespace selector executes");
    assert_success(
        "sm get sessions --selector namespace:beta -A",
        &namespace_selector,
    );
    assert_contains_only(&namespace_selector, beta_id, alpha_id);
}

fn assert_namespace_selector_scope(fixture: &ScopeFixture) {
    let daemon = &fixture.daemon;
    let alpha_dir = fixture.alpha_dir.as_path();
    let beta_dir = fixture.beta_dir.as_path();
    let beta_id = fixture.beta_id.as_str();
    let alpha_id = fixture.alpha_id.as_str();

    let namespace_selector_default_scope = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["nudge", "--to", "namespace:beta", "--content", "ping"])
        .output()
        .or_panic("sm nudge namespace selector executes");
    assert_success(
        "sm nudge --to namespace:beta",
        &namespace_selector_default_scope,
    );
    assert_total_line_count(&namespace_selector_default_scope, 1);
    assert_contains_only(&namespace_selector_default_scope, beta_id, alpha_id);

    let matching_namespace_flag = daemon
        .command()
        .current_dir(alpha_dir)
        .args([
            "nudge",
            "--to",
            "namespace:beta",
            "--namespace",
            "beta",
            "--content",
            "ping",
        ])
        .output()
        .or_panic("sm nudge matching namespace flag executes");
    assert_success(
        "sm nudge --to namespace:beta --namespace beta",
        &matching_namespace_flag,
    );
    assert_contains_only(&matching_namespace_flag, beta_id, alpha_id);

    let conflicting_namespace_flag = daemon
        .command()
        .current_dir(alpha_dir)
        .args([
            "nudge",
            "--to",
            "namespace:beta",
            "--namespace",
            "alpha",
            "--content",
            "ping",
        ])
        .output()
        .or_panic("sm nudge conflicting namespace flag executes");
    assert!(!conflicting_namespace_flag.status.success());
    assert!(stderr(&conflicting_namespace_flag).contains("namespace conflict"));
    assert!(stderr(&conflicting_namespace_flag).contains("alpha"));
    assert!(stderr(&conflicting_namespace_flag).contains("namespace:beta"));

    let beta_dir_selector = format!("dir:{}", canonical(beta_dir));
    let dir_selector = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["get", "sessions", "--selector", &beta_dir_selector, "-A"])
        .output()
        .or_panic("sm get sessions dir selector executes");
    assert_success("sm get sessions --selector dir -A", &dir_selector);
    assert_contains_only(&dir_selector, beta_id, alpha_id);
}

fn assert_mail_and_nudge_scope(fixture: &ScopeFixture) {
    let daemon = &fixture.daemon;
    let alpha_dir = fixture.alpha_dir.as_path();
    let beta_id = fixture.beta_id.as_str();
    let alpha_id = fixture.alpha_id.as_str();

    let mail_default = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["mail", "send", "--to", "all", "--content", "scoped"])
        .output()
        .or_panic("sm mail send executes");
    assert_success("sm mail send", &mail_default);
    assert_line_count(&mail_default.stdout, 1);

    let mail_all = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["mail", "send", "--to", "all", "--content", "all", "-A"])
        .output()
        .or_panic("sm mail send -A executes");
    assert_success("sm mail send -A", &mail_all);
    assert_line_count(&mail_all.stdout, 2);

    let nudge_default = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["nudge", "--to", "all", "--content", "ping"])
        .output()
        .or_panic("sm nudge executes");
    assert_success("sm nudge", &nudge_default);
    assert_total_line_count(&nudge_default, 1);

    let nudge_flag_scope = daemon
        .command()
        .current_dir(alpha_dir)
        .args([
            "nudge",
            "--to",
            "role:engineer",
            "--namespace",
            "beta",
            "--content",
            "ping",
        ])
        .output()
        .or_panic("sm nudge --namespace executes");
    assert_success("sm nudge --namespace beta", &nudge_flag_scope);
    assert_contains_only(&nudge_flag_scope, beta_id, alpha_id);

    let nudge_all = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["nudge", "--to", "all", "--content", "ping", "-A"])
        .output()
        .or_panic("sm nudge -A executes");
    assert_success("sm nudge -A", &nudge_all);
    assert_total_line_count(&nudge_all, 2);
}

fn assert_label_and_delete_scope(fixture: &ScopeFixture) {
    let daemon = &fixture.daemon;
    let alpha_dir = fixture.alpha_dir.as_path();
    let alpha_id = fixture.alpha_id.as_str();
    let beta_id = fixture.beta_id.as_str();

    let labeled = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["label", "all", "scope=alpha"])
        .output()
        .or_panic("sm label executes");
    assert_success("sm label", &labeled);
    assert_contains_only(&labeled, alpha_id, beta_id);

    let deleted = daemon
        .command()
        .current_dir(alpha_dir)
        .args(["delete", "session", "all", "--namespace", "beta"])
        .output()
        .or_panic("sm delete executes");
    assert_success("sm delete --namespace beta", &deleted);
    assert_contains_only(&deleted, beta_id, alpha_id);
}

#[test]
fn legacy_workspace_selector_is_rejected_by_cli() {
    let daemon = common::DaemonFixture::start();

    let selected = daemon
        .command()
        .args(["get", "sessions", "--selector", "workspace:test", "-A"])
        .output()
        .or_panic("sm get sessions executes");

    assert!(!selected.status.success());
    assert!(stderr(&selected).contains("unsupported selector"));
}

fn create_namespace(daemon: &common::DaemonFixture, namespace: &str) {
    let output = daemon
        .command()
        .args(["create", "namespace", namespace])
        .output()
        .or_panic("sm create namespace executes");
    assert_success("sm create namespace", &output);
}

fn set_context(daemon: &common::DaemonFixture, namespace: &str) {
    let output = daemon
        .command()
        .args(["config", "set-context", namespace])
        .output()
        .or_panic("sm config set-context executes");
    assert_success("sm config set-context", &output);
}

fn run_session(daemon: &common::DaemonFixture, namespace: &str, dir: &Path) -> String {
    let output = daemon
        .command()
        .args([
            "run",
            "claude",
            "--role",
            "engineer",
            "--dir",
            &dir.display().to_string(),
            "--namespace",
            namespace,
            "--detach",
        ])
        .output()
        .or_panic("sm run executes");
    assert_success("sm run", &output);
    first_field(&output.stdout)
}

fn assert_success(command: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{command} failed\nstdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn assert_contains_only(output: &std::process::Output, present: &str, absent: &str) {
    assert_contains(output, present);
    assert!(
        !stdout(output).contains(absent),
        "expected output to omit {absent}\nstdout:\n{}",
        stdout(output)
    );
}

fn assert_contains(output: &std::process::Output, needle: &str) {
    assert!(
        stdout(output).contains(needle),
        "expected output to contain {needle}\nstdout:\n{}",
        stdout(output)
    );
}

fn assert_line_count(bytes: &[u8], expected: usize) {
    let lines = String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    assert_eq!(lines, expected);
}

fn assert_total_line_count(output: &std::process::Output, expected: usize) {
    let lines = [output.stdout.as_slice(), output.stderr.as_slice()]
        .into_iter()
        .map(nonempty_line_count)
        .sum::<usize>();
    assert_eq!(
        lines,
        expected,
        "stdout:\n{}\nstderr:\n{}",
        stdout(output),
        stderr(output)
    );
}

fn nonempty_line_count(bytes: &[u8]) -> usize {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count()
}

fn first_field(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout)
        .split_whitespace()
        .next()
        .or_panic("stdout has first field")
        .to_string()
}

fn canonical(path: &Path) -> String {
    std::fs::canonicalize(path)
        .or_panic("path canonicalizes")
        .display()
        .to_string()
}

fn stdout(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).to_string()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).to_string()
}
