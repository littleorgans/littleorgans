use std::path::Path;
use std::time::{Duration, Instant};

use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{EventBatch, EventsRequest, RuntimeRpc, StatusFilter};
use lilo_runtime_daemon::{
    DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig, run_daemon,
};
use lilo_runtime_store::StoreConfig;
use tokio::net::UnixStream;

struct TestDaemon {
    client: RuntimeClient,
    task: tokio::task::JoinHandle<()>,
    _tempdir: tempfile::TempDir,
}

impl TestDaemon {
    async fn start() -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let socket_path = tempdir.path().join("rtmd.sock");
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(socket_path.clone()),
            shim_path: std::env::current_exe().expect("current test executable"),
            log_root: tempdir.path().join("logs"),
            store: StoreConfig {
                db_path: tempdir.path().join("rtm.sqlite"),
            },
            reconcile: ReconcileConfig::default(),
            docker_preflight: DockerPreflightConfig::default(),
        };
        let task = tokio::spawn(async move {
            run_daemon(config).await.expect("daemon run");
        });
        wait_for_socket(&socket_path).await;
        Self {
            client: RuntimeClient::new(socket_path),
            task,
            _tempdir: tempdir,
        }
    }

    async fn stop(self) {
        let response = self
            .client
            .request(RuntimeRpc::Stop)
            .await
            .expect("stop daemon");
        assert_eq!(response, lilo_rm_core::RuntimeResponse::Stopping);
        self.task.await.expect("daemon task");
    }
}

async fn wait_for_socket(socket_path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last_error = None;
    while Instant::now() < deadline {
        match UnixStream::connect(socket_path).await {
            Ok(_) => return,
            Err(error) => {
                last_error = Some(error);
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    panic!(
        "daemon socket never accepted connections at {}; last error={last_error:?}",
        socket_path.display()
    );
}

#[tokio::test]
async fn typed_helpers_round_trip_against_real_daemon() {
    let daemon = TestDaemon::start().await;

    let version = daemon.client.version().await.expect("version helper");
    assert_eq!(version.version.protocol_version, "0.6");

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
