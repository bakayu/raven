use chrono::Utc;
use prost_types::Timestamp;
use tokio::sync::mpsc;

use raven_proto::proto::HeartbeatRequest;
use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;

use crate::AgentEvent;

pub async fn transport_task(mut rx: mpsc::Receiver<AgentEvent>) {
    let mut client = RavenIngestionClient::connect("http://localhost:9090")
        .await
        .unwrap();

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::Heartbeat { agent_id, hostname } => {
                let now = Utc::now();

                let response = client
                    .heartbeat(HeartbeatRequest {
                        agent_id,
                        hostname,
                        sent_at: Some(Timestamp {
                            seconds: now.timestamp(),
                            nanos: now.timestamp_subsec_nanos() as i32,
                        }),
                    })
                    .await
                    .unwrap();

                println!("HEARTBEAT RESPONSE: {:?}", response.into_inner());
            }
            AgentEvent::Metrics(snapshot) => {
                println!("METRICS: {:#?}", snapshot);
            }
            AgentEvent::Inventory(snapshot) => {
                println!("INVENTORY: {:#?}", snapshot);
            }
        }
    }
}
