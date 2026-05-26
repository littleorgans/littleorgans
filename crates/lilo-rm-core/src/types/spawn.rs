use std::fmt::{Display, Formatter};
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

use crate::{IsolationPolicy, LaunchEnv, RuntimeKind, RuntimeSignal, ShellResume};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TmuxAddress {
    pub session: String,
    pub window: u32,
    pub pane: u32,
}

impl Display for TmuxAddress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}.{}", self.session, self.window, self.pane)
    }
}

impl FromStr for TmuxAddress {
    type Err = TmuxAddressParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (session, pane_target) = value
            .rsplit_once(':')
            .ok_or_else(|| TmuxAddressParseError(value.to_owned()))?;
        let (window, pane) = pane_target
            .split_once('.')
            .ok_or_else(|| TmuxAddressParseError(value.to_owned()))?;
        if session.is_empty() {
            return Err(TmuxAddressParseError(value.to_owned()));
        }

        Ok(Self {
            session: session.to_owned(),
            window: window
                .parse()
                .map_err(|_| TmuxAddressParseError(value.to_owned()))?,
            pane: pane
                .parse()
                .map_err(|_| TmuxAddressParseError(value.to_owned()))?,
        })
    }
}

impl Serialize for TmuxAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TmuxAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid tmux pane target {0}")]
pub struct TmuxAddressParseError(pub String);

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("invalid spawn target {0}; expected headless or tmux:<session>:<window>.<pane>")]
pub struct SpawnTargetParseError(pub String);

/// Shared mount shape for spawn requests.
///
/// See this type's [`std::str::FromStr`] implementation for accepted syntax,
/// access mode defaults, and host source expansion behavior.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MountSpec {
    /// Host source path after parser expansion.
    pub source: PathBuf,
    /// Container target path. The parser keeps this path literal.
    pub target: PathBuf,
    /// Whether the mount is read only. The parser defaults this to `true`.
    #[serde(default, skip_serializing_if = "is_false")]
    pub read_only: bool,
}

/// Error returned when a mount value cannot be parsed.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MountSpecParseError {
    /// The value did not contain the required separator between host source and
    /// container target.
    #[error("mount value is missing ':' between host source and container target")]
    MissingSeparator,
    /// The host source field was empty.
    #[error("mount host source cannot be empty")]
    EmptySource,
    /// The container target field was empty.
    #[error("mount container target cannot be empty")]
    EmptyTarget,
    /// The access mode was neither `ro` nor `rw`, or another colon separated
    /// field followed the mode.
    #[error("unknown mount access mode {mode}; expected ro or rw")]
    UnknownMode { mode: String },
    /// The host source requested home expansion, but `HOME` was not set.
    #[error("mount source uses '~' but HOME is not set")]
    MissingHome,
}

/// Parses the public mount syntax used by CLI consumers.
///
/// Accepted values use `HOST:CONTAINER[:ro|:rw]`. The first field is the host
/// source, the second is the container target, and the optional third field is
/// the access mode. When the access mode is omitted, the mount is read only.
/// `ro` maps to [`MountSpec::read_only`] as `true`; `rw` maps to `false`.
///
/// Source paths are expanded only for the host side. A source of `~` expands to
/// `$HOME`, and `~/sub` expands below `$HOME`. A source that starts with another
/// tilde form, such as `~foo`, is joined to `$HOME` after dropping the leading
/// `~`; this existing fallback is preserved for compatibility. Container
/// targets are kept literal, so a target starting with `~` is not expanded.
///
/// Values with four or more colon separated fields are rejected as
/// [`MountSpecParseError::UnknownMode`]. This parser only decodes the shared
/// mount shape. Host isolation checks, such as rejecting paths outside an
/// allowed workspace, are enforced by CLI consumers before spawn submission.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// use lilo_rm_core::MountSpec;
///
/// let mount = "/host/config:/container/config:rw"
///     .parse::<MountSpec>()
///     .expect("mount spec should parse");
///
/// assert_eq!(mount.source, PathBuf::from("/host/config"));
/// assert_eq!(mount.target, PathBuf::from("/container/config"));
/// assert!(!mount.read_only);
/// ```
impl FromStr for MountSpec {
    type Err = MountSpecParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        let source = parts.next().unwrap_or_default();
        let Some(target) = parts.next() else {
            return Err(MountSpecParseError::MissingSeparator);
        };
        let mode = parts.next();
        if let Some(extra) = parts.next() {
            return Err(MountSpecParseError::UnknownMode {
                mode: extra.to_owned(),
            });
        }
        if source.is_empty() {
            return Err(MountSpecParseError::EmptySource);
        }
        if target.is_empty() {
            return Err(MountSpecParseError::EmptyTarget);
        }
        let read_only = match mode {
            None | Some("ro") => true,
            Some("rw") => false,
            Some(other) => {
                return Err(MountSpecParseError::UnknownMode {
                    mode: other.to_owned(),
                });
            }
        };

