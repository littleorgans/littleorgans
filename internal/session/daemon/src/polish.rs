use std::fs;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use lilo_im_core::{Action, ResourceSpec};
use lilo_rm_client::{ClientError, RuntimeClient};
use lilo_rm_core::RUNTIME_PROTOCOL_VERSION;
use lilo_session_core::{
    DoctorFinding, DoctorRequest, DoctorResponse, LogsRequest, LogsResponse, RpcResponse,
    RuntimeDoctorReport, Selector, Session, SessionState, WaitCondition, WaitRequest, WaitResponse,
};

use crate::handler::DaemonState;
use crate::identity_client::session_resource;

impl DaemonState {
    pub(crate) async fn logs(
        &self,
        context: &crate::identity_client::RequestContext,
        request: LogsRequest,
    ) -> Result<RpcResponse> {
        let session = self.single_session(&request.selector, "logs")?;
        self.identity
            .authorize(
                &context.principal,
                Action::Logs,
                &session_resource(session.id),
            )
            .await?;
        let transcript_path = session
            .transcript_path
            .clone()
            .with_context(|| format!("no transcript available for session {}", session.id))?;
        let transcript = read_transcript(&transcript_path, request.max_bytes)
            .with_context(|| format!("failed to read transcript {}", transcript_path.display()))?;

        Ok(RpcResponse::Logs {
            response: LogsResponse {
                session,
                transcript_path,
                content: transcript,
            },
        })
    }

