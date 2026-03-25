use std::{sync::Arc, time::Duration};

use chrono::Utc;
use prost_types::Timestamp;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use raven_proto::proto::HeartbeatRequest;
use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;

use crate::{AgentConfig, AgentEvent};

pub async fn transport_task(mut rx: mpsc::Receiver<AgentEvent>, cfg: Arc<AgentConfig>) {
    let max_backoff = Duration::from_secs(cfg.transport.retry_max_interval_seconds.max(1));
    let mut backoff = Duration::from_secs(1);

    loop {
        let endpoint = format!("http://{}", cfg.server.address);

        match RavenIngestionClient::connect(endpoint.clone()).await {
            Ok(mut client) => {
                info!(server = %cfg.server.address, "transport connected");
                backoff = Duration::from_secs(1);

                while let Some(event) = rx.recv().await {
                    match event {
                        AgentEvent::Heartbeat { agent_id, hostname } => {
                            let now = Utc::now();

                            let request = HeartbeatRequest {
                                agent_id,
                                hostname,
                                sent_at: Some(Timestamp {
                                    seconds: now.timestamp(),
                                    nanos: now.timestamp_subsec_nanos() as i32,
                                }),
                            };

                            if let Err(error_value) = client.heartbeat(request).await {
                                warn!(error = %error_value, "heartbeat send failed; reconnecting");
                                break;
                            }
                        }
                        AgentEvent::Metrics(snapshot) => {
                            debug!(?snapshot, "metrics event queued for transport");
                        }
                        AgentEvent::Inventory(snapshot) => {
                            debug!(?snapshot, "inventory event queued for transport");
                        }
                    }
                }

                if rx.is_closed() {
                    info!("transport exiting: receiver closed");
                    return;
                }
            }
            Err(error_value) => {
                warn!(
                    error = %error_value,
                    server = %cfg.server.address,
                    retry_seconds = backoff.as_secs(),
                    "transport connect failed"
                );
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}
