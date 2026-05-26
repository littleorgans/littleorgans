use serde::{Deserialize, Serialize};

use crate::exit_codes;

/// Stable user-facing diagnostic envelope for human and JSON output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct Diagnostic {
    pub code: String,
    pub message: String,
    pub detail: Option<String>,
    pub exit_code: i32,
}

impl Diagnostic {
    pub fn new(code: impl Into<String>, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            detail: None,
            exit_code,
        }
    }

    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal", message, exit_codes::INTERNAL)
    }

    pub fn domain(message: impl Into<String>) -> Self {
        Self::new("domain", message, exit_codes::DOMAIN)
    }

    pub fn input_validation(message: impl Into<String>) -> Self {
        Self::new("input_validation", message, exit_codes::INPUT_VALIDATION)
    }

    pub fn daemon_unavailable(message: impl Into<String>) -> Self {
        Self::new(
            "daemon_unavailable",
            message,
            exit_codes::DAEMON_UNAVAILABLE,
        )
    }

    pub fn authz_denied(message: impl Into<String>) -> Self {
        Self::new("authz_denied", message, exit_codes::AUTHZ_DENIED)
    }
}

impl From<anyhow::Error> for Diagnostic {
    fn from(error: anyhow::Error) -> Self {
        Self::internal(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn diagnostic_round_trips_with_stable_json_shape() {
        let diagnostic = Diagnostic::input_validation("invalid flag").with_detail("--output");

        let json_value = serde_json::to_value(&diagnostic).expect("serialize diagnostic");

        assert_eq!(
            json_value,
            json!({
                "code": "input_validation",
                "message": "invalid flag",
                "detail": "--output",
                "exit_code": 3
            })
        );

        let round_tripped: Diagnostic =
            serde_json::from_value(json_value).expect("deserialize diagnostic");

        assert_eq!(round_tripped, diagnostic);
    }

    #[test]
    fn anyhow_errors_map_to_internal_diagnostics() {
        let diagnostic = Diagnostic::from(anyhow::anyhow!("boom"));

        assert_eq!(diagnostic.code, "internal");
        assert_eq!(diagnostic.message, "boom");
        assert_eq!(diagnostic.exit_code, exit_codes::INTERNAL);
    }
}
