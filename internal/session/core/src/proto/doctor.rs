use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DoctorRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorResponse {
    pub status: String,
    pub daemon_version: Option<String>,
    pub runtime: String,
    pub runtime_matters: RuntimeDoctorReport,
    pub findings: Vec<DoctorFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDoctorReport {
    pub status: String,
    pub doctor: Option<Box<lilo_rm_core::DoctorResponse>>,
    pub socket_path: Option<String>,
    pub code: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorFinding {
    pub severity: String,
    pub session_id: Option<String>,
    pub message: String,
}
