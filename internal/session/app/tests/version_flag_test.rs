mod common;

use common::OrPanic as _;

#[test]
fn root_version_flag_prints_session_matters_package_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sm"))
        .arg("--version")
        .output()
        .or_panic("sm --version");

    assert!(output.status.success(), "sm --version failed: {output:?}");
    assert!(output.stderr.is_empty(), "stderr was not empty: {output:?}");

    let stdout = String::from_utf8(output.stdout).or_panic("version output utf8");
    let expected = format!("session-matters {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(stdout, expected);
}
