use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::{AgentConfig, LogTailer};

pub async fn logs_task(cfg: Arc<AgentConfig>) {
    if cfg.logs.is_empty() {
        info!("logs task disabled: no log sources configured");
        return;
    }

    let (tx, mut rx) = mpsc::channel(cfg.transport.channel_capacity);

    let tailer = match LogTailer::new(cfg.logs.clone(), tx) {
        Ok(tailer) => tailer,
        Err(err) => {
            error!(error = %err, "failed to initialize log tailer");
            return;
        }
    };

    let tailer_handle = tokio::spawn(async move { tailer.run().await });

    while let Some(batch) = rx.recv().await {
        info!(
        source = %batch.source,
        entries = batch.entries.len(),
        "log batch captured"
        );

        for entry in batch.entries {
            debug!(
            source = %entry.source,
            path = %entry.path.display(),
            stream = ?entry.stream,
            timestamp = %entry.timestamp.to_rfc3339(),
            line = %entry.line,
            "log entry captured"
            );
        }
    }

    match tailer_handle.await {
        Ok(Ok(())) => info!("log tailer exited"),
        Ok(Err(err)) => error!(error = %err, "log tailer failed"),
        Err(err) => error!(error = %err, "log tailer task join failed"),
    }
}
