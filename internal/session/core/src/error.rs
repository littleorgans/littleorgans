use lilo_rm_core::CaptureError;
use thiserror::Error;

pub type SmResult<T> = Result<T, SmError>;

#[derive(Debug, Error)]
pub enum SmError {
    #[error("unsupported runtime: {0}")]
    UnsupportedRuntime(String),
    #[error(transparent)]
    Uuid(#[from] uuid::Error),
    #[error("{0}")]
    Message(String),
}

pub fn humanize_capture_error(error: &CaptureError) -> String {
    match error {
        CaptureError::NotATmuxTarget => "capture is only supported for tmux targets".to_string(),
        CaptureError::PaneUnavailable => "tmux pane is no longer available".to_string(),
        CaptureError::SessionMissing => "tmux session has gone away".to_string(),
        CaptureError::TmuxNotAvailable => "tmux is not available on this host".to_string(),
        CaptureError::CapturePaneFailed { stderr } => {
            format!("tmux capture-pane failed: {}", stderr.trim())
        }
        _ => format!("unknown capture error ({error:?})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_named_capture_errors() {
        assert_eq!(
            humanize_capture_error(&CaptureError::NotATmuxTarget),
            "capture is only supported for tmux targets"
        );
        assert_eq!(
            humanize_capture_error(&CaptureError::PaneUnavailable),
            "tmux pane is no longer available"
        );
        assert_eq!(
            humanize_capture_error(&CaptureError::SessionMissing),
            "tmux session has gone away"
        );
        assert_eq!(
            humanize_capture_error(&CaptureError::TmuxNotAvailable),
            "tmux is not available on this host"
        );
    }

    #[test]
    fn humanize_capture_pane_failed_trims_stderr() {
        let error = CaptureError::CapturePaneFailed {
            stderr: "  no server running\n".to_string(),
        };
        assert_eq!(
            humanize_capture_error(&error),
            "tmux capture-pane failed: no server running"
        );
    }
}
