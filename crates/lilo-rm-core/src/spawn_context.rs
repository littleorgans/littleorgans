use std::ffi::OsString;
use std::path::PathBuf;

use crate::{LaunchEnv, ShellResume};

/// Exact env-var names dropped when forwarding caller env into a spawned runtime.
///
/// These either name the calling process's parent context (so forwarding lies to the spawned
/// runtime about who its parent is) or are daemon/test internals the daemon re-sets correctly.
pub const CALLER_ENV_DENYLIST: &[&str] = &[
    "CLAUDECODE",
    "TMUX",
    "TMUX_PANE",
    "LILO_SOCKET_PATH",
    "HELIOY_SESSION_ID",
    "HELIOY_RUNTIME",
    "RTM_SESSION_ID",
    "RTM_RUNTIME_KIND",
];

/// Prefixes dropped when forwarding caller env. Used for variable families
/// like `CLAUDE_CODE_*` and `CLAUDE_PLUGIN_*` that describe the calling claude
/// instance, not user state.
pub const CALLER_ENV_DENYLIST_PREFIXES: &[&str] = &["CLAUDE_CODE_", "CLAUDE_PLUGIN_"];

const SHELL_RESUME_ENV_ALLOWLIST: &[&str] = &[
    "COLORTERM",
    "HOME",
    "LANG",
    "LC_ALL",
    "LOGNAME",
    "PATH",
    "SHELL",
    "TERM",
    "USER",
];

/// Capture the caller's environment, filtered through the denylist.
///
/// Iterates `std::env::vars_os()` so non-UTF-8 keys and values do not panic.
/// Lossy decoding is applied via [`capture_env_from_os`]: we choose lossy over
/// reject because env values are an open universe, and refusing on the first
/// non-UTF-8 byte would break callers on systems with non-UTF-8 locales for
/// reasons unrelated to anything rtm cares about.
pub fn capture_caller_env() -> Vec<LaunchEnv> {
    capture_env_from_os(std::env::vars_os())
}

/// Variant of [`capture_env_from`] that accepts `OsString` keys and values
/// (the shape returned by `std::env::vars_os()`). Lossy-converts both before
/// applying the denylist. Exposed for tests that want to feed `OsString`
/// directly, exercising the same code path as `capture_caller_env`.
pub fn capture_env_from_os<I>(iter: I) -> Vec<LaunchEnv>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    capture_env_from(iter.into_iter().map(|(k, v)| {
        (
            k.to_string_lossy().into_owned(),
            v.to_string_lossy().into_owned(),
        )
    }))
}

/// Filter an iterator of `(String, String)` env entries through the denylist.
/// Use [`capture_caller_env`] or [`capture_env_from_os`] for OS-sourced env;
/// this lower-level variant is the right choice when env is already UTF-8.
pub fn capture_env_from<I, K, V>(iter: I) -> Vec<LaunchEnv>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    iter.into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .filter(|(k, _)| !is_denied(k))
        .map(|(k, v)| LaunchEnv::new(k, v))
        .collect()
}

fn is_denied(key: &str) -> bool {
    if CALLER_ENV_DENYLIST.contains(&key) {
        return true;
    }
    CALLER_ENV_DENYLIST_PREFIXES
        .iter()
        .any(|prefix| key.starts_with(prefix))
}

/// Capture the caller's current working directory.
pub fn capture_caller_cwd() -> std::io::Result<PathBuf> {
    std::env::current_dir()
}

pub fn capture_shell_resume(cwd: PathBuf) -> ShellResume {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_owned());
    let mut env = capture_shell_resume_env(std::env::vars_os());
    ensure_shell_env(&mut env, &shell);
    ShellResume {
        argv: vec![shell],
        env,
        cwd,
    }
}

pub fn capture_shell_resume_env<I>(iter: I) -> Vec<LaunchEnv>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    iter.into_iter()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .filter(|(k, _)| SHELL_RESUME_ENV_ALLOWLIST.contains(&k.as_str()))
        .map(|(k, v)| LaunchEnv::new(k, v))
        .collect()
}

