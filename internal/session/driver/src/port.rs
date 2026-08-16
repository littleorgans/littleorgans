use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use lilo_common::id::SessionId;
use lilo_rm_core::{
    EventBatch, EventsRequest, Lifecycle, NudgeMode, RuntimeSignal,
    SpawnRequest as RuntimeSpawnRequest, StatusFilter,
};
use lilo_session_core::RuntimeDoctorReport;
use tokio::time::{Instant, sleep};

use crate::conv::{status_session, terminal_child_exit};
use crate::driver::{CaptureResult, ChildExit, NudgeResult, RuntimeError, SpawnedProcess};

pub type RuntimePortFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

pub trait RuntimePort: Send + Sync {
    fn spawn(&self, request: RuntimeSpawnRequest) -> RuntimePortFuture<'_, SpawnedProcess>;

    fn reap_exited(&self) -> RuntimePortFuture<'_, Vec<ChildExit>>;

    fn capture(
        &self,
        session_id: SessionId,
        scrollback_lines: Option<u32>,
    ) -> RuntimePortFuture<'_, CaptureResult>;

    fn terminate(
        &self,
        session_id: SessionId,
        signal: RuntimeSignal,
        grace: Duration,
    ) -> RuntimePortFuture<'_, Option<ChildExit>>;

    fn nudge<'a>(
        &'a self,
        session_id: SessionId,
        content: &'a str,
        mode: NudgeMode,
        timeout_ms: Option<u64>,
    ) -> RuntimePortFuture<'a, NudgeResult>;

    fn status(&self, filter: StatusFilter) -> RuntimePortFuture<'_, Vec<Lifecycle>>;

    fn poll_events(&self, request: EventsRequest) -> RuntimePortFuture<'_, EventBatch>;

    fn doctor(&self) -> RuntimePortFuture<'_, RuntimeDoctorReport>;

    fn terminate_all(&self);
}

pub async fn wait_for_terminal<P: RuntimePort + ?Sized>(
    port: &P,
    session_id: SessionId,
    grace: Duration,
) -> Result<Option<ChildExit>, RuntimeError> {
    let timeout = grace.max(Duration::from_secs(1));
    let deadline = Instant::now() + timeout;
    loop {
        let lifecycles = port.status(status_session(session_id)).await?;
        let exit = lifecycles
            .iter()
            .find(|lifecycle| lifecycle.session_id == session_id)
            .map(terminal_child_exit)
            .transpose()?
            .flatten();
        if exit.is_some() || Instant::now() >= deadline {
            return Ok(exit);
        }
        sleep(Duration::from_millis(100)).await;
    }
}