    pub(crate) async fn doctor(
        &self,
        context: &crate::identity_client::RequestContext,
        _request: DoctorRequest,
    ) -> Result<RpcResponse> {
        self.identity
            .authorize(&context.principal, Action::Doctor, &ResourceSpec::default())
            .await?;
        let fresh_findings = if self.rtmd_socket_path.is_some() {
            crate::reconcile::reconcile_once(self)
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let runtime_matters = self.runtime_doctor().await;
        let sessions = self
            .store()?
            .list_sessions_by_selector(&Selector::All)
            .context("failed to list sessions")?;
        let mut findings = sessions
            .into_iter()
            .filter(|session| matches!(session.state, SessionState::Lost { .. }))
            .map(|session| DoctorFinding {
                severity: "error".to_string(),
                session_id: Some(session.id.to_string()),
                message: lost_session_evidence(&session, &fresh_findings),
            })
            .collect::<Vec<_>>();
        findings.extend(runtime_doctor_findings(&runtime_matters));
        findings.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        let status = if findings.is_empty() {
            "ok"
        } else {
            "degraded"
        };

        Ok(RpcResponse::Doctor {
            response: DoctorResponse {
                status: status.to_string(),
                runtime: format!(
                    "rtmd (lilo-rm-client 0.6.x, protocol {RUNTIME_PROTOCOL_VERSION})"
                ),
                runtime_matters,
                findings,
            },
        })
    }

    pub(crate) async fn wait(&self, request: WaitRequest) -> Result<RpcResponse> {
        let deadline = Instant::now() + Duration::from_secs(request.timeout_secs);
        loop {
            let sessions = self
                .store()?
                .list_sessions_by_selector(&request.selector)
                .context("failed to list sessions")?;
            if wait_condition_met(&request.condition, &sessions) {
                return Ok(wait_response(true, sessions));
            }
            if Instant::now() >= deadline {
                return Ok(wait_response(false, sessions));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    fn single_session(&self, selector: &Selector, label: &str) -> Result<Session> {
        let sessions = self.resolve_selector(selector, label)?;
        anyhow::ensure!(
            sessions.len() == 1,
            "{label} selector matched {} sessions; expected exactly one",
            sessions.len()
        );
        sessions
            .into_iter()
            .next()
            .with_context(|| format!("{label} selector matched no sessions; expected exactly one"))
    }

    async fn runtime_doctor(&self) -> RuntimeDoctorReport {
        let Some(socket_path) = &self.rtmd_socket_path else {
            return RuntimeDoctorReport {
                status: "not_configured".to_string(),
                doctor: None,
                socket_path: None,
                code: None,
                message: Some("rtmd socket path is not configured".to_string()),
            };
        };
        let socket_path = socket_path.clone();
        match RuntimeClient::new(socket_path.clone()).doctor().await {
            Ok(payload) => {
                let status = runtime_doctor_status(&payload.doctor);
                RuntimeDoctorReport {
                    status,
                    doctor: Some(Box::new(payload.doctor)),
                    socket_path: Some(socket_path.display().to_string()),
                    code: None,
                    message: None,
                }
            }
            Err(error) => RuntimeDoctorReport {
                status: "error".to_string(),
                doctor: None,
                socket_path: Some(socket_path.display().to_string()),
                code: Some(runtime_error_code(&error)),
                message: Some(error.to_string()),
            },
        }
    }
}

fn read_transcript(path: &std::path::Path, max_bytes: Option<u64>) -> Result<String> {
    let mut bytes = fs::read(path)?;
    let max_bytes = max_bytes.and_then(|value| usize::try_from(value).ok());
    if let Some(max_bytes) = max_bytes
        && bytes.len() > max_bytes
    {
        bytes = bytes.split_off(bytes.len() - max_bytes);
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

fn wait_condition_met(condition: &WaitCondition, sessions: &[Session]) -> bool {
    match condition {
        WaitCondition::Running => sessions
            .iter()
            .any(|session| session.state == SessionState::Running),
        WaitCondition::Terminated => sessions.iter().any(|session| {
            matches!(
                session.state,
                SessionState::Terminated | SessionState::Lost { .. }
            )
        }),
        WaitCondition::Count { count } => sessions.len() == *count,
    }
}

fn wait_response(matched: bool, sessions: Vec<Session>) -> RpcResponse {
    RpcResponse::Wait {
        response: WaitResponse { matched, sessions },
    }
}

fn lost_session_evidence(
    session: &Session,
    findings: &[crate::reconcile::ReconcileFinding],
) -> String {
    findings
        .iter()
        .find(|finding| finding.session_id == session.id.to_string())
        .map_or_else(
            || match session.state {
                SessionState::Lost { evidence } => format!("session is LOST: {evidence}"),
                _ => format!("session is not LOST: {}", session.state),
            },
            |finding| finding.evidence.clone(),
        )
}

fn runtime_doctor_status(doctor: &lilo_rm_core::DoctorResponse) -> String {
    if doctor.version.protocol_version != RUNTIME_PROTOCOL_VERSION
        || !doctor.sqlite.pending_descriptions.is_empty()
    {
        "degraded".to_string()
    } else {
        "ok".to_string()
    }
}

fn runtime_doctor_findings(report: &RuntimeDoctorReport) -> Vec<DoctorFinding> {
    match report.status.as_str() {
        "ok" | "not_configured" => Vec::new(),
        _ => vec![DoctorFinding {
            severity: "error".to_string(),
            session_id: None,
            message: runtime_doctor_message(report),
        }],
    }
}

fn runtime_doctor_message(report: &RuntimeDoctorReport) -> String {
    if let Some(message) = &report.message {
        return format!("runtime-matters doctor failed: {message}");
    }
    let Some(doctor) = &report.doctor else {
        return "runtime-matters doctor failed".to_string();
    };
    if doctor.version.protocol_version != RUNTIME_PROTOCOL_VERSION {
        return format!(
            "runtime-matters protocol mismatch: required {RUNTIME_PROTOCOL_VERSION}, got {}",
            doctor.version.protocol_version
        );
    }
    if !doctor.sqlite.pending_descriptions.is_empty() {
        return format!(
            "runtime-matters sqlite migration drift: pending {}",
            doctor.sqlite.pending_descriptions.join(", ")
        );
    }
    format!("runtime-matters doctor status {}", report.status)
}

fn runtime_error_code(error: &ClientError) -> String {
    error.code().as_str().to_string()
}
