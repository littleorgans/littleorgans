use thiserror::Error;

use crate::types::{Action, Principal};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuthzError {
    #[error("unauthorized principal for action")]
    Unauthorized {
        principal: Principal,
        action: Action,
        reason: String,
    },
    #[error("unknown principal")]
    UnknownPrincipal,
    #[error("audit sink failed: {message}")]
    Audit { message: String },
    #[error("internal authorization error: {message}")]
    Internal { message: String },
}

impl AuthzError {
    #[must_use]
    pub fn audit(error: &AuditError) -> Self {
        Self::Audit {
            message: error.to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum AuditError {
    #[error("{message}")]
    Sink { message: String },
}

impl AuditError {
    #[must_use]
    pub fn sink(message: impl Into<String>) -> Self {
        Self::Sink {
            message: message.into(),
        }
    }
}
