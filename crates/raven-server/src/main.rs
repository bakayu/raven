use chrono::{TimeZone, Utc};
use tokio::net::unix::ReuniteError;
use tonic::{Request, Response, Status, transport::Server};
use tracing::info;

use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
use raven_proto::proto::{
    HeartbeatRequest, HeartbeatResponse, MetricBatch, RegisterRequest, RegisterResponse,
    StreamResponse,
};
use raven_server::init_subscriber;

#[derive(Debug, Default)]
pub struct RavenServer {}

#[tonic::async_trait]
impl RavenIngestion for RavenServer {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        let request = request.into_inner();

        if request.agent_id.trim().is_empty() || request.hostname.trim().is_empty() {
            return Err(Status::invalid_argument(
                "agent_id and hostname are required",
            ));
        }

        // TODO: currently only logging the info, later this information
        // should be stored in a db.
        info!(
            agent_id = %request.agent_id,
            hostname = %request.hostname,
            os = %request.os,
            agent_version = %request.agent_version,
            log_files = request.log_files.len(),
            "agent registered"
        );

        Ok(Response::new(RegisterResponse {
            ok: true,
            message: "agent registered".to_string(),
        }))
    }

    async fn ingest_metrics(
        &self,
        request: Request<MetricBatch>,
    ) -> Result<Response<StreamResponse>, Status> {
        todo!()
    }

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
            ok: true,
            message: "healthy".into(),
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
