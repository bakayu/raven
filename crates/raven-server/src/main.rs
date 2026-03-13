use std::error::Error;

use chrono::{TimeZone, Utc};
use tonic::{Request, Response, Status, transport::Server};

use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
use raven_proto::proto::{HeartbeatRequest, HeartbeatResponse};

#[derive(Debug, Default)]
pub struct RavenServer {}

#[tonic::async_trait]
impl RavenIngestion for RavenServer {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();

        let sent_at = request.sent_at.unwrap();
        let date_time = Utc
            .timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
            .unwrap();
        println!(
            "HEARTBEAT from {} ({}) at {}",
            request.hostname, request.agent_id, date_time
        );

        let response = HeartbeatResponse {
            message: "ok".into(),
            status: "healthy".into(),
        };

        Ok(Response::new(response))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let addr = "0.0.0.0:9090".parse()?;
    let ingestion_server = RavenServer::default();

    let service = RavenIngestionServer::new(ingestion_server);

    Server::builder().add_service(service).serve(addr).await?;

    Ok(())
}
