//! Curated environment variables whose values Claude Code treats as paths.
//!
//! This list is intentionally explicit. Docker preflight uses it to reject
//! path values that would be unreadable inside the container namespace.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathValueShape {
    Single,
    ColonList,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PathShapedEnv {
    pub key: &'static str,
    pub doc_comment: &'static str,
    pub value_shape: PathValueShape,
}

impl PathShapedEnv {
    pub fn path_values<'a>(&self, value: &'a str) -> Vec<&'a str> {
        match self.value_shape {
            PathValueShape::Single => vec![value],
            PathValueShape::ColonList if value.is_empty() => vec![value],
            PathValueShape::ColonList => value.split(':').collect(),
        }
    }
}

pub fn claude_path_shaped_env(key: &str) -> Option<&'static PathShapedEnv> {
    CLAUDE_PATH_SHAPED_ENVS
        .iter()
        .find(|entry| entry.key == key)
}

pub const CLAUDE_PATH_SHAPED_ENVS: &[PathShapedEnv] = &[
    // `CLAUDE_BG_AUTH_SNAPSHOT_PATH`: Claude Code source names this auth
    // snapshot file path, which must be readable where the runtime starts.
    PathShapedEnv {
        key: "CLAUDE_BG_AUTH_SNAPSHOT_PATH",
        doc_comment: "Auth snapshot file path. Source: Claude Code source string.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_CLIENT_CERT`: documented mTLS client certificate file.
    PathShapedEnv {
        key: "CLAUDE_CODE_CLIENT_CERT",
        doc_comment: "mTLS client certificate file. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_CLIENT_KEY`: documented mTLS private key file.
    PathShapedEnv {
        key: "CLAUDE_CODE_CLIENT_KEY",
        doc_comment: "mTLS client private key file. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_DEBUG_LOGS_DIR`: documented debug log file path.
    PathShapedEnv {
        key: "CLAUDE_CODE_DEBUG_LOGS_DIR",
        doc_comment: "Debug log file path. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_GIT_BASH_PATH`: documented Git Bash executable path.
    PathShapedEnv {
        key: "CLAUDE_CODE_GIT_BASH_PATH",
        doc_comment: "Git Bash executable path. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_PLUGIN_CACHE_DIR`: documented plugin parent directory.
    PathShapedEnv {
        key: "CLAUDE_CODE_PLUGIN_CACHE_DIR",
        doc_comment: "Plugin parent directory. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_PLUGIN_SEED_DIR`: documented seed directory list.
    PathShapedEnv {
        key: "CLAUDE_CODE_PLUGIN_SEED_DIR",
        doc_comment: "Plugin seed directory list. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::ColonList,
    },
    // `CLAUDE_CODE_SHELL_PREFIX`: documented command prefix path for shell
    // commands, hooks, and MCP server startup.
    PathShapedEnv {
        key: "CLAUDE_CODE_SHELL_PREFIX",
        doc_comment: "Shell command prefix path. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CODE_TMPDIR`: documented temp directory root.
    PathShapedEnv {
        key: "CLAUDE_CODE_TMPDIR",
        doc_comment: "Claude Code temp directory root. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_CONFIG_DIR`: documented configuration directory.
    PathShapedEnv {
        key: "CLAUDE_CONFIG_DIR",
        doc_comment: "Configuration directory containing settings, credentials, history, and plugins. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_ENV_FILE`: documented shell script path sourced for Bash calls.
    PathShapedEnv {
        key: "CLAUDE_ENV_FILE",
        doc_comment: "Shell script path sourced before Bash commands. Source: Claude Code environment variable documentation.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_PLUGIN_ROOT`: Claude Code source names this plugin root
    // directory, which must exist inside the runtime namespace.
    PathShapedEnv {
        key: "CLAUDE_PLUGIN_ROOT",
        doc_comment: "Plugin root directory. Source: Claude Code source string.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_PROJECT_DIR`: conventional Claude project directory used by
    // hooks and subprocesses.
    PathShapedEnv {
        key: "CLAUDE_PROJECT_DIR",
        doc_comment: "Project directory for hooks and subprocesses. Source: Claude Code source string and convention.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_SECURESTORAGE_CONFIG_DIR`: Claude Code source names this
    // secure storage configuration directory.
    PathShapedEnv {
        key: "CLAUDE_SECURESTORAGE_CONFIG_DIR",
        doc_comment: "Secure storage configuration directory. Source: Claude Code source string.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_SKILL_DIR`: Claude Code source names this skills directory.
    PathShapedEnv {
        key: "CLAUDE_SKILL_DIR",
        doc_comment: "Skills directory. Source: Claude Code source string.",
        value_shape: PathValueShape::Single,
    },
    // `CLAUDE_TMPDIR`: Claude Code source names this temp directory root.
    PathShapedEnv {
        key: "CLAUDE_TMPDIR",
        doc_comment: "Claude temp directory root. Source: Claude Code source string.",
        value_shape: PathValueShape::Single,
    },
];

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn claude_path_shaped_envs_have_frozen_cardinality() {
        assert_eq!(CLAUDE_PATH_SHAPED_ENVS.len(), 16);
    }

    #[test]
    fn claude_path_shaped_envs_have_doc_comments() {
        for entry in CLAUDE_PATH_SHAPED_ENVS {
            assert!(
                !entry.doc_comment.trim().is_empty(),
                "{} is missing an entry doc comment",
                entry.key
            );
        }
    }

    #[test]
    fn claude_path_shaped_envs_are_unique_claude_keys() {
        let mut seen = BTreeSet::new();

        for entry in CLAUDE_PATH_SHAPED_ENVS {
            assert!(entry.key.starts_with("CLAUDE_"), "{}", entry.key);
            assert!(seen.insert(entry.key), "{} is duplicated", entry.key);
        }
    }

    #[test]
    fn colon_lists_preserve_each_declared_path() {
        let entry = claude_path_shaped_env("CLAUDE_CODE_PLUGIN_SEED_DIR").expect("entry");

        assert_eq!(
            entry.path_values("/seed/a:/seed/b"),
            vec!["/seed/a", "/seed/b"]
        );
    }
}
