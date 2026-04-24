use std::sync::Arc;

use tokio::sync::{Mutex, mpsc};
use tracing::{debug, error, info};

use crate::{AgentConfig, AgentEvent, LogTailer};

pub async fn logs_task(tx_events: mpsc::Sender<AgentEvent>, cfg: Arc<AgentConfig>) {
    if cfg.logs.is_empty() {
        info!("logs task disabled: no log sources configured");
        return;
    }

    let (tx, rx) = mpsc::channel(cfg.transport.channel_capacity);
    let rx = Arc::new(Mutex::new(rx));

    let tailer = match LogTailer::new_with_drop_oldest(cfg.logs.clone(), tx, rx.clone()) {
        Ok(tailer) => tailer,
        Err(err) => {
            error!(error = %err, "failed to initialize log tailer");
            return;
        }
    };

    let tailer_handle = tokio::spawn(async move { tailer.run().await });

    loop {
        let maybe_batch = {
            let mut guard = rx.lock().await;
            guard.recv().await
        };

        let Some(batch) = maybe_batch else {
            break;
        };

        info!(
            source = %batch.source,
            entries = batch.entries.len(),
            "log batch captured"
        );

        for entry in &batch.entries {
            debug!(
                source = %entry.source,
                path = %entry.path.display(),
                stream = ?entry.stream,
                timestamp = %entry.timestamp.to_rfc3339(),
                line = %entry.line,
                "log entry captured"
            );
        }

        if tx_events.send(AgentEvent::Logs(batch)).await.is_err() {
            error!("transport channel closed, stopping logs task");
            break;
        }
    }

    match tailer_handle.await {
        Ok(Ok(())) => info!("log tailer exited"),
        Ok(Err(err)) => error!(error = %err, "log tailer failed"),
        Err(err) => error!(error = %err, "log tailer task join failed"),
    }
}
