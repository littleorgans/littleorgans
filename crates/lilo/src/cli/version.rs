use clap::Args;
use lilo_common::diagnostic::Diagnostic;
use lilo_rm_core::{VersionInfo, version_info};

use super::Output;

#[derive(Debug, Args)]
pub struct VersionCommand {}

impl VersionCommand {
    pub fn run(output: Output) -> Result<(), Diagnostic> {
        let info = build_version_info();

        match output {
            Output::Human => println!("{}", render_human(&info)),
            Output::Json => println!("{}", render_json(&info)?),
        }

        Ok(())
    }
}

fn build_version_info() -> VersionInfo {
    let mut info = version_info();
    crate::VERSION.clone_into(&mut info.version);
    info
}

fn render_human(info: &VersionInfo) -> String {
    format!(
        "lilo:\n  version: {}\n  git_sha: {}\nruntime:\n  protocol_version: {}\n  capabilities:\n{}",
        info.version,
        info.git_sha,
        info.protocol_version,
        render_capabilities(info)
    )
}

fn render_capabilities(info: &VersionInfo) -> String {
    info.capabilities
        .iter()
        .map(|capability| format!("    - {}", capability.as_str()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_json(info: &VersionInfo) -> Result<String, Diagnostic> {
    serde_json::to_string(info).map_err(|error| {
        Diagnostic::internal("failed to serialize version metadata").with_detail(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use lilo_rm_core::{RuntimeCapability, VersionInfo};

    use super::{render_human, render_json};

    fn test_info() -> VersionInfo {
        VersionInfo {
            version: "1.2.3".to_owned(),
            git_sha: "abcdef123456".to_owned(),
            protocol_version: "0.7".to_owned(),
            capabilities: vec![
                RuntimeCapability::StructuredProtocolErrors,
                RuntimeCapability::SpawnRequestMounts,
                RuntimeCapability::NudgeWaitTimeout,
            ],
        }
    }

    #[test]
    fn human_output_includes_protocol_and_capabilities() {
        let rendered = render_human(&test_info());

        assert!(rendered.contains("version: 1.2.3"));
        assert!(rendered.contains("git_sha: abcdef123456"));
        assert!(rendered.contains("protocol_version: 0.7"));
        assert!(rendered.contains("structured_protocol_errors"));
        assert!(rendered.contains("spawn_request_mounts"));
    }

    #[test]
    fn json_output_reuses_version_info_contract() {
        let json = render_json(&test_info()).expect("version json serializes");
        let value: serde_json::Value =
            serde_json::from_str(&json).expect("version json is parseable");

        assert_eq!(value["version"], "1.2.3");
        assert_eq!(value["git_sha"], "abcdef123456");
        assert_eq!(value["protocol_version"], "0.7");
        assert_eq!(
            value["capabilities"],
            serde_json::json!([
                "structured_protocol_errors",
                "spawn_request_mounts",
                "nudge_wait_timeout"
            ])
        );
    }
}
