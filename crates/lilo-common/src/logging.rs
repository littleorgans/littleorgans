use std::io::IsTerminal;

use lilo_paths::env::{LILO_LOG, LILO_LOG_FORMAT};
use tracing_subscriber::EnvFilter;

use crate::diagnostic::Diagnostic;

const DEFAULT_LOG_FILTER: &str = "info";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Compact,
    Json,
    Pretty,
}

pub fn init_logging() -> Result<(), Diagnostic> {
    let filter = log_filter()?;
    let format = select_format(output_json_requested(), std::io::stderr().is_terminal())?;

    try_init_subscriber(filter, format);

    Ok(())
}

fn log_filter() -> Result<EnvFilter, Diagnostic> {
    let directive = std::env::var(LILO_LOG).unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_string());

    EnvFilter::try_new(directive).map_err(|error| {
        Diagnostic::input_validation(format!("{LILO_LOG} is not a valid tracing filter"))
            .with_detail(error.to_string())
    })
}

fn output_json_requested() -> bool {
    let mut args = std::env::args_os().skip(1);

    while let Some(arg) = args.next() {
        if arg == "--output=json" {
            return true;
        }

        if arg == "--output" && matches!(args.next().as_deref(), Some(value) if value == "json") {
            return true;
        }
    }

    false
}

fn select_format(output_json: bool, stderr_is_terminal: bool) -> Result<LogFormat, Diagnostic> {
    if let Some(format) = log_format_override()? {
        return Ok(format);
    }

    Ok(select_auto_format(output_json, stderr_is_terminal))
}

fn log_format_override() -> Result<Option<LogFormat>, Diagnostic> {
    match std::env::var(LILO_LOG_FORMAT) {
        Ok(value) => parse_log_format(&value),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(error) => Err(Diagnostic::input_validation(format!(
            "{LILO_LOG_FORMAT} is not valid UTF-8"
        ))
        .with_detail(error.to_string())),
    }
}

fn parse_log_format(value: &str) -> Result<Option<LogFormat>, Diagnostic> {
    match value.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(None),
        "compact" => Ok(Some(LogFormat::Compact)),
        "json" => Ok(Some(LogFormat::Json)),
        "pretty" => Ok(Some(LogFormat::Pretty)),
        _ => Err(Diagnostic::input_validation(format!(
            "{LILO_LOG_FORMAT} must be one of auto, pretty, json, compact"
        ))
        .with_detail(format!("received {value:?}"))),
    }
}

fn select_auto_format(output_json: bool, stderr_is_terminal: bool) -> LogFormat {
    if output_json || !stderr_is_terminal {
        LogFormat::Json
    } else {
        LogFormat::Pretty
    }
}

fn try_init_subscriber(filter: EnvFilter, format: LogFormat) {
    let _ = match format {
        LogFormat::Compact => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .compact()
            .try_init(),
        LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .json()
            .try_init(),
        LogFormat::Pretty => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .pretty()
            .try_init(),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    #[test]
    fn init_logging_succeeds_when_called_twice() {
        init_logging().expect("first logging init");
        init_logging().expect("second logging init");
    }

    #[test]
    fn json_output_flag_selects_json_logging() {
        let _env = EnvVarGuard::new(LILO_LOG_FORMAT, None);
        assert_eq!(select_format(true, true).expect("format"), LogFormat::Json);
    }

    #[test]
    fn terminal_human_output_selects_pretty_logging() {
        let _env = EnvVarGuard::new(LILO_LOG_FORMAT, None);
        assert_eq!(
            select_format(false, true).expect("format"),
            LogFormat::Pretty
        );
    }

    #[test]
    fn non_terminal_human_output_selects_json_logging() {
        let _env = EnvVarGuard::new(LILO_LOG_FORMAT, None);
        assert_eq!(
            select_format(false, false).expect("format"),
            LogFormat::Json
        );
    }

    #[test]
    fn lilo_log_format_values_select_expected_formats() {
        for (value, expected) in [
            ("auto", LogFormat::Pretty),
            ("pretty", LogFormat::Pretty),
            ("json", LogFormat::Json),
            ("compact", LogFormat::Compact),
            (" CoMpAcT ", LogFormat::Compact),
        ] {
            let _env = EnvVarGuard::new(LILO_LOG_FORMAT, Some(value));
            assert_eq!(select_format(false, true).expect("format"), expected);
        }
    }

    #[test]
    fn lilo_log_format_precedes_json_output_flag() {
        let _env = EnvVarGuard::new(LILO_LOG_FORMAT, Some("pretty"));
        assert_eq!(
            select_format(true, false).expect("format"),
            LogFormat::Pretty
        );
    }

    #[test]
    fn invalid_lilo_log_format_errors() {
        let _env = EnvVarGuard::new(LILO_LOG_FORMAT, Some("xml"));
        let error = select_format(false, true).expect_err("invalid format");
        assert_eq!(error.code, "input_validation");
        assert!(error.message.contains(LILO_LOG_FORMAT));
    }

    struct EnvVarGuard {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl EnvVarGuard {
        fn new(name: &'static str, value: Option<&str>) -> Self {
            let lock = ENV_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("environment lock is not poisoned");
            let previous = std::env::var_os(name);
            set_env_var(name, value.map(std::ffi::OsStr::new));
            Self {
                name,
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            set_env_var(self.name, self.previous.as_deref());
        }
    }

    fn set_env_var(name: &str, value: Option<&std::ffi::OsStr>) {
        match value {
            Some(value) => {
                // SAFETY: Tests serialize environment mutation through ENV_LOCK.
                unsafe { std::env::set_var(name, value) };
            }
            None => {
                // SAFETY: Tests serialize environment mutation through ENV_LOCK.
                unsafe { std::env::remove_var(name) };
            }
        }
    }
}
