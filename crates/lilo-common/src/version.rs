pub const VERSION_STRING: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_string_comes_from_workspace_package_version() {
        assert!(VERSION_STRING.starts_with("0.8.0"));
    }
}
