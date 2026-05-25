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
        ExitCode::from(exit_codes::DOMAIN as u8)
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
