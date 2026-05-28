use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser};
use lilo_common::exit_codes;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "xtask",
    about = "Workspace task runner",
    arg_required_else_help = true,
    disable_help_subcommand = true
)]
enum Xtask {
    #[command(about = "Regenerate authored schema outputs")]
    Codegen(CodegenArgs),
    #[command(about = "Run release distribution checks")]
    DistCheck,
    #[command(about = "Stage substrate mirror repositories")]
    MirrorPublish,
}

#[derive(Debug, Args)]
struct CodegenArgs {
    #[arg(long, help = "Fail if generated outputs are stale")]
    check: bool,
}

impl Xtask {
    fn run(self) -> ExitCode {
        match self {
            Self::Codegen(args) => exit_code(run_codegen(args.check)),
            command => {
                eprintln!("{}", command.deferral_message());
                exit_code(Err(io::Error::other("xtask command is deferred").into()))
            }
        }
    }

    fn deferral_message(&self) -> &'static str {
        match self {
            Self::Codegen(_) => "xtask codegen is implemented.",
            Self::DistCheck => "xtask dist-check is deferred to Phase 8 release integration.",
            Self::MirrorPublish => {
                "xtask mirror-publish is deferred to Phase 8 tools/mirror-publish work."
            }
        }
    }
}

type XtaskResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run_codegen(check: bool) -> XtaskResult<()> {
    let repo_root = repo_root();
    let registry_path = repo_root.join("tools/schemas/cli.toml");
    let registry = CliRegistry::from_path(&registry_path)?;
    registry.validate()?;

    for output in generated_outputs(&repo_root, &registry)? {
        if check {
            verify_output(&output)?;
        } else {
            write_output(&output)?;
        }
    }
    Ok(())
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("tools/xtask has a repo root parent"))
        .to_path_buf()
}

fn generated_outputs(
    repo_root: &Path,
    registry: &CliRegistry,
) -> XtaskResult<Vec<GeneratedOutput>> {
    Ok(vec![
        GeneratedOutput::new(
            repo_root.join("crates/lilo/src/cli/generated_help.rs"),
            generated_help_rs(registry),
        ),
        GeneratedOutput::new(
            repo_root.join("crates/lilo/src/cli/generated_schema.rs"),
            generated_schema_rs(registry)?,
        ),
        GeneratedOutput::new(
            repo_root.join("tools/schemas/generated/lilo_cli_surface.json"),
            format!("{}\n", cli_surface_json(registry)?),
        ),
        GeneratedOutput::new(
            repo_root.join("tools/schemas/generated/lilo_mcp_schema.json"),
            format!("{}\n", mcp_schema_json(registry)?),
        ),
    ])
}

fn write_output(output: &GeneratedOutput) -> XtaskResult<()> {
    if let Some(parent) = output.path.parent() {
        fs::create_dir_all(parent)?;
    }
    if fs::read_to_string(&output.path).is_ok_and(|existing| existing == output.content) {
        return Ok(());
    }
    fs::write(&output.path, &output.content)?;
    Ok(())
}

fn verify_output(output: &GeneratedOutput) -> XtaskResult<()> {
    if fs::read_to_string(&output.path).is_ok_and(|existing| existing == output.content) {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "generated output is stale: {}",
        output.path.display()
    ))
    .into())
}

struct GeneratedOutput {
    path: PathBuf,
    content: String,
}

