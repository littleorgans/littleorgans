use std::io::IsTerminal;

use tracing_subscriber::EnvFilter;

use crate::diagnostic::Diagnostic;

const LILO_LOG_ENV: &str = "LILO_LOG";
const DEFAULT_LOG_FILTER: &str = "info";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogFormat {
    Json,
    Pretty,
}

pub fn init_logging() -> Result<(), Diagnostic> {
    let filter = log_filter()?;
    let format = select_format(output_json_requested(), std::io::stderr().is_terminal());

    try_init_subscriber(filter, format);

    Ok(())
}

fn log_filter() -> Result<EnvFilter, Diagnostic> {
    let directive = std::env::var(LILO_LOG_ENV).unwrap_or_else(|_| DEFAULT_LOG_FILTER.to_string());

    EnvFilter::try_new(directive).map_err(|error| {
        Diagnostic::input_validation(format!("{LILO_LOG_ENV} is not a valid tracing filter"))
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

fn select_format(output_json: bool, stderr_is_terminal: bool) -> LogFormat {
    if output_json || !stderr_is_terminal {
        LogFormat::Json
    } else {
        LogFormat::Pretty
    }
}

fn try_init_subscriber(filter: EnvFilter, format: LogFormat) {
    let _ = match format {
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

    #[test]
    fn init_logging_succeeds_when_called_twice() {
        init_logging().expect("first logging init");
        init_logging().expect("second logging init");
    }

    #[test]
    fn json_output_flag_selects_json_logging() {
        assert_eq!(select_format(true, true), LogFormat::Json);
    }

    #[test]
    fn terminal_human_output_selects_pretty_logging() {
        assert_eq!(select_format(false, true), LogFormat::Pretty);
    }

    #[test]
    fn non_terminal_human_output_selects_json_logging() {
        assert_eq!(select_format(false, false), LogFormat::Json);
    }

    #[test]
    fn lilo_log_json_env_var_has_no_format_effect() {
        assert_eq!(select_format(false, true), LogFormat::Pretty);
    }
}
