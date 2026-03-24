use std::{sync::Arc, time::Duration};

use tokio::sync::mpsc;
use tracing::{error, warn};

use crate::{AgentConfig, AgentEvent, Collector};

pub async fn collector_task(tx: mpsc::Sender<AgentEvent>, cfg: Arc<AgentConfig>) {
    let mut interval = tokio::time::interval(Duration::from_secs(cfg.metrics.interval_seconds));
    let mut collector = Collector::new();

    loop {
        interval.tick().await;

        match collector.collect().await {
            Ok(output) => {
                if tx
                    .send(AgentEvent::Metrics(output.telemetry))
                    .await
                    .is_err()
                {
                    warn!("collector task exiting: channel closed");
                    break;
                }

                if let Some(inventory) = output.inventory
                    && tx.send(AgentEvent::Inventory(inventory)).await.is_err()
                {
                    warn!("collector inventory send failed: channel closed");
                    break;
                }
            }
            Err(error_value) => {
                error!(error = %error_value, "collector error");
            }
        }
    }
}
