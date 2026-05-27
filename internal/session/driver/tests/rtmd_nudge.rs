mod common;

use common::OrPanic as _;
use lilo_rm_core::{
    NudgeFailureReason, NudgeOutcome, NudgePayload, NudgeRequest, NudgeResponse, RuntimeResponse,
    RuntimeRpc, read_json_line, write_json_line,
};
use lilo_session_driver::{RtmdDriver, SpawnDriver};
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::task::JoinHandle;
use uuid::Uuid;

#[tokio::test]
async fn rtmd_nudge_maps_delivered_outcome() {
    let session_id = Uuid::now_v7();
    let (driver, server) = mock_rtmd_nudge(session_id, NudgeOutcome::Delivered);

    let result = driver
        .nudge(&session_id.to_string(), "hello")
        .await
        .or_panic("nudge delegates to rtmd");

    assert!(result.delivered);
    assert_eq!(result.message, "delivered");
    server.await.or_panic("server task");
}

#[tokio::test]
async fn rtmd_nudge_maps_tmux_pane_dead_outcome() {
    let session_id = Uuid::now_v7();
    let (driver, server) = mock_rtmd_nudge(
        session_id,
        NudgeOutcome::Failed(NudgeFailureReason::TmuxPaneDead),
    );

    let result = driver
        .nudge(&session_id.to_string(), "hello")
        .await
        .or_panic("failed nudge outcome remains a response");

    assert!(!result.delivered);
    assert_eq!(result.message, "tmux pane is no longer available");
    server.await.or_panic("server task");
}

fn mock_rtmd_nudge(session_id: Uuid, outcome: NudgeOutcome) -> (RtmdDriver, JoinHandle<()>) {
    let tempdir = tempfile::tempdir().or_panic("tempdir");
    let socket_path = tempdir.path().join("rtmd.sock");
    let listener = UnixListener::bind(&socket_path).or_panic("bind test socket");
    let driver = RtmdDriver::new(socket_path);
    let server = tokio::spawn(async move {
        let _tempdir = tempdir;
        let (stream, _) = listener.accept().await.or_panic("accept client");
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let rpc: RuntimeRpc = read_json_line(&mut reader).await.or_panic("read rpc");
        assert_eq!(
            rpc,
            RuntimeRpc::Nudge {
                request: NudgeRequest {
                    session_id,
                    content: "hello".to_string(),
                },
            }
        );
        write_json_line(
            &mut write_half,
            &RuntimeResponse::Nudge(NudgePayload {
                response: NudgeResponse {
                    delivered: matches!(outcome, NudgeOutcome::Delivered),
                    outcome,
                },
            }),
        )
        .await
        .or_panic("write response");
    });
    (driver, server)
}
