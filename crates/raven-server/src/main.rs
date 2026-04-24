use std::fs;

use chrono::{TimeZone, Utc};
use tonic::{
    Request, Response, Status,
    transport::{Identity, Server, ServerTlsConfig},
};
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

fn validate_metric_batch(batch: &MetricBatch) -> Result<chrono::DateTime<Utc>, Status> {
    let sent_at = batch
        .sent_at
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("missing sent_at"))?;

    Utc.timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
        .single()
        .ok_or_else(|| Status::invalid_argument("invalid sent_at timestamp"))
}

fn validate_log_batch(batch: &LogBatch) -> Result<(chrono::DateTime<Utc>, usize, usize), Status> {
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
        .as_ref()
        .ok_or_else(|| Status::invalid_argument("missing sent_at"))?;

    let sent_at = Utc
        .timestamp_opt(sent_at.seconds, sent_at.nanos as u32)
        .single()
        .ok_or_else(|| Status::invalid_argument("invalid sent_at timestamp"))?;

    let stdout_count = batch
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                ProtoLogStream::try_from(entry.stream),
                Ok(ProtoLogStream::Stdout)
            )
        })
        .count();

    let stderr_count = batch
        .entries
        .iter()
        .filter(|entry| {
            matches!(
                ProtoLogStream::try_from(entry.stream),
                Ok(ProtoLogStream::Stderr)
            )
        })
        .count();

    Ok((sent_at, stdout_count, stderr_count))
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

    async fn stream_metrics(
        &self,
        request: Request<tonic::Streaming<MetricBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request)?;

        let mut stream = request.into_inner();
        let mut batches = 0usize;

        while let Some(batch) = stream.message().await? {
            let sent_at = validate_metric_batch(&batch)?;

            let cpu_total = batch
                .cpu
                .as_ref()
                .map(|c| c.total_usage_percent)
                .unwrap_or_default();

            debug!(
                agent_id = %batch.agent_id,
                hostname = %batch.hostname,
                sent_at = %sent_at,
                cpu_total = cpu_total,
                disk_io = batch.disk_io.len(),
                filesystems = batch.filesystems.len(),
                interfaces = batch.network_interfaces.len(),
                "metrics batch received"
            );

            batches += 1;
        }

        Ok(Response::new(StreamResponse {
            ok: true,
            message: format!("metrics stream accepted {batches} batches"),
        }))
    }

    async fn stream_logs(
        &self,
        request: Request<tonic::Streaming<LogBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request)?;

        let mut stream = request.into_inner();
        let mut batches = 0usize;
        let mut entries = 0usize;

        while let Some(batch) = stream.message().await? {
            let (sent_at, stdout_count, stderr_count) = validate_log_batch(&batch)?;

            info!(
                agent_id = %batch.agent_id,
                hostname = %batch.hostname,
                source = %batch.source,
                entries = batch.entries.len(),
                stdout_entries = stdout_count,
                stderr_entries = stderr_count,
                sent_at = %sent_at,
                "logs batch received"
            );

            batches += 1;
            entries += batch.entries.len();
        }

        Ok(Response::new(StreamResponse {
            ok: true,
            message: format!("logs stream accepted {batches} batches and {entries} entries"),
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

#[derive(Debug, Clone)]
struct RuntimeConfig {
    grpc_listen_addr: String,
    expected_token: String,
    tls_enabled: bool,
    tls_cert_path: Option<String>,
    tls_key_path: Option<String>,
}

fn env_bool(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        })
        .unwrap_or(default)
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn load_runtime_config() -> RuntimeConfig {
    RuntimeConfig {
        grpc_listen_addr: std::env::var("RAVEN_GRPC_LISTEN_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:9090".to_string()),
        expected_token: std::env::var("RAVEN_AGENT_TOKEN")
            .unwrap_or_else(|_| "rvn_dev_token".to_string()),
        tls_enabled: env_bool("RAVEN_GRPC_TLS_ENABLED", false),
        tls_cert_path: env_opt("RAVEN_GRPC_TLS_CERT_PATH"),
        tls_key_path: env_opt("RAVEN_GRPC_TLS_KEY_PATH"),
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_subscriber("raven-server", "info")?;

    let runtime = load_runtime_config();

    let addr = runtime.grpc_listen_addr.parse()?;
    let ingestion_server = RavenServer::new(runtime.expected_token.clone());
    let service = RavenIngestionServer::new(ingestion_server);

    let mut server_builder = Server::builder();

    if runtime.tls_enabled {
        let cert_path = runtime.tls_cert_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("RAVEN_GRPC_TLS_CERT_PATH is required when TLS is enabled")
        })?;
        let key_path = runtime.tls_key_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("RAVEN_GRPC_TLS_KEY_PATH is required when TLS is enabled")
        })?;

        let cert = fs::read(cert_path)?;
        let key = fs::read(key_path)?;
        let identity = Identity::from_pem(cert, key);

        server_builder = server_builder.tls_config(ServerTlsConfig::new().identity(identity))?;
    }

    info!(listen_addr = %addr, tls = runtime.tls_enabled, "server starting");
    server_builder.add_service(service).serve(addr).await?;

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

    #[test]
    fn metric_validation_rejects_missing_timestamp() {
        let req = MetricBatch {
            sent_at: None,
            ..Default::default()
        };

        let err = validate_metric_batch(&req).unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn log_validation_rejects_missing_timestamp() {
        let req = LogBatch {
            agent_id: "a1".to_string(),
            hostname: "host1".to_string(),
            source: "app-out".to_string(),
            sent_at: None,
            entries: vec![],
        };

        let err = validate_log_batch(&req).unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
    }

    #[test]
    fn log_validation_accepts_valid_batch() {
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

        let result = validate_log_batch(&req).expect("valid log batch should pass validation");
        assert_eq!(result.1, 1);
        assert_eq!(result.2, 0);
    }
}
