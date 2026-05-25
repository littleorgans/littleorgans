use std::collections::BTreeMap;

use clap::Args;
use lilo_common::diagnostic::Diagnostic;
use serde::Serialize;

use super::Output;

#[derive(Debug, Args)]
pub struct DoctorCommand {}

impl DoctorCommand {
    pub fn run(&self, output: Output) -> Result<(), Diagnostic> {
        let status = DoctorStatus::empty();

        match output {
            Output::Human => println!("{}", status.render_human()),
            Output::Json => println!("{}", status.render_json()?),
        }

        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct DoctorStatus {
    daemon: Option<DaemonStatus>,
    substrates: BTreeMap<String, SubstrateStatus>,
    warnings: Vec<String>,
}

impl DoctorStatus {
    fn empty() -> Self {
        Self {
            daemon: None,
            substrates: BTreeMap::new(),
            warnings: Vec::new(),
        }
    }

    fn render_human(&self) -> String {
        "lilo doctor: empty state\ndaemon: unavailable\nsubstrates: none\nwarnings: none"
            .to_string()
    }

    fn render_json(&self) -> Result<String, Diagnostic> {
        serde_json::to_string(self).map_err(|error| {
            Diagnostic::internal("failed to serialize doctor status").with_detail(error.to_string())
        })
    }
}

#[derive(Debug, Serialize)]
struct DaemonStatus {}

#[derive(Debug, Serialize)]
struct SubstrateStatus {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_status_has_stable_json_shape() {
        assert_eq!(
            DoctorStatus::empty()
                .render_json()
                .expect("render doctor json"),
            r#"{"daemon":null,"substrates":{},"warnings":[]}"#
        );
    }

    #[test]
    fn human_output_is_stable() {
        assert_eq!(
            DoctorStatus::empty().render_human(),
            "lilo doctor: empty state\ndaemon: unavailable\nsubstrates: none\nwarnings: none"
        );
    }
}