        Ok(Self {
            source: expand_mount_source(source)?,
            target: PathBuf::from(target),
            read_only,
        })
    }
}

/// Expands the host source field for a [`MountSpec`] parser input.
///
/// This helper follows the host source expansion behavior described on the
/// [`std::str::FromStr`] implementation for [`MountSpec`]. It does not handle
/// container targets because those remain literal.
pub fn expand_mount_source(source: &str) -> Result<PathBuf, MountSpecParseError> {
    if !source.starts_with('~') {
        return Ok(PathBuf::from(source));
    }

    let home = std::env::var_os("HOME").ok_or(MountSpecParseError::MissingHome)?;
    let home = PathBuf::from(home);
    if source == "~" {
        return Ok(home);
    }

    let rest = source.strip_prefix("~/").unwrap_or(&source[1..]);
    Ok(home.join(rest))
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SpawnRequest {
    pub session_id: Uuid,
    pub runtime: RuntimeKind,
    #[serde(default)]
    pub isolation: IsolationPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default)]
    pub env: Vec<LaunchEnv>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mounts: Vec<MountSpec>,
    pub cwd: PathBuf,
    pub target: SpawnTarget,
    #[serde(default, skip_serializing_if = "is_false")]
    pub force: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_resume: Option<ShellResume>,
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if predicates receive borrowed field values"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum SpawnTarget {
    Tmux(TmuxSpawnTarget),
    Headless(HeadlessSpawnTarget),
}

impl SpawnTarget {
    pub fn tmux_address(&self) -> Option<&TmuxAddress> {
        match self {
            Self::Tmux(target) => Some(&target.address),
            Self::Headless(_) => None,
        }
    }
}

impl FromStr for SpawnTarget {
    type Err = SpawnTargetParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value == "headless" {
            return Ok(Self::Headless(HeadlessSpawnTarget {}));
        }

