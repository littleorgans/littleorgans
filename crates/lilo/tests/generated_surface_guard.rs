#[test]
fn generated_lilo_surface_is_current() {
    let mut command = xtask_command();
    let output = command
        .args(["codegen", "--check"])
        .output()
        .expect("xtask codegen --check");

    assert!(
        output.status.success(),
        "xtask codegen --check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn xtask_command() -> std::process::Command {
    if let Some(path) = std::env::var_os("CARGO_BIN_EXE_xtask") {
        return std::process::Command::new(path);
    }
    let mut command = std::process::Command::new(env!("CARGO"));
    command.args(["run", "-p", "xtask", "--"]);
    command
}