fn ensure_shell_env(env: &mut Vec<LaunchEnv>, shell: &str) {
    if env.iter().any(|entry| entry.key == "SHELL") {
        return;
    }
    env.push(LaunchEnv::new("SHELL", shell));
}

/// Placeholder cwd for call sites that only exercise launcher resolution
/// (argv lookup, env builders) and never actually spawn a runtime in the
/// returned directory. Centralized so future audits can grep for it and know
/// "this cwd was deliberately not load-bearing."
pub fn launcher_probe_cwd() -> PathBuf {
    PathBuf::from("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn denylist_drops_parent_markers() {
        let env = capture_env_from([
            ("PATH", "/usr/bin"),
            ("CLAUDECODE", "1"),
            ("CLAUDE_CODE_SESSION_ID", "abc"),
            ("CLAUDE_PLUGIN_DATA", "/tmp"),
            ("TMUX", "/private/tmp/tmux"),
            ("TMUX_PANE", "%4"),
            ("LILO_SOCKET_PATH", "/tmp/lilod.sock"),
            ("HELIOY_SESSION_ID", "session"),
            ("HELIOY_RUNTIME", "claude"),
            ("RTM_SESSION_ID", "session"),
            ("RTM_RUNTIME_KIND", "claude"),
            ("HELIOY_PAT", "ghp_secret"),
            ("ANTHROPIC_API_KEY", "sk-secret"),
        ]);
        let keys: Vec<&str> = env.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["PATH", "HELIOY_PAT", "ANTHROPIC_API_KEY"]);
    }

    #[test]
    fn denylist_keeps_user_state() {
        let env = capture_env_from([
            ("PATH", "/usr/bin"),
            ("HOME", "/Users/alphab"),
            ("LANG", "en_US.UTF-8"),
            ("MISE_SHELL", "zsh"),
        ]);
        assert_eq!(env.len(), 4);
    }

    #[test]
    fn capture_env_from_os_tolerates_non_utf8() {
        use std::os::unix::ffi::OsStringExt;

        // 0xFF is invalid as a leading UTF-8 byte. capture_env_from_os runs
        // the same lossy conversion path as capture_caller_env (the runtime
        // entry point), so this test protects the actual production path
        // rather than the already-UTF-8 capture_env_from variant.
        let raw_value = OsString::from_vec(vec![b'A', 0xFF, b'B']);
        let env = capture_env_from_os([(OsString::from("RTM_TEST_BAD_BYTES"), raw_value)]);
        assert_eq!(env.len(), 1);
        assert_eq!(env[0].key, "RTM_TEST_BAD_BYTES");
        assert!(env[0].value.contains('\u{FFFD}'), "{:?}", env[0].value);
    }

    #[test]
    fn capture_env_from_os_applies_denylist() {
        // Denylist must also run through the OsString path, not only the
        // UTF-8 capture_env_from path.
        let env = capture_env_from_os([
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("CLAUDECODE"), OsString::from("1")),
            (
                OsString::from("CLAUDE_CODE_SESSION_ID"),
                OsString::from("abc"),
            ),
        ]);
        let keys: Vec<&str> = env.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["PATH"]);
    }

    #[test]
    fn shell_resume_env_keeps_shell_state_without_runtime_secrets() {
        let env = capture_shell_resume_env([
            (OsString::from("SHELL"), OsString::from("/bin/zsh")),
            (OsString::from("HOME"), OsString::from("/Users/test")),
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("TERM"), OsString::from("xterm-256color")),
            (OsString::from("RTM_SESSION_ID"), OsString::from("secret")),
            (
                OsString::from("ANTHROPIC_API_KEY"),
                OsString::from("secret"),
            ),
        ]);
        let keys: Vec<&str> = env.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, vec!["SHELL", "HOME", "PATH", "TERM"]);
    }
}
