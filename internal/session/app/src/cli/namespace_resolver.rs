use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lilo_session_core::{Namespace, NamespaceError, SmPaths, SmPathsError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamespaceResolution {
    pub namespace: Namespace,
    pub canonical_dir: PathBuf,
    pub warnings: Vec<NamespaceResolutionWarning>,
}

impl NamespaceResolution {
    pub fn into_pair(self) -> (Namespace, PathBuf) {
        (self.namespace, self.canonical_dir)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamespaceResolutionWarning {
    MissingOrInvalidHome,
}

#[derive(Debug, thiserror::Error)]
pub enum NamespaceResolutionError {
    #[error("invalid namespace value: {source}")]
    InvalidEnv {
        #[source]
        source: NamespaceError,
    },
    #[error("failed to read namespace binding {path}: {source}")]
    ReadBinding {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid namespace binding {path}: {source}")]
    InvalidBinding {
        path: PathBuf,
        #[source]
        source: NamespaceError,
    },
    #[error("failed to canonicalize namespace directory {path}: {source}")]
    CanonicalizeDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

pub fn resolve_namespace_dir(
    start_dir: impl AsRef<Path>,
    explicit_namespace: Option<Namespace>,
) -> Result<NamespaceResolution, NamespaceResolutionError> {
    resolve_namespace_dir_with_paths(
        start_dir.as_ref(),
        explicit_namespace,
        None,
        SmPaths::from_env(),
    )
}

fn resolve_namespace_dir_with_paths(
    start_dir: &Path,
    explicit_namespace: Option<Namespace>,
    env_namespace: Option<OsString>,
    paths: Result<SmPaths, SmPathsError>,
) -> Result<NamespaceResolution, NamespaceResolutionError> {
    let (paths, warnings) = match paths {
        Ok(paths) => (Some(paths), Vec::new()),
        Err(SmPathsError::MissingHome | SmPathsError::EmptyPath) => {
            (None, vec![NamespaceResolutionWarning::MissingOrInvalidHome])
        }
    };
    // Resolver output is a point-in-time snapshot. Mutating downstream commands
    // must revalidate against daemon state before committing side effects.
    let namespace = resolve_namespace(explicit_namespace, env_namespace, paths.as_ref())?;

    Ok(NamespaceResolution {
        namespace,
        canonical_dir: canonical_dir(start_dir)?,
        warnings,
    })
}

fn resolve_namespace(
    explicit_namespace: Option<Namespace>,
    env_namespace: Option<OsString>,
    paths: Option<&SmPaths>,
) -> Result<Namespace, NamespaceResolutionError> {
    if let Some(namespace) = explicit_namespace {
        return Ok(namespace);
    }

    if let Some(raw) = env_namespace.filter(|value| !value.is_empty()) {
        let raw = raw.to_string_lossy();
        return Namespace::from_str(raw.trim())
            .map_err(|source| NamespaceResolutionError::InvalidEnv { source });
    }

    if let Some(paths) = paths {
        let binding = paths.namespace_binding();
        let binding_exists =
            binding
                .try_exists()
                .map_err(|source| NamespaceResolutionError::ReadBinding {
                    path: binding.clone(),
                    source,
                })?;
        if binding_exists {
            let raw = std::fs::read_to_string(&binding).map_err(|source| {
                NamespaceResolutionError::ReadBinding {
                    path: binding.clone(),
                    source,
                }
            })?;
            return Namespace::from_str(raw.trim()).map_err(|source| {
                NamespaceResolutionError::InvalidBinding {
                    path: binding,
                    source,
                }
            });
        }
    }

    Ok(Namespace::default())
}

fn canonical_dir(dir: &Path) -> Result<PathBuf, NamespaceResolutionError> {
    std::fs::canonicalize(dir).map_err(|source| NamespaceResolutionError::CanonicalizeDir {
        path: dir.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{ErrOrPanic as _, OrPanic as _};

    #[test]
    fn falls_back_to_default_when_no_config_found() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");

        let resolved =
            resolve_with_paths(&start, None, Some(SmPaths::new(root.path().join(".sm"))));

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    #[test]
    fn reads_user_namespace_binding() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).or_panic("project dir");
        let paths = SmPaths::new(root.path().join("sm-home"));
        write_binding(&paths, "alpha");

        let resolved = resolve_with_paths(&project, None, Some(paths));

        assert_eq!(resolved.namespace.as_str(), "alpha");
        assert_eq!(resolved.canonical_dir, canonical(&project));
    }

    #[test]
    fn env_namespace_overrides_user_binding() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");
        let paths = SmPaths::new(root.path().join("sm-home"));
        write_binding(&paths, "bound");

        let resolved = resolve_with_paths(&start, Some("env-ns"), Some(paths));

        assert_eq!(resolved.namespace.as_str(), "env-ns");
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    #[test]
    fn explicit_namespace_overrides_env_and_user_binding() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");
        let paths = SmPaths::new(root.path().join("sm-home"));
        write_binding(&paths, "bound");

        let resolved = resolve_namespace_dir_with_paths(
            &start,
            Some(Namespace::new("explicit").or_panic("namespace")),
            Some(OsString::from("env-ns")),
            Ok(paths),
        )
        .or_panic("resolves");

        assert_eq!(resolved.namespace.as_str(), "explicit");
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    #[test]
    fn workspace_marker_at_start_dir_is_ignored() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");
        write_workspace_marker(&start, "marker");

        let resolved =
            resolve_with_paths(&start, None, Some(SmPaths::new(root.path().join(".sm"))));

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    #[test]
    fn workspace_marker_at_ancestor_is_ignored() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let project = root.path().join("project");
        let nested = project.join("src/bin");
        std::fs::create_dir_all(&nested).or_panic("nested dirs");
        write_workspace_marker(&project, "ancestor");

        let resolved =
            resolve_with_paths(&nested, None, Some(SmPaths::new(root.path().join(".sm"))));

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(resolved.canonical_dir, canonical(&nested));
    }

    #[test]
    fn symlink_return_dir_is_canonical() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let real = root.path().join("real");
        let link = root.path().join("link");
        let start = link.join("child");
        std::fs::create_dir_all(real.join("child")).or_panic("real child");
        symlink_dir(&real, &link);

        let resolved =
            resolve_with_paths(&start, None, Some(SmPaths::new(root.path().join(".sm"))));

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            resolved.canonical_dir,
            canonical(real.join("child").as_path())
        );
    }

    #[test]
    fn invalid_env_namespace_fails_loudly() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");

        let error = resolve_error(
            &start,
            Some("Alpha"),
            Some(SmPaths::new(root.path().join(".sm"))),
        );

        assert!(matches!(error, NamespaceResolutionError::InvalidEnv { .. }));
        assert!(error.to_string().contains("invalid namespace value"));
    }

    #[test]
    fn invalid_binding_content_fails_loudly() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");
        let paths = SmPaths::new(root.path().join("sm-home"));
        write_binding(&paths, "Alpha");

        let error = resolve_error(&start, None, Some(paths));

        assert!(matches!(
            error,
            NamespaceResolutionError::InvalidBinding { .. }
        ));
        assert!(error.to_string().contains("invalid namespace binding"));
    }

    #[cfg(unix)]
    #[test]
    fn unreadable_binding_fails_loudly() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");
        let paths = SmPaths::new(root.path().join("sm-home"));
        let binding = write_binding(&paths, "alpha");
        remove_read_permissions(&binding);

        let error = resolve_error(&start, None, Some(paths));

        restore_read_permissions(&binding);
        assert!(matches!(
            error,
            NamespaceResolutionError::ReadBinding { .. }
        ));
    }

    #[test]
    fn missing_or_invalid_home_warns_and_falls_back_to_default() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");

        let resolved =
            resolve_namespace_dir_with_paths(&start, None, None, Err(SmPathsError::MissingHome))
                .or_panic("resolves without home");

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            resolved.warnings,
            vec![NamespaceResolutionWarning::MissingOrInvalidHome]
        );
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    #[test]
    fn empty_home_warns_and_falls_back_to_default() {
        let root = tempfile::tempdir().or_panic("tempdir");
        let start = root.path().join("project");
        std::fs::create_dir_all(&start).or_panic("start dir");

        let resolved =
            resolve_namespace_dir_with_paths(&start, None, None, Err(SmPathsError::EmptyPath))
                .or_panic("resolves without valid home");

        assert_eq!(resolved.namespace, Namespace::default());
        assert_eq!(
            resolved.warnings,
            vec![NamespaceResolutionWarning::MissingOrInvalidHome]
        );
        assert_eq!(resolved.canonical_dir, canonical(&start));
    }

    fn resolve_with_paths(
        start: &Path,
        env_namespace: Option<&str>,
        paths: Option<SmPaths>,
    ) -> NamespaceResolution {
        resolve_namespace_dir_with_paths(
            start,
            None,
            env_namespace.map(OsString::from),
            paths.ok_or(SmPathsError::MissingHome),
        )
        .or_panic("namespace resolves")
    }

    fn resolve_error(
        start: &Path,
        env_namespace: Option<&str>,
        paths: Option<SmPaths>,
    ) -> NamespaceResolutionError {
        resolve_namespace_dir_with_paths(
            start,
            None,
            env_namespace.map(OsString::from),
            paths.ok_or(SmPathsError::MissingHome),
        )
        .err_or_panic("namespace fails")
    }

    fn write_binding(paths: &SmPaths, value: &str) -> PathBuf {
        let binding = paths.namespace_binding();
        std::fs::create_dir_all(binding.parent().or_panic("binding parent"))
            .or_panic("binding dir");
        std::fs::write(&binding, value).or_panic("binding write");
        binding
    }

    fn write_workspace_marker(dir: &Path, value: &str) -> PathBuf {
        let marker = dir.join(".sm").join("namespace");
        std::fs::create_dir_all(marker.parent().or_panic("marker parent")).or_panic("marker dir");
        std::fs::write(&marker, value).or_panic("marker write");
        marker
    }

    fn canonical(path: &Path) -> PathBuf {
        std::fs::canonicalize(path).or_panic("canonical path")
    }

    #[cfg(unix)]
    fn symlink_dir(original: &Path, link: &Path) {
        std::os::unix::fs::symlink(original, link).or_panic("symlink dir");
    }

    #[cfg(windows)]
    fn symlink_dir(original: &Path, link: &Path) {
        std::os::windows::fs::symlink_dir(original, link).or_panic("symlink dir");
    }

    #[cfg(unix)]
    fn remove_read_permissions(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .or_panic("binding metadata")
            .permissions();
        permissions.set_mode(0o000);
        std::fs::set_permissions(path, permissions).or_panic("remove read permissions");
    }

    #[cfg(unix)]
    fn restore_read_permissions(path: &Path) {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .or_panic("binding metadata")
            .permissions();
        permissions.set_mode(0o600);
        std::fs::set_permissions(path, permissions).or_panic("restore read permissions");
    }
}
