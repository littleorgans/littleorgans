#[test]
fn root_version_flag_prints_littleorgans_package_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_lilo"))
        .arg("--version")
        .output()
        .expect("lilo --version");

    assert!(output.status.success(), "lilo --version failed: {output:?}");
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");

    let stdout = String::from_utf8(output.stdout).expect("version output utf8");
    let expected = format!("littleorgans {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(stdout, expected);
}
