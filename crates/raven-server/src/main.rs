use chrono::{TimeZone, Utc};
use tonic::{Request, Response, Status, transport::Server};
use tracing::info;

use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
use raven_proto::proto::{HeartbeatRequest, HeartbeatResponse};
use raven_server::init_subscriber;

#[derive(Debug, Default)]
pub struct RavenServer {}

#[tonic::async_trait]
impl RavenIngestion for RavenServer {
    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        let request = request.into_inner();

        let sent_at = request
            .sent_at
            .ok_or_else(|| Status::invalid_argument("missing sent_at"))?;

        let date_time = Utc
            .timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
            .single()
            .ok_or_else(|| Status::invalid_argument("invalid sent_at timestamp"))?;

        info!(
            hostname = %request.hostname,
            agent_id = %request.agent_id,
            sent_at = %date_time,
            "heartbeat received"
        );

        let response = HeartbeatResponse {
            message: "ok".into(),
            status: "healthy".into(),
        };

        Ok(Response::new(response))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_subscriber("raven-server", "info")?;

    let addr = "0.0.0.0:9090".parse()?;
    let ingestion_server = RavenServer::default();
    let service = RavenIngestionServer::new(ingestion_server);

    info!(listen_addr = %addr, "server starting");
    Server::builder().add_service(service).serve(addr).await?;

    Ok(())
}
