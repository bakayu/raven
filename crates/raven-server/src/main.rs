use chrono::{TimeZone, Utc};
use tonic::{Request, Response, Status, transport::Server};
use tracing::{debug, info};

use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
use raven_proto::proto::{
    HeartbeatRequest, HeartbeatResponse, LogBatch, LogStream as ProtoLogStream, MetricBatch,
    RegisterRequest, RegisterResponse, StreamResponse,
};
use raven_server::init_subscriber;

#[derive(Debug, Clone)]
pub struct RavenServer {
    expected_token: String,
}

impl RavenServer {
    pub fn new(expected_token: String) -> Self {
        Self { expected_token }
    }

    fn authorize<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let header = request
            .metadata()
            .get("authorization")
            .ok_or_else(|| Status::unauthenticated("missing authorization metadata"))?;

        let value = header
            .to_str()
            .map_err(|_| Status::unauthenticated("invalid authorization metadata"))?;

        let token = value
            .strip_prefix("Bearer ")
            .ok_or_else(|| Status::unauthenticated("authorization scheme must be Bearer"))?;

        if token != self.expected_token {
            return Err(Status::unauthenticated("invalid bearer token"));
        }

        Ok(())
    }
}

impl Default for RavenServer {
    fn default() -> Self {
        Self::new("rvn_dev_token".to_string())
    }
}

#[tonic::async_trait]
impl RavenIngestion for RavenServer {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        self.authorize(&request)?;

        let request = request.into_inner();

        if request.agent_id.trim().is_empty()
            || request.hostname.trim().is_empty()
            || request.os.trim().is_empty()
            || request.agent_version.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "agent_id, hostname, os and agent_version are required",
            ));
        }

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
        self.authorize(&request)?;

        let batch = request.into_inner();

        let sent_at = batch
            .sent_at
            .ok_or_else(|| Status::invalid_argument("missing sent_at"))?;

        let date_time = Utc
            .timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
            .single()
            .ok_or_else(|| Status::invalid_argument("invalid sent_at timestamp"))?;

        let cpu_total = batch
            .cpu
            .as_ref()
            .map(|c| c.total_usage_percent)
            .unwrap_or_default();

        // TODO: this is just for debugging, included some of the info from MetricBatch
        debug!(
            agent_id = %batch.agent_id,
            hostname = %batch.hostname,
            sent_at = %date_time,
            cpu_total = cpu_total,
            disk_io = batch.disk_io.len(),
            filesystems = batch.filesystems.len(),
            interfaces = batch.network_interfaces.len(),
            "metrics batch received"
        );

        Ok(Response::new(StreamResponse {
            ok: true,
            message: "metrics accepted".to_string(),
        }))
    }

    async fn ingest_logs(
        &self,
        request: Request<LogBatch>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request)?;

        let batch = request.into_inner();

        if batch.agent_id.trim().is_empty()
            || batch.hostname.trim().is_empty()
            || batch.source.trim().is_empty()
        {
            return Err(Status::invalid_argument(
                "agent_id, hostname and source are required",
            ));
        }

        let sent_at = batch
            .sent_at
            .ok_or_else(|| Status::invalid_argument("missing sent_at"))?;

        let date_time = Utc
            .timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
            .single()
            .ok_or_else(|| Status::invalid_argument("invalid sent_at timestamp"))?;

        let stdout_count = batch
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    ProtoLogStream::try_from(e.stream),
                    Ok(ProtoLogStream::Stdout)
                )
            })
            .count();

        let stderr_count = batch
            .entries
            .iter()
            .filter(|e| {
                matches!(
                    ProtoLogStream::try_from(e.stream),
                    Ok(ProtoLogStream::Stderr)
                )
            })
            .count();

        info!(
            agent_id = %batch.agent_id,
            hostname = %batch.hostname,
            source = %batch.source,
            entries = batch.entries.len(),
            stdout_entries = stdout_count,
            stderr_entries = stderr_count,
            sent_at = %date_time,
            "logs batch received"
        );

        Ok(Response::new(StreamResponse {
            ok: true,
            message: "logs accepted".to_string(),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        self.authorize(&request)?;

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
    let expected_token =
        std::env::var("RAVEN_AGENT_TOKEN").unwrap_or_else(|_| "rvn_dev_token".to_string());
    let ingestion_server = RavenServer::new(expected_token);
    let service = RavenIngestionServer::new(ingestion_server);

    info!(listen_addr = %addr, "server starting");
    Server::builder().add_service(service).serve(addr).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Timestamp;
    use raven_proto::proto::raven_ingestion_server::RavenIngestion;
    use tonic::{Code, Request};

    fn with_auth<T>(payload: T) -> Request<T> {
        let mut request = Request::new(payload);
        request.metadata_mut().insert(
            "authorization",
            "Bearer rvn_test_token"
                .parse()
                .expect("valid metadata value"),
        );
        request
    }

    #[tokio::test]
    async fn register_rejects_missing_auth() {
        let server = RavenServer::new("rvn_test_token".to_string());

        let req = RegisterRequest {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            os: "linux".to_string(),
            agent_version: "0.1.0".to_string(),
            log_files: vec![],
        };

        let err = server.register(Request::new(req)).await.unwrap_err();
        assert_eq!(err.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn register_rejects_missing_identity() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = RegisterRequest {
            agent_id: "".to_string(),
            hostname: "".to_string(),
            os: "linux".to_string(),
            agent_version: "0.1.0".to_string(),
            log_files: vec![],
        };

        let err = server.register(with_auth(req)).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn heartbeat_rejects_missing_timestamp() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = HeartbeatRequest {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            sent_at: None,
        };

        let err = server.heartbeat(with_auth(req)).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn heartbeat_accepts_valid_timestamp() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = HeartbeatRequest {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            sent_at: Some(Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
        };

        let resp = server.heartbeat(with_auth(req)).await.unwrap().into_inner();
        assert!(resp.ok);
    }

    #[tokio::test]
    async fn ingest_metrics_rejects_missing_timestamp() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = MetricBatch {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            sent_at: None,
            ..Default::default()
        };

        let err = server.ingest_metrics(with_auth(req)).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn ingest_logs_rejects_missing_timestamp() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = LogBatch {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            source: "app-out".to_string(),
            sent_at: None,
            entries: vec![],
        };

        let err = server.ingest_logs(with_auth(req)).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[tokio::test]
    async fn ingest_logs_accepts_valid_batch() {
        let server = RavenServer::new("rvn_test_token".to_string());
        let req = LogBatch {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            source: "app-out".to_string(),
            sent_at: Some(Timestamp {
                seconds: 1_700_000_000,
                nanos: 0,
            }),
            entries: vec![raven_proto::proto::LogEntry {
                source: "app-out".to_string(),
                path: "/tmp/app.log".to_string(),
                line: "hello".to_string(),
                stream: ProtoLogStream::Stdout as i32,
                timestamp: Some(Timestamp {
                    seconds: 1_700_000_000,
                    nanos: 0,
                }),
            }],
        };

        let resp = server
            .ingest_logs(with_auth(req))
            .await
            .unwrap()
            .into_inner();

        assert!(resp.ok);
    }
}
