mod common;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::OrPanic as _;
use common::{LOCAL_UID, TestDaemon, local_context, mail_request, spawn_test_session};
use lilo_rm_core::{EventBatch, EventsRequest, Lifecycle, NudgeMode, StatusFilter};
use lilo_session_core::{
    MailIntent, MailNotifyMode, RpcResponse, RuntimeDoctorReport, Selector, SessionRpc,
};
use lilo_session_driver::{
    CaptureResult, ChildExit, NudgeResult, RuntimeError, RuntimePort, SpawnLaunch, SpawnedProcess,
};
use tokio::sync::Barrier;

type TestRuntimeFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, RuntimeError>> + Send + 'a>>;

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn notify_wait_fanout_runs_concurrently_and_preserves_result_order() {
    let daemon = TestDaemon::new(LOCAL_UID).await;
    let context = local_context();
    let sender = spawn_test_session(&daemon, &context, "pm").await;
    let first = spawn_test_session(&daemon, &context, "engineer").await;
    let second = spawn_test_session(&daemon, &context, "engineer").await;
    let third = spawn_test_session(&daemon, &context, "engineer").await;
    let runtime = Arc::new(ConcurrentNudgeRuntimePort::new(3));
    let state = daemon.state_with_runtime_port(Arc::clone(&runtime) as Arc<dyn RuntimePort>);
    let mut request = mail_request(
        Selector::Role {
            name: "engineer".to_string(),
        },
        "review notify fanout",
        "notify-fanout",
        MailIntent::Inform,
    );
    request.notify = Some(MailNotifyMode::Wait);
    request.timeout_ms = Some(1_000);

    let response = tokio::time::timeout(
        Duration::from_secs(1),
        state.handle(
            context.with_mcp_caller_session_id(sender.id),
            SessionRpc::MailSend { request },
        ),
    )
    .await
    .or_panic("notify fanout should not serialize at the barrier");
    let RpcResponse::MailSent { response } = response.response else {
        panic!("expected mail sent response");
    };

    assert_eq!(runtime.peak_in_flight(), 3);
    assert_eq!(runtime.timeouts(), vec![Some(1_000); 3]);
    assert_eq!(
        response
            .results
            .iter()
            .map(|result| result.recipient.session_id)
            .collect::<Vec<_>>(),
        vec![first.id, second.id, third.id]
    );
    daemon.cleanup().await;
}

struct ConcurrentNudgeRuntimePort {
    barrier: Barrier,
    in_flight: AtomicUsize,
    peak_in_flight: AtomicUsize,
    timeouts: Mutex<Vec<Option<u64>>>,
}

impl ConcurrentNudgeRuntimePort {
    fn new(expected: usize) -> Self {
        Self {
            barrier: Barrier::new(expected),
            in_flight: AtomicUsize::new(0),
            peak_in_flight: AtomicUsize::new(0),
            timeouts: Mutex::new(Vec::new()),
        }
    }

    fn peak_in_flight(&self) -> usize {
        self.peak_in_flight.load(Ordering::SeqCst)
    }

    fn timeouts(&self) -> Vec<Option<u64>> {
        self.timeouts.lock().or_panic("timeouts lock").clone()
    }
}

impl RuntimePort for ConcurrentNudgeRuntimePort {
    fn spawn<'a>(
        &'a self,
        _session_id: &'a str,
        _launch: &'a SpawnLaunch,
    ) -> TestRuntimeFuture<'a, SpawnedProcess> {
        unsupported("spawn")
    }

    fn reap_exited(&self) -> TestRuntimeFuture<'_, Vec<ChildExit>> {
        unsupported("reap_exited")
    }

    fn capture<'a>(
        &'a self,
        _session_id: &'a str,
        _scrollback_lines: Option<u32>,
    ) -> TestRuntimeFuture<'a, CaptureResult> {
        unsupported("capture")
    }

    fn terminate<'a>(
        &'a self,
        _session_id: &'a str,
        _signal: &'a str,
        _grace: Duration,
    ) -> TestRuntimeFuture<'a, Option<ChildExit>> {
        unsupported("terminate")
    }

    fn nudge<'a>(
        &'a self,
        _session_id: &'a str,
        _content: &'a str,
        _mode: NudgeMode,
        timeout_ms: Option<u64>,
    ) -> TestRuntimeFuture<'a, NudgeResult> {
        Box::pin(async move {
            self.timeouts
                .lock()
                .or_panic("timeouts lock")
                .push(timeout_ms);
            let in_flight = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak_in_flight.fetch_max(in_flight, Ordering::SeqCst);
            self.barrier.wait().await;
            self.in_flight.fetch_sub(1, Ordering::SeqCst);
            Ok(NudgeResult {
                delivered: true,
                message: "delivered".to_string(),
            })
        })
    }

    fn status(&self, _filter: StatusFilter) -> TestRuntimeFuture<'_, Vec<Lifecycle>> {
        unsupported("status")
    }

    fn poll_events(&self, _request: EventsRequest) -> TestRuntimeFuture<'_, EventBatch> {
        unsupported("poll_events")
    }

    fn doctor(&self) -> TestRuntimeFuture<'_, RuntimeDoctorReport> {
        unsupported("doctor")
    }

    fn terminate_all(&self) {}
}

fn unsupported<T: Send + 'static>(operation: &'static str) -> TestRuntimeFuture<'static, T> {
    Box::pin(async move {
        Err(RuntimeError::local(format!(
            "unsupported test runtime operation {operation}"
        )))
    })
}
