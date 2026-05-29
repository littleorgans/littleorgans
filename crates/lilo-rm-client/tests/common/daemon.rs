use std::path::Path;
use std::time::{Duration, Instant};

use lilo_rm_client::RuntimeClient;
use lilo_rm_core::{RuntimeResponse, RuntimeRpc};
use lilo_runtime_daemon::{
    DaemonConfig, ReconcileConfig, docker_preflight::DockerPreflightConfig, run_daemon,
};
use lilo_runtime_store::StoreConfig;
use tokio::net::UnixStream;
use tokio::task::JoinHandle;

pub struct TestDaemon {
    pub client: RuntimeClient,
    task: JoinHandle<()>,
    _tempdir: tempfile::TempDir,
}

#[allow(dead_code)]
impl TestDaemon {
    pub async fn start() -> Self {
        Self::start_with_data(|_| {}).await
    }

    pub async fn start_with_data(prepare_data: impl FnOnce(&Path)) -> Self {
        let tempdir = tempfile::tempdir().expect("tempdir");
        let socket_path = tempdir.path().join("rtmd.sock");
        prepare_data(tempdir.path());
        let config = DaemonConfig {
            endpoint: lilo_paths::RuntimeEndpoint::unix_socket(socket_path.clone()),
            shim_path: std::env::current_exe().expect("current test executable"),
            log_root: tempdir.path().join("logs"),
            store: StoreConfig {
                db_path: tempdir.path().join("rtm.sqlite"),
            },
            reconcile: ReconcileConfig::default(),
            docker_preflight: DockerPreflightConfig::default(),
            tmux_server_label: None,
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

    pub fn client(&self) -> RuntimeClient {
        self.client.clone()
    }

    pub async fn stop(self) {
        let response = self
            .client
            .request(RuntimeRpc::Stop)
            .await
            .expect("stop daemon");
        assert_eq!(response, RuntimeResponse::Stopping);
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
