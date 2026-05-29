#[test]
fn generated_lilo_surface_is_current() {
    let output = std::process::Command::new(assert_cmd::cargo::cargo_bin("xtask"))
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
