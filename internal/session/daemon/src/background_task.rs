use std::future::Future;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;

pub(crate) struct BackgroundTask {
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl BackgroundTask {
    pub(crate) fn spawn(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            handle: Mutex::new(Some(tokio::spawn(future))),
        }
    }

    pub(crate) async fn shutdown(&self) {
        let handle = {
            let mut guard = self.handle.lock().await;
            guard.take()
        };

        if let Some(handle) = handle {
            handle.abort();
            if let Err(error) = handle.await
                && !error.is_cancelled()
            {
                tracing::warn!(%error, "background task failed during shutdown");
            }
        }
    }
}

impl Drop for BackgroundTask {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.get_mut().take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::future;
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::BackgroundTask;

    #[tokio::test]
    async fn shutdown_aborts_awaits_and_is_idempotent() {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, mut dropped_rx) = oneshot::channel();
        let task = BackgroundTask::spawn(pending_task(started_tx, dropped_tx));
        started_rx.await.expect("task started");

        task.shutdown().await;
        dropped_rx
            .try_recv()
            .expect("shutdown awaited task cancellation");
        task.shutdown().await;
        drop(task);
    }

    #[tokio::test]
    async fn drop_aborts_when_shutdown_was_not_called() {
        let (started_tx, started_rx) = oneshot::channel();
        let (dropped_tx, dropped_rx) = oneshot::channel();
        let task = BackgroundTask::spawn(pending_task(started_tx, dropped_tx));
        started_rx.await.expect("task started");

        drop(task);

        tokio::time::timeout(Duration::from_secs(1), dropped_rx)
            .await
            .expect("drop aborted task before timeout")
            .expect("task observed cancellation");
    }

    async fn pending_task(started_tx: oneshot::Sender<()>, dropped_tx: oneshot::Sender<()>) {
        let _signal = DropSignal(Some(dropped_tx));
        let _ = started_tx.send(());
        future::pending::<()>().await;
    }

    struct DropSignal(Option<oneshot::Sender<()>>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }
}
