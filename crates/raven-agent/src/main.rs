use std::error::Error;
use std::time::Duration;

use chrono::Utc;
use prost_types::Timestamp;

use raven_agent::Collector;
use raven_proto::proto::HeartbeatRequest;
use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut client = RavenIngestionClient::connect("http://localhost:9090").await?;

    let mut interval = tokio::time::interval(Duration::from_millis(2_000));

    let mut proc_collector = Collector::new();

    loop {
        interval.tick().await;

        let stats = proc_collector.collect().await?;
        println!("{:?}", stats);

        let now = Utc::now();
        let timestamp = Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        };

        let response = client
            .heartbeat(HeartbeatRequest {
                agent_id: "agent-1".into(),
                hostname: "my-machine".into(),
                sent_at: Some(timestamp),
            })
            .await?;

        println!("RESPONSE: {:?}", response.into_inner());
    }

    Ok(())
}