        let Some(address) = value.strip_prefix("tmux:") else {
            return Err(SpawnTargetParseError(value.to_owned()));
        };
        let address = address
            .parse()
            .map_err(|_| SpawnTargetParseError(value.to_owned()))?;
        Ok(Self::Tmux(TmuxSpawnTarget { address }))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TmuxSpawnTarget {
    pub address: TmuxAddress,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HeadlessSpawnTarget {}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct KillRequest {
    pub session_id: Uuid,
    pub signal: RuntimeSignal,
    pub grace_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_pane_round_trips_as_target_string() {
        let pane: TmuxAddress = "test:0.1".parse().expect("pane");

        assert_eq!(pane.session, "test");
        assert_eq!(pane.window, 0);
        assert_eq!(pane.pane, 1);
        assert_eq!(pane.to_string(), "test:0.1");
        assert_eq!(serde_json::to_string(&pane).expect("json"), "\"test:0.1\"");

        let restored: TmuxAddress = serde_json::from_str("\"test:0.1\"").expect("restored");
        assert_eq!(restored, pane);
    }

    #[test]
    fn tmux_pane_rejects_malformed_targets() {
        for value in ["", "test", "test:window.0", "test:0", "test:0.pane"] {
            assert!(
                value.parse::<TmuxAddress>().is_err(),
                "accepted malformed pane target {value}"
            );
        }
    }

    #[test]
    fn spawn_target_parses_headless_and_tmux() {
        assert_eq!(
            "headless".parse::<SpawnTarget>().expect("headless target"),
            SpawnTarget::Headless(HeadlessSpawnTarget {})
        );
        assert_eq!(
            "tmux:test:0.1".parse::<SpawnTarget>().expect("tmux target"),
            SpawnTarget::Tmux(TmuxSpawnTarget {
                address: TmuxAddress {
                    session: "test".to_owned(),
                    window: 0,
                    pane: 1,
                },
            })
        );
    }

    #[test]
    fn spawn_target_rejects_missing_mode() {
        for value in ["", "test:0.1", "tmux:", "tmux:test", "other:test:0.1"] {
            assert!(
                value.parse::<SpawnTarget>().is_err(),
                "accepted malformed spawn target {value}"
            );
        }
    }

    #[test]
    fn mount_spec_round_trips_bind_paths_and_mode() {
        let mount = MountSpec {
            source: "/host/config".into(),
            target: "/container/config".into(),
            read_only: true,
        };

        let value = serde_json::to_value(&mount).expect("serialize");
        assert_eq!(
            value,
            serde_json::json!({
                "source": "/host/config",
                "target": "/container/config",
                "read_only": true
            })
        );

        let restored: MountSpec = serde_json::from_value(value).expect("deserialize");
        assert_eq!(restored, mount);
    }

    #[test]
    fn mount_spec_parses_default_read_only() {
        let mount: MountSpec = "/host/config:/container/config".parse().expect("mount");

        assert_eq!(mount.source, PathBuf::from("/host/config"));
        assert_eq!(mount.target, PathBuf::from("/container/config"));
        assert!(mount.read_only);
    }

    #[test]
    fn mount_spec_parses_explicit_read_only() {
        let mount: MountSpec = "/host/config:/container/config:ro".parse().expect("mount");

        assert_eq!(mount.source, PathBuf::from("/host/config"));
        assert_eq!(mount.target, PathBuf::from("/container/config"));
        assert!(mount.read_only);
    }

    #[test]
    fn mount_spec_parses_explicit_read_write() {
        let mount: MountSpec = "/host/config:/container/config:rw".parse().expect("mount");

        assert_eq!(mount.source, PathBuf::from("/host/config"));
        assert_eq!(mount.target, PathBuf::from("/container/config"));
        assert!(!mount.read_only);
    }

    #[test]
    fn mount_source_expands_tilde() {
        let home = std::env::var_os("HOME").expect("HOME");

        assert_eq!(
            expand_mount_source("~").expect("source"),
            PathBuf::from(home)
        );
    }

    #[test]
    fn mount_source_expands_tilde_subpath() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));

        assert_eq!(
            expand_mount_source("~/config").expect("source"),
            home.join("config")
        );
    }

    #[test]
    fn mount_source_expands_tilde_prefix() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));

        assert_eq!(
            expand_mount_source("~foo").expect("source"),
            home.join("foo")
        );
    }

    #[test]
    fn mount_spec_rejects_missing_separator() {
        let error = "missing-separator"
            .parse::<MountSpec>()
            .expect_err("mount without separator");

        assert_eq!(
            error.to_string(),
            "mount value is missing ':' between host source and container target"
        );
    }

    #[test]
    fn mount_spec_rejects_empty_parts() {
        let empty_source = ":/container".parse::<MountSpec>().expect_err("source");
        let empty_target = "/host:".parse::<MountSpec>().expect_err("target");

        assert_eq!(
            empty_source.to_string(),
            "mount host source cannot be empty"
        );
        assert_eq!(
            empty_target.to_string(),
            "mount container target cannot be empty"
        );
    }

    #[test]
    fn mount_spec_rejects_unknown_modes() {
        let unknown = "/host:/container:bad"
            .parse::<MountSpec>()
            .expect_err("unknown mode");
        let overflow = "/host:/container:ro:extra"
            .parse::<MountSpec>()
            .expect_err("extra mode");

        assert_eq!(
            unknown.to_string(),
            "unknown mount access mode bad; expected ro or rw"
        );
        assert_eq!(
            overflow.to_string(),
            "unknown mount access mode extra; expected ro or rw"
        );
    }

    #[test]
    fn mount_spec_parse_error_supports_cli_value_parser_bounds() {
        fn assert_bounds<T: std::error::Error + Send + Sync + 'static>() {}

        assert_bounds::<MountSpecParseError>();
    }
}
