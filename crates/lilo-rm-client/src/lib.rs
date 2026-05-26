#![forbid(unsafe_code)]

//! Async Unix socket client for the public rtmd JSON line contract.
//!
//! `lilo-rm-client` owns connection setup, newline delimited JSON framing, and
//! typed client side error normalization. Protocol request and response shapes
//! remain in `lilo-rm-core`.

use std::path::{Path, PathBuf};

use lilo_rm_core::{
    CaptureRequest, CaptureResponse, DoctorPayload, ErrorCode, EventBatch, EventsRequest,
    KillByPidRequest, KillOutcome, KillRequest, ProtocolError, RUNTIME_PROTOCOL_VERSION,
    RuntimeResponse, RuntimeRpc, SpawnRequest, SpawnedPayload, StatusFilter, StatusPayload,
    ValidateTargetRequest, ValidateTargetResponse, VersionPayload, read_json_line, write_json_line,
};
use thiserror::Error;
use tokio::io::BufReader;
use tokio::net::UnixStream;

mod event_watcher;

pub use event_watcher::{EventWatcher, EventWatcherBuilder};

/// Async client for the rtmd Unix socket JSON line protocol.
#[derive(Clone, Debug)]
pub struct RuntimeClient {
    socket_path: PathBuf,
}

impl RuntimeClient {
    /// Create a client connected to `socket_path`.
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    /// Return the Unix socket path this client connects to.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Send a raw protocol request and return the raw protocol response.
    pub async fn request(&self, rpc: RuntimeRpc) -> Result<RuntimeResponse, ClientError> {
        request(self.socket_path(), rpc).await
    }

    /// Spawn a runtime session and return the created lifecycle payload.
    pub async fn spawn(&self, request: SpawnRequest) -> Result<SpawnedPayload, ClientError> {
        match self.request(RuntimeRpc::Spawn { request }).await? {
            RuntimeResponse::Spawned(payload) => Ok(payload),
            RuntimeResponse::SpawnConflict(payload) => {
                Err(ClientError::SpawnConflict(Box::new(payload)))
            }
            response => unexpected_response("Spawned", &response),
        }
    }

    /// Kill a runtime session by session id.
    pub async fn kill(&self, request: KillRequest) -> Result<KillOutcome, ClientError> {
        match self.request(RuntimeRpc::Kill { request }).await? {
            RuntimeResponse::Killed(payload) => Ok(payload.outcome),
            response => unexpected_response("Killed", &response),
        }
    }

    /// Kill an arbitrary process id through the daemon admin path.
    pub async fn kill_by_pid(&self, request: KillByPidRequest) -> Result<KillOutcome, ClientError> {
        match self.request(RuntimeRpc::KillByPid { request }).await? {
            RuntimeResponse::KillByPid(payload) => Ok(payload.response.outcome),
            response => unexpected_response("KillByPid", &response),
        }
    }

    /// Query runtime lifecycle status.
    pub async fn status(&self, filter: StatusFilter) -> Result<StatusPayload, ClientError> {
        match self
            .request(RuntimeRpc::Status {
                request: filter.into(),
            })
            .await?
        {
            RuntimeResponse::Status(payload) => Ok(payload),
            response => unexpected_response("Status", &response),
        }
    }

    /// Send a text nudge to a runtime session and return the delivery outcome.
    pub async fn nudge(
        &self,
        request: lilo_rm_core::NudgeRequest,
    ) -> Result<lilo_rm_core::NudgeResponse, ClientError> {
        match self.request(RuntimeRpc::Nudge { request }).await? {
            RuntimeResponse::Nudge(payload) => Ok(payload.response),
            response => unexpected_response("Nudge", &response),
        }
    }

    /// Capture scrollback for a runtime session.
    pub async fn capture(&self, request: CaptureRequest) -> Result<CaptureResponse, ClientError> {
        match self.request(RuntimeRpc::Capture { request }).await? {
            RuntimeResponse::Capture(payload) => Ok(payload.response),
            response => unexpected_response("Capture", &response),
        }
    }

    /// Validate a user supplied spawn target string.
    pub async fn validate_target(
        &self,
        target: &str,
    ) -> Result<ValidateTargetResponse, ClientError> {
        match self
            .request(RuntimeRpc::ValidateTarget {
                request: ValidateTargetRequest {
                    target: target.to_owned(),
                },
            })
            .await?
        {
            RuntimeResponse::ValidateTarget(payload) => Ok(payload.response),
            response => unexpected_response("ValidateTarget", &response),
        }
    }

    /// Query daemon diagnostics.
    pub async fn doctor(&self) -> Result<DoctorPayload, ClientError> {
        match self.request(RuntimeRpc::Doctor).await? {
            RuntimeResponse::Doctor(payload) => Ok(payload),
            response => unexpected_response("Doctor", &response),
        }
    }

    /// Query the daemon version and protocol capability payload.
    pub async fn version(&self) -> Result<VersionPayload, ClientError> {
        match self.request(RuntimeRpc::Version).await? {
            RuntimeResponse::Version(payload) => Ok(payload),
            response => unexpected_response("Version", &response),
        }
    }