impl GeneratedOutput {
    fn new(path: PathBuf, content: String) -> Self {
        Self { path, content }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CliRegistry {
    surface: Surface,
    groups: Vec<CommandGroup>,
    commands: Vec<CommandSpec>,
}

impl CliRegistry {
    fn from_path(path: &Path) -> XtaskResult<Self> {
        let content = fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }

    fn validate(&self) -> XtaskResult<()> {
        let groups = self
            .groups
            .iter()
            .map(|group| group.id.as_str())
            .collect::<HashSet<_>>();
        for command in &self.commands {
            if command.hidden() {
                continue;
            }
            let Some(group) = command.group.as_deref() else {
                return Err(io::Error::other(format!(
                    "public command {} is missing group",
                    command.name
                ))
                .into());
            };
            if !groups.contains(group) {
                return Err(io::Error::other(format!(
                    "command {} references unknown group {group}",
                    command.name
                ))
                .into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Surface {
    binary: String,
    display_name: String,
    about: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct CommandGroup {
    id: String,
    heading: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct CommandSpec {
    name: String,
    const_name: String,
    group: Option<String>,
    about: String,
    hidden: Option<bool>,
}

impl CommandSpec {
    fn hidden(&self) -> bool {
        self.hidden.unwrap_or(false)
    }
}

fn generated_help_rs(registry: &CliRegistry) -> String {
    let mut output = generated_header();
    output.push_str("#[rustfmt::skip]\n");
    writeln!(
        output,
        "pub const ROOT_HELP_TEMPLATE: &str = {};",
        rust_string(&root_help_template(registry))
    )
    .expect("write help template");
    output.push('\n');
    for command in &registry.commands {
        output.push_str("#[rustfmt::skip]\n");
        writeln!(
            output,
            "pub const {}_ABOUT: &str = {};",
            command.const_name,
            rust_string(&command.about)
        )
        .expect("write command about");
    }
    output
}

fn generated_schema_rs(registry: &CliRegistry) -> XtaskResult<String> {
    let mut output = generated_header();
    output.push_str("#[rustfmt::skip]\n");
    writeln!(
        output,
        "pub const CLI_SURFACE_JSON: &str = r#\"{}\"#;",
        cli_surface_json(registry)?
    )?;
    output.push('\n');
    output.push_str("#[rustfmt::skip]\n");
    writeln!(
        output,
        "pub const MCP_SCHEMA_JSON: &str = r#\"{}\"#;",
        mcp_schema_json(registry)?
    )?;
    Ok(output)
}

fn generated_header() -> String {
    "// AUTO-GENERATED by xtask codegen from tools/schemas/cli.toml - do not edit\n\
     #![allow(clippy::all, dead_code)]\n\n"
        .to_string()
}

fn root_help_template(registry: &CliRegistry) -> String {
    let mut template = String::from("{about-with-newline}\n{usage-heading} {usage}\n\n");
    for group in &registry.groups {
        writeln!(template, "{}:", group.heading).expect("write group heading");
        for command in registry.public_commands_for_group(&group.id) {
            writeln!(template, "  {:<10} {}", command.name, command.about)
                .expect("write command help row");
        }
        template.push('\n');
    }
    template.push_str("Options:\n{options}{after-help}\n");
    template
}

fn cli_surface_json(registry: &CliRegistry) -> XtaskResult<String> {
    let groups = registry
        .groups
        .iter()
        .map(|group| {
            json!({
                "id": group.id,
                "heading": group.heading,
                "commands": registry.public_commands_for_group(&group.id),
            })
        })
        .collect::<Vec<_>>();
    let hidden_commands = registry
        .commands
        .iter()
        .filter(|command| command.hidden())
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&json!({
        "surface": &registry.surface,
        "groups": groups,
        "hidden_commands": hidden_commands,
    }))?)
}

fn mcp_schema_json(registry: &CliRegistry) -> XtaskResult<String> {
    let command_names = registry
        .commands
        .iter()
        .filter(|command| !command.hidden())
        .map(|command| command.name.as_str())
        .collect::<Vec<_>>();
    Ok(serde_json::to_string_pretty(&json!({
        "name": &registry.surface.binary,
        "description": &registry.surface.about,
        "input_schema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "enum": command_names,
                }
            },
            "required": ["command"],
        },
    }))?)
}

impl CliRegistry {
    fn public_commands_for_group(&self, group_id: &str) -> Vec<&CommandSpec> {
        self.commands
            .iter()
            .filter(|command| !command.hidden() && command.group.as_deref() == Some(group_id))
            .collect()
    }
}

fn rust_string(value: &str) -> String {
    serde_json::to_string(value).expect("string serializes")
}

fn exit_code(result: XtaskResult<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::from(u8::try_from(exit_codes::SUCCESS).unwrap_or(0)),
        Err(error) => {
            eprintln!("{error}");
            // Exit codes in `lilo_common::exit_codes` are `i32` to align with
            // `Diagnostic.exit_code`, but fit in `u8` by construction.
            ExitCode::from(u8::try_from(exit_codes::DOMAIN).unwrap_or(1))
        }
    }
}

fn main() -> ExitCode {
    Xtask::parse().run()
}
