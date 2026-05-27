#![forbid(unsafe_code)]

pub mod agent_config;
pub mod error;
pub mod label;
pub mod mail;
pub mod mcp;
pub mod namespace;
pub mod paths;
pub mod proto;
pub mod runtime;
pub mod selector;
pub mod session;
pub mod tool_contracts;
pub mod types;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

pub use agent_config::{
    agent_config_uses_home_prefix, is_agent_config_path_like, normalize_agent_config_request,
};
pub use error::{SmError, SmResult, humanize_capture_error};
pub use lilo_rm_core::{IsolationPolicy, MountSpec};
pub use mcp::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION, tool_error, tool_success,
};
pub use paths::{SmEndpoint, SmPaths, SmPathsError, rtmd_socket_path};
pub use proto::{
    CaptureRequest, CaptureResponse, DaemonStatus, DeleteRequest, DeleteResponse, DoctorFinding,
    DoctorRequest, DoctorResponse, LabelRequest, LabelResponse, ListRequest, ListResponse,
    LogsRequest, LogsResponse, MailCheckRequest, MailCheckResponse, MailReadRequest,
    MailReadResponse, MailSendRequest, MailSendResponse, MailStopCheckRequest,
    MailStopCheckResponse, MailUnreadCount, McpBridgeRequest, McpBridgeResponse,
    NamespaceCreateRequest, NamespaceCreateResponse, NamespaceDeleteRequest,
    NamespaceDeleteResponse, NamespaceGetRequest, NamespaceGetResponse, NamespaceListRequest,
    NamespaceListResponse, NudgeDelivery, NudgeRequest, NudgeResponse, RpcRequest, RpcResponse,
    RuntimeDoctorReport, ShutdownResponse, SpawnRequest, SpawnResponse, TargetError, WaitCondition,
    WaitRequest, WaitResponse,
};
pub use types::{
    Channel, DEFAULT_NAMESPACE, Label, LabelMutation, LabelOp, LostEvidence, Mail, MailStatus,
    NAMESPACE_MAX_LEN, Namespace, NamespaceError, NamespaceRecord, NamespaceScope,
    RESERVED_NAMESPACE_PREFIX, RuntimeKind, Selector, Session, SessionState,
};
