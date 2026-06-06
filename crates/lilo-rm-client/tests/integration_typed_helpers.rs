#[path = "common/daemon.rs"]
mod daemon;

use daemon::TestDaemon;
use lilo_rm_core::{EventBatch, EventsRequest, StatusFilter};

#[tokio::test]
#[ignore = "requires Postgres: set LILO_TEST_DATABASE_URL; run with --run-ignored all"]
async fn typed_helpers_round_trip_against_real_daemon() {
    let daemon = TestDaemon::start().await;

    let version = daemon.client.version().await.expect("version helper");
    assert_eq!(version.version.protocol_version, "0.7");

    let status = daemon
        .client
        .status(StatusFilter::default())
        .await
        .expect("status helper");
    assert!(status.lifecycles.is_empty());

    let events = daemon
        .client
        .events(EventsRequest {
            since: None,
            wait_ms: Some(0),
        })
        .await
        .expect("events helper");
    assert_eq!(
        events,
        EventBatch::Events {
            events: Vec::new(),
            cursor: 0
        }
    );

    daemon.stop().await;
}
