#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fs;
use std::path::Path;

#[test]
fn install_recipe_switches_to_release_install() {
    let justfile = read_workspace_justfile();

    assert!(
        justfile.contains("\ninstall: install-release\n"),
        "just install must reinstall the release binary"
    );
}

#[test]
fn install_helper_prints_installed_binary_version() {
    let justfile = read_workspace_justfile();

    assert!(
        justfile.contains("\"$dest\" --version"),
        "install helper must print the version from the installed binary"
    );
}

fn read_workspace_justfile() -> String {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let justfile = manifest_dir.join("../../..").join("justfile");
    fs::read_to_string(&justfile)
        .unwrap_or_else(|error| panic!("read {}: {error}", justfile.display()))
}
