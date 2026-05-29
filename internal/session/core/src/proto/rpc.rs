use serde::{Deserialize, Serialize};

use super::{
    CaptureRequest, CaptureResponse, DeleteRequest, DeleteResponse, DoctorRequest, DoctorResponse,
    IdentityAuditRequest, IdentityAuditResponse, IdentityWhoamiRequest, IdentityWhoamiResponse,
    LabelRequest, LabelResponse, ListRequest, ListResponse, LogsRequest, LogsResponse,
    MailCheckRequest, MailCheckResponse, MailReadRequest, MailReadResponse, MailSendRequest,
    MailSendResponse, MailStopCheckRequest, MailStopCheckResponse, McpBridgeRequest,
    McpBridgeResponse, NamespaceCreateRequest, NamespaceCreateResponse, NamespaceDeleteRequest,
    NamespaceDeleteResponse, NamespaceGetRequest, NamespaceGetResponse, NamespaceListRequest,
    NamespaceListResponse, NudgeRequest, NudgeResponse, ShutdownResponse, SpawnRequest,
    SpawnResponse, WaitRequest, WaitResponse,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionRpc {
    Spawn { request: Box<SpawnRequest> },
    List { request: ListRequest },
    NamespaceCreate { request: NamespaceCreateRequest },
    NamespaceGet { request: NamespaceGetRequest },
    NamespaceList { request: NamespaceListRequest },
    NamespaceDelete { request: NamespaceDeleteRequest },
    Delete { request: DeleteRequest },
    MailSend { request: MailSendRequest },
    MailRead { request: MailReadRequest },
    MailCheck { request: MailCheckRequest },
    MailStopCheck { request: MailStopCheckRequest },
    Nudge { request: NudgeRequest },
    Label { request: LabelRequest },
    Logs { request: LogsRequest },
    Capture { request: CaptureRequest },
    Doctor { request: DoctorRequest },
    Wait { request: WaitRequest },
    McpBridge { request: McpBridgeRequest },
    IdentityWhoami { request: IdentityWhoamiRequest },
    IdentityAudit { request: IdentityAuditRequest },
    Shutdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RpcResponse {
    Spawned { response: SpawnResponse },
    Listed { response: ListResponse },
    NamespaceCreated { response: NamespaceCreateResponse },
    NamespaceGot { response: NamespaceGetResponse },
    NamespacesListed { response: NamespaceListResponse },
    NamespaceDeleted { response: NamespaceDeleteResponse },
    Deleted { response: DeleteResponse },
    MailSent { response: MailSendResponse },
    MailRead { response: MailReadResponse },
    MailChecked { response: MailCheckResponse },
    MailStopChecked { response: MailStopCheckResponse },
    Nudged { response: NudgeResponse },
    Labeled { response: LabelResponse },
    Logs { response: LogsResponse },
    Capture { response: CaptureResponse },
    Doctor { response: DoctorResponse },
    Wait { response: WaitResponse },
    McpBridge { response: McpBridgeResponse },
    IdentityWhoami { response: IdentityWhoamiResponse },
    IdentityAudit { response: IdentityAuditResponse },
    Shutdown { response: ShutdownResponse },
    Error { message: String },
}

impl RpcResponse {
    pub fn kind(&self) -> &'static str {
        match self {
            RpcResponse::Spawned { .. } => "Spawned",
            RpcResponse::Listed { .. } => "Listed",
            RpcResponse::NamespaceCreated { .. } => "NamespaceCreated",
            RpcResponse::NamespaceGot { .. } => "NamespaceGot",
            RpcResponse::NamespacesListed { .. } => "NamespacesListed",
            RpcResponse::NamespaceDeleted { .. } => "NamespaceDeleted",
            RpcResponse::Deleted { .. } => "Deleted",
            RpcResponse::MailSent { .. } => "MailSent",
            RpcResponse::MailRead { .. } => "MailRead",
            RpcResponse::MailChecked { .. } => "MailChecked",
            RpcResponse::MailStopChecked { .. } => "MailStopChecked",
            RpcResponse::Nudged { .. } => "Nudged",
            RpcResponse::Labeled { .. } => "Labeled",
            RpcResponse::Logs { .. } => "Logs",
            RpcResponse::Capture { .. } => "Capture",
            RpcResponse::Doctor { .. } => "Doctor",
            RpcResponse::Wait { .. } => "Wait",
            RpcResponse::McpBridge { .. } => "McpBridge",
            RpcResponse::IdentityWhoami { .. } => "IdentityWhoami",
            RpcResponse::IdentityAudit { .. } => "IdentityAudit",
            RpcResponse::Shutdown { .. } => "Shutdown",
            RpcResponse::Error { .. } => "Error",
        }
    }
}
