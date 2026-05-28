use std::sync::OnceLock;

use lilo_rm_core::{LauncherError, RuntimeKind};

static CODEX_PATH: OnceLock<Result<String, LauncherError>> = OnceLock::new();

pub type CodexLauncher = crate::BinaryLauncher;

pub(crate) static CODEX: CodexLauncher =
    CodexLauncher::new(RuntimeKind::Codex, "codex", &CODEX_PATH);
