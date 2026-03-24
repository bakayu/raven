use std::{sync::Arc, time::Duration};

use tokio::sync::mpsc;
use tracing::warn;

use crate::{AgentConfig, AgentEvent};

pub async fn heartbeat_task(tx: mpsc::Sender<AgentEvent>, cfg: Arc<AgentConfig>) {
    let mut interval = tokio::time::interval(Duration::from_secs(
        cfg.transport.heartbeat_interval_seconds,
    ));

    let hostname = hostname::get()
        .ok()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown-host".to_string());

    let agent_id = hostname.clone();

    loop {
        interval.tick().await;

        if tx
            .send(AgentEvent::Heartbeat {
                agent_id: agent_id.clone(),
                hostname: hostname.clone(),
            })
            .await
            .is_err()
        {
            warn!("heartbeat task exiting: channel closed");
            break;
        }
    }
}
