use std::sync::OnceLock;

use lilo_rm_core::{LauncherError, RuntimeKind};

static CLAUDE_PATH: OnceLock<Result<String, LauncherError>> = OnceLock::new();

pub type ClaudeLauncher = crate::BinaryLauncher;

pub(crate) static CLAUDE: ClaudeLauncher =
    ClaudeLauncher::new(RuntimeKind::Claude, "claude", &CLAUDE_PATH);
