use std::process::ExitCode;

use clap::Parser;
use lilo_common::exit_codes;

#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    about = "Workspace task runner",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
enum Xtask {
    #[command(about = "Regenerate authored schema outputs")]
    Codegen,
    #[command(about = "Run release distribution checks")]
    DistCheck,
    #[command(about = "Stage substrate mirror repositories")]
    MirrorPublish,
}

impl Xtask {
    fn run(self) -> ExitCode {
        eprintln!("{}", self.deferral_message());
        // Exit codes in `lilo_common::exit_codes` are `i32` to align with the
        // `Diagnostic.exit_code` field, but fit in `u8` by construction. The
        // fallback to 1 (`INTERNAL`) is defensive: any value outside `u8`
        // signals a logic bug worth surfacing as an internal failure.
        ExitCode::from(u8::try_from(exit_codes::DOMAIN).unwrap_or(1))
    }

    fn deferral_message(&self) -> &'static str {
        match self {
            Self::Codegen => "xtask codegen is deferred to Phase 6 generated surface work.",
            Self::DistCheck => "xtask dist-check is deferred to Phase 8 release integration.",
            Self::MirrorPublish => {
                "xtask mirror-publish is deferred to Phase 8 tools/mirror-publish work."
            }
        }
    }
}

fn main() -> ExitCode {
    Xtask::parse().run()
}
