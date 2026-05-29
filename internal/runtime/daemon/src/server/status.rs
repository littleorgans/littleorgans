use lilo_rm_core::{
    Lifecycle, LifecycleLogAvailability, LogAvailability, LogsUnavailableReason, StatusFilter,
};
use lilo_runtime_store::LifecycleStore;

use super::DaemonConfig;

pub(super) struct StatusReader;

impl StatusReader {
    pub(super) fn new() -> Self {
        Self
    }

    pub(super) async fn status(
        &self,
        store: &LifecycleStore,
        config: &DaemonConfig,
        filter: StatusFilter,
    ) -> Vec<Lifecycle> {
        match store.list(&filter).await {
            Ok(mut rows) => {
                self.populate_log_availability_for(config, &mut rows).await;
                rows
            }
            Err(error) => {
                tracing::warn!(%error, "failed to read lifecycle status");
                Vec::new()
            }
        }
    }

    pub(super) async fn log_availability_statuses(
        &self,
        store: &LifecycleStore,
        config: &DaemonConfig,
    ) -> Vec<LifecycleLogAvailability> {
        self.status(store, config, StatusFilter::empty())
            .await
            .into_iter()
            .filter_map(|lifecycle| {
                lifecycle
                    .log_availability
                    .map(|log_availability| LifecycleLogAvailability {
                        session_id: lifecycle.session_id,
                        log_availability,
                    })
            })
            .collect()
    }

    pub(super) async fn populate_log_availability_for(
        &self,
        config: &DaemonConfig,
        lifecycles: &mut [Lifecycle],
    ) {
        for lifecycle in lifecycles {
            self.populate_log_availability(config, lifecycle).await;
        }
    }

    pub(super) async fn populate_log_availability(
        &self,
        config: &DaemonConfig,
        lifecycle: &mut Lifecycle,
    ) {
        let log_availability = if let Some(address) = lifecycle.tmux_pane.as_ref() {
            match lilo_runtime_platform::tmux::TmuxGateway::is_alive(
                config.tmux_server_label.as_deref(),
                address,
            )
            .await
            {
                Ok(true) => LogAvailability::TmuxPaneSnapshot,
                Ok(false) | Err(_) => LogAvailability::Unavailable {
                    reason: LogsUnavailableReason::PaneUnavailable,
                },
            }
        } else {
            let paths = config.session_log_paths(lifecycle.session_id);
            LogAvailability::Headless {
                stdout_path: paths.stdout_path,
                stderr_path: paths.stderr_path,
            }
        };
        lifecycle.log_availability = Some(log_availability);
    }
}
