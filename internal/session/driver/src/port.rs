use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use lilo_rm_core::{EventBatch, EventsRequest, Lifecycle, StatusFilter};
use lilo_session_core::RuntimeDoctorReport;
use tokio::time::{Instant, sleep};
use uuid::Uuid;

use crate::conv::{status_session, terminal_child_exit};
use crate::driver::{
    CaptureResult, ChildExit, DriverError, NudgeResult, SpawnLaunch, SpawnedProcess,
};

pub type RuntimePortFuture<'a, T> =
    Pin<Box<dyn Future<Output = Result<T, DriverError>> + Send + 'a>>;

pub trait RuntimePort: Send + Sync {
    fn spawn<'a>(
        &'a self,
        session_id: &'a str,
        launch: &'a SpawnLaunch,
    ) -> RuntimePortFuture<'a, SpawnedProcess>;

    fn reap_exited(&self) -> RuntimePortFuture<'_, Vec<ChildExit>>;

    fn capture<'a>(
        &'a self,
        session_id: &'a str,
        scrollback_lines: Option<u32>,
    ) -> RuntimePortFuture<'a, CaptureResult>;

    fn terminate<'a>(
        &'a self,
        session_id: &'a str,
        signal: &'a str,
        grace: Duration,
    ) -> RuntimePortFuture<'a, Option<ChildExit>>;

    fn nudge<'a>(
        &'a self,
        session_id: &'a str,
        content: &'a str,
    ) -> RuntimePortFuture<'a, NudgeResult>;

    fn status(&self, filter: StatusFilter) -> RuntimePortFuture<'_, Vec<Lifecycle>>;

    fn poll_events(&self, request: EventsRequest) -> RuntimePortFuture<'_, EventBatch>;

    fn doctor(&self) -> RuntimePortFuture<'_, RuntimeDoctorReport>;

    fn terminate_all(&self);
}

pub async fn wait_for_terminal<P: RuntimePort + ?Sized>(
    port: &P,
    session_id: Uuid,
    grace: Duration,
) -> Result<Option<ChildExit>, DriverError> {
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