    /// Query one batch of lifecycle events.
    pub async fn events(&self, request: EventsRequest) -> Result<EventBatch, ClientError> {
        match self.request(RuntimeRpc::Events { request }).await? {
            RuntimeResponse::Events(payload) => Ok(EventBatch::Events {
                events: payload.events,
                cursor: payload.cursor,
            }),
            RuntimeResponse::CursorExpired(payload) => Ok(EventBatch::CursorExpired {
                oldest: payload.oldest,
            }),
            response => unexpected_response("Events or CursorExpired", &response),
        }
    }

    async fn check_protocol_version(&self) -> Result<(), ClientError> {
        let payload = self.version().await?;
        let got = payload.version.protocol_version;
        if got == RUNTIME_PROTOCOL_VERSION {
            Ok(())
        } else {
            Err(ClientError::Protocol {
                source: ProtocolError::UnsupportedVersion {
                    expected: RUNTIME_PROTOCOL_VERSION,
                    got,
                },
            })
        }
    }
}

/// Error returned by the rtmd client.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClientError {
    /// The configured daemon socket could not be reached.
    #[error("rtmd unavailable at {socket_path}: {source}")]
    DaemonUnavailable {
        /// Socket path the client tried to connect to.
        socket_path: PathBuf,
        #[source]
        /// Underlying I/O error.
        source: std::io::Error,
    },
    /// The daemon or transport returned malformed protocol data.
    #[error("rtmd protocol error: {source}")]
    Protocol {
        #[from]
        /// Underlying protocol error.
        source: ProtocolError,
    },
    /// The daemon returned an explicit error response.
    #[error("rtmd returned {code}: {message}")]
    ErrorResponse { code: ErrorCode, message: String },
    /// The daemon refused a spawn because the requested identity or pane is already occupied.
    #[error("rtmd spawn conflict: {0:?}")]
    SpawnConflict(Box<lilo_rm_core::SpawnConflictPayload>),
    /// A typed helper received a different response variant than expected.
    #[error("expected {expected} response, got {got}")]
    UnexpectedResponse {
        /// Response variant the helper expected.
        expected: &'static str,
        /// Response variant the daemon returned.
        got: &'static str,
    },
}

impl ClientError {
    /// Return the stable runtime error code represented by this client error.
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::DaemonUnavailable { .. } => ErrorCode::RuntimeUnavailable,
            Self::Protocol { .. } | Self::UnexpectedResponse { .. } => ErrorCode::ProtocolMismatch,
            Self::ErrorResponse { code, .. } => *code,
            Self::SpawnConflict(_) => ErrorCode::SpawnConflict,
        }
    }
}

/// Send a raw protocol request to `socket_path`.
pub async fn request(
    socket_path: impl AsRef<Path>,
    rpc: RuntimeRpc,
) -> Result<RuntimeResponse, ClientError> {
    let socket_path = socket_path.as_ref();
    let stream = UnixStream::connect(socket_path).await.map_err(|source| {
        ClientError::DaemonUnavailable {
            socket_path: socket_path.to_path_buf(),
            source,
        }
    })?;
    request_on_stream(stream, rpc).await
}

fn unexpected_response<T>(
    expected: &'static str,
    response: &RuntimeResponse,
) -> Result<T, ClientError> {
    Err(ClientError::UnexpectedResponse {
        expected,
        got: response_name(response),
    })
}

fn response_name(response: &RuntimeResponse) -> &'static str {
    match response {
        RuntimeResponse::Spawned(_) => "Spawned",
        RuntimeResponse::SpawnConflict(_) => "SpawnConflict",
        RuntimeResponse::ValidateTarget(_) => "ValidateTarget",
        RuntimeResponse::Status(_) => "Status",
        RuntimeResponse::Killed(_) => "Killed",
        RuntimeResponse::KillByPid(_) => "KillByPid",
        RuntimeResponse::Nudge(_) => "Nudge",
        RuntimeResponse::Capture(_) => "Capture",
        RuntimeResponse::Version(_) => "Version",
        RuntimeResponse::Watchers(_) => "Watchers",
        RuntimeResponse::Doctor(_) => "Doctor",
        RuntimeResponse::Events(_) => "Events",
        RuntimeResponse::CursorExpired(_) => "CursorExpired",
        RuntimeResponse::McpBridge(_) => "McpBridge",
        RuntimeResponse::ShimLaunch(_) => "ShimLaunch",
        RuntimeResponse::Ack => "Ack",
        RuntimeResponse::Stopping => "Stopping",
        RuntimeResponse::Error(_) => "Error",
        _ => "Unknown",
    }
}

async fn request_on_stream(
    stream: UnixStream,
    rpc: RuntimeRpc,
) -> Result<RuntimeResponse, ClientError> {
    let (read_half, mut write_half) = stream.into_split();
    write_json_line(&mut write_half, &rpc).await?;

    let mut reader = BufReader::new(read_half);
    match read_json_line(&mut reader).await? {
        RuntimeResponse::Error(payload) => {
            let code = payload.code;
            let message = payload.message;
            Err(ClientError::ErrorResponse { code, message })
        }
        response => Ok(response),
    }
}
