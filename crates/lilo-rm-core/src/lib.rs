#![forbid(unsafe_code)]

//! Runtime Matters core protocol types and JSON line transport helpers.
//!
//! This crate is the stable contract shared by `rtm` clients and rtmd. The
//! daemon, CLI, platform, launcher, and store crates remain private
//! implementation details.
//!
//! ## Events contract
//!
//! v0.4 events use [`RuntimeRpc::Events`] and
//! [`RuntimeResponse::Events`].
//! The daemon appends lifecycle observations to a durable JSONL log in global
//! order. Clients pass the returned cursor as `since` to resume without
//! duplicate delivery after client or daemon restarts.
//!
//! If a cursor is older than the retained log floor, rtmd returns
//! [`RuntimeResponse::CursorExpired`].

pub mod admin;
pub mod capture;
mod cli_output;
pub mod error;
pub mod isolation;
pub mod launcher;
pub mod mcp;
pub mod path_shaped_envs;
pub mod proto;
pub mod spawn_context;
pub mod tool_contracts;
pub mod types;
mod version;

pub use admin::{
    DockerIsolationStatus, DockerReadiness, DockerStatus, DoctorResponse, KillByPidRequest,
    KillByPidResponse, KillOutcome, LauncherStatus, LifecycleCounts, LifecycleLogAvailability,
    MigrationState, RecentLostEvent, StatusFilter, StatusResponse, TmuxStatus, WatcherCounts,
};
pub use capture::{
    CaptureError, CaptureRequest, CaptureResponse, LogAvailability, LogsUnavailableReason,
    PaneSnapshot,
};
pub use cli_output::{Ack, CliOutput};
pub use error::{ErrorCode, ProtocolError, RuntimeKindParseError};
pub use isolation::{IsolationPolicy, IsolationPolicyParseError, IsolationProfile};
pub use launcher::{
    LaunchEnv, LaunchSpec, LauncherError, RuntimeLauncher, ShellResume, upsert_launch_env,
};
pub use mcp::{
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, MCP_PROTOCOL_VERSION, McpBridgeRequest,
    McpBridgeResponse, json_rpc_error, json_rpc_failure, json_rpc_result, tool_error, tool_success,
};
pub use path_shaped_envs::{
    CLAUDE_PATH_SHAPED_ENVS, PathShapedEnv, PathValueShape, claude_path_shaped_env,
};
pub use proto::{
    CapturePayload, CursorExpiredPayload, DoctorPayload, EVENT_LOG_RETENTION_MIN_AGE_SECS,
    EVENT_LOG_RETENTION_MIN_EVENTS, EVENT_WAIT_MAX_MS, ErrorPayload, EventBatch, EventCursor,
    EventsPayload, EventsRequest, KillByPidPayload, KilledPayload, McpBridgePayload, NudgePayload,
    RuntimeResponse, RuntimeRpc, ShimLaunchPayload, SpawnConflictKind, SpawnConflictPayload,
    SpawnedPayload, StatusPayload, StatusRequest, ValidateTargetPayload, VersionPayload,
    WatchersPayload, clamped_event_wait_ms, read_json_line, read_json_line_blocking,
    write_json_line, write_json_line_blocking,
};
pub use spawn_context::{
    CALLER_ENV_DENYLIST, CALLER_ENV_DENYLIST_PREFIXES, capture_caller_cwd, capture_caller_env,
    capture_env_from, capture_env_from_os, capture_shell_resume, capture_shell_resume_env,
    launcher_probe_cwd,
};
pub use types::{
    HeadlessSpawnTarget, KillRequest, Lifecycle, LifecycleState, LostEvidence, MountSpec,
    MountSpecParseError, NudgeFailureReason, NudgeOutcome, NudgeRequest, NudgeResponse,
    RuntimeEvent, RuntimeExit, RuntimeKind, RuntimeSignal, RuntimeSignalParseError, ShimExit,
    ShimLaunchRequest, ShimReady, SpawnRequest, SpawnTarget, SpawnTargetParseError,
    TerminationEvidence, TmuxAddress, TmuxAddressParseError, TmuxSpawnTarget,
    ValidateTargetOutcome, ValidateTargetRequest, ValidateTargetResponse, expand_mount_source,
};
pub use version::{
    RUNTIME_PROTOCOL_CAPABILITIES, RUNTIME_PROTOCOL_VERSION, RuntimeCapability, VersionInfo,
    version_info,
};
