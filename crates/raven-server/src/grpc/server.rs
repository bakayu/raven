use chrono::{TimeZone, Utc};
use futures::StreamExt;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use raven_proto::proto::raven_ingestion_server::RavenIngestion;
use raven_proto::proto::{
    HeartbeatRequest, HeartbeatResponse, LogBatch, LogStream as ProtoLogStream, MetricBatch,
    RegisterRequest, RegisterResponse, StreamResponse,
};

use crate::grpc::extract_bearer_token;
use crate::state::AppState;
use crate::tokens::validate_agent_token;

#[derive(Debug, Clone)]
pub struct RavenServer {
    state: AppState,
}

impl RavenServer {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl RavenServer {
    /// Validates the Bearer token on every incoming RPC.
    async fn authorize<T>(&self, request: &Request<T>) -> Result<(), Status> {
        let token = extract_bearer_token(request)?;

        let token_id = validate_agent_token(&self.state.db.read, &token)
            .await
            .map_err(Status::from)?;

        if token_id.is_none() {
            return Err(Status::unauthenticated("invalid token"));
        }

        Ok(())
    }

    async fn handle_metric_stream(
        &self,
        mut stream: impl futures::Stream<Item = Result<MetricBatch, tonic::Status>> + Unpin,
    ) -> Result<StreamResponse, Status> {
        let mut batches = 0usize;

        while let Some(batch) = stream.next().await {
            let batch = batch?;

            require_fields(&[("agent_id", &batch.agent_id), ("hostname", &batch.hostname)])?;

            let sent_at = parse_timestamp(batch.sent_at.as_ref(), "sent_at")?;

            let cpu_total = batch
                .cpu
                .as_ref()
                .map(|c| c.total_usage_percent)
                .unwrap_or_default();

            debug!(
                agent_id = %batch.agent_id,
                hostname = %batch.hostname,
                sent_at = %sent_at,
                cpu_total,
                disk_io = batch.disk_io.len(),
                filesystems = batch.filesystems.len(),
                interfaces = batch.network_interfaces.len(),
                "metrics batch received"
            );

            batches += 1;
        }

        Ok(StreamResponse {
            ok: true,
            message: format!("accepted {batches} batches"),
        })
    }

    async fn handle_log_stream(
        &self,
        mut stream: impl futures::Stream<Item = Result<LogBatch, tonic::Status>> + Unpin,
    ) -> Result<StreamResponse, Status> {
        let mut batches = 0usize;
        let mut total_entries = 0usize;

        while let Some(batch) = stream.next().await {
            let batch = batch?;

            require_fields(&[
                ("agent_id", &batch.agent_id),
                ("hostname", &batch.hostname),
                ("source", &batch.source),
            ])?;

            let sent_at = parse_timestamp(batch.sent_at.as_ref(), "sent_at")?;
            let (stdout_count, stderr_count) = count_log_streams(&batch);

            info!(
                agent_id = %batch.agent_id,
                hostname = %batch.hostname,
                source = %batch.source,
                entries = batch.entries.len(),
                stdout = stdout_count,
                stderr = stderr_count,
                sent_at = %sent_at,
                "log batch received"
            );

            batches += 1;
            total_entries += batch.entries.len();
        }

        Ok(StreamResponse {
            ok: true,
            message: format!("accepted {batches} batches, {total_entries} entries"),
        })
    }
}

// Validation helpers
fn parse_timestamp(
    ts: Option<&prost_types::Timestamp>,
    field: &str,
) -> Result<chrono::DateTime<Utc>, Status> {
    let ts = ts.ok_or_else(|| Status::invalid_argument(format!("missing {field}")))?;

    Utc.timestamp_opt(ts.seconds, ts.nanos as u32)
        .single()
        .ok_or_else(|| Status::invalid_argument(format!("invalid {field}")))
}

fn require_fields(fields: &[(&str, &str)]) -> Result<(), Status> {
    for (name, value) in fields {
        if value.trim().is_empty() {
            return Err(Status::invalid_argument(format!("{name} is required")));
        }
    }
    Ok(())
}

#[tonic::async_trait]
impl RavenIngestion for RavenServer {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        self.authorize(&request).await?;
        let req = request.into_inner();

        require_fields(&[
            ("agent_id", &req.agent_id),
            ("hostname", &req.hostname),
            ("os", &req.os),
            ("agent_version", &req.agent_version),
        ])?;

        // TODO: db::agents::upsert_agent(&self.state.db.write, &req).await.map_err(Status::from)?;

        info!(
            agent_id = %req.agent_id,
            hostname = %req.hostname,
            os = %req.os,
            agent_version = %req.agent_version,
            log_files = req.log_files.len(),
            "agent registered"
        );

        Ok(Response::new(RegisterResponse {
            ok: true,
            message: "registered".into(),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        self.authorize(&request).await?;
        let req = request.into_inner();

        require_fields(&[("agent_id", &req.agent_id), ("hostname", &req.hostname)])?;

        let sent_at = parse_timestamp(req.sent_at.as_ref(), "sent_at")?;

        // TODO: db::agents::update_last_seen(&self.state.db.write, &req.agent_id).await.map_err(Status::from)?;

        info!(
            agent_id = %req.agent_id,
            hostname = %req.hostname,
            sent_at = %sent_at,
            "heartbeat received"
        );

        Ok(Response::new(HeartbeatResponse {
            ok: true,
            message: "healthy".into(),
        }))
    }

    async fn stream_metrics(
        &self,
        request: Request<tonic::Streaming<MetricBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request).await?;
        let response = self.handle_metric_stream(request.into_inner()).await?;
        Ok(Response::new(response))
    }

    async fn stream_logs(
        &self,
        request: Request<tonic::Streaming<LogBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request).await?;
        let response = self.handle_log_stream(request.into_inner()).await?;
        Ok(Response::new(response))
    }
}

fn count_log_streams(batch: &LogBatch) -> (usize, usize) {
    batch.entries.iter().fold(
        (0, 0),
        |(stdout, stderr), entry| match ProtoLogStream::try_from(entry.stream) {
            Ok(ProtoLogStream::Stdout) => (stdout + 1, stderr),
            Ok(ProtoLogStream::Stderr) => (stdout, stderr + 1),
            _ => (stdout, stderr),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost_types::Timestamp;
    use raven_proto::proto::raven_ingestion_server::RavenIngestion;
    use tonic::Code;

    const TEST_TOKEN: &str = "rvn_test_token";

    async fn test_server() -> RavenServer {
        RavenServer::new(AppState::for_test().await)
    }

    fn with_auth<T>(payload: T) -> Request<T> {
        let mut req = Request::new(payload);
        req.metadata_mut().insert(
            "authorization",
            format!("Bearer {TEST_TOKEN}")
                .parse()
                .expect("valid header"),
        );
        req
    }

    fn with_bad_token<T>(payload: T) -> Request<T> {
        let mut req = Request::new(payload);
        req.metadata_mut().insert(
            "authorization",
            "Bearer wrong_token".parse().expect("valid header"),
        );
        req
    }

    fn valid_ts() -> Timestamp {
        Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        }
    }

    #[tokio::test]
    async fn register_rejects_missing_auth() {
        let server = test_server().await;
        let req = Request::new(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });
        let err = server.register(req).await.unwrap_err();
        assert_eq!(err.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn register_rejects_wrong_token() {
        let server = test_server().await;
        let req = with_bad_token(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });
        let err = server.register(req).await.unwrap_err();
        assert_eq!(err.code(), Code::Unauthenticated);
    }

    #[tokio::test]
    async fn register_rejects_empty_agent_id() {
        let server = test_server().await;
        let req = with_auth(RegisterRequest {
            agent_id: "".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });
        let err = server.register(req).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("agent_id"));
    }

    #[tokio::test]
    async fn register_succeeds() {
        let server = test_server().await;
        let req = with_auth(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });
        let resp = server.register(req).await.unwrap();
        assert!(resp.into_inner().ok);
    }

    #[tokio::test]
    async fn heartbeat_rejects_missing_timestamp() {
        let server = test_server().await;
        let req = with_auth(HeartbeatRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: None,
        });
        let err = server.heartbeat(req).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("sent_at"));
    }

    #[tokio::test]
    async fn heartbeat_succeeds() {
        let server = test_server().await;
        let req = with_auth(HeartbeatRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(valid_ts()),
        });
        let resp = server.heartbeat(req).await.unwrap();
        let inner = resp.into_inner();
        assert!(inner.ok);
        assert_eq!(inner.message, "healthy");
    }

    #[tokio::test]
    async fn stream_metrics_accepts_batch() {
        use raven_proto::proto::{CpuMetrics, MemoryMetrics};

        let server = test_server().await;
        let batch = MetricBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(valid_ts()),
            cpu: Some(CpuMetrics {
                total_usage_percent: 42.5,
                per_core_usage_percent: vec![40.0, 45.0],
            }),
            memory: Some(MemoryMetrics {
                total_bytes: 8 * 1024 * 1024 * 1024,
                used_bytes: 4 * 1024 * 1024 * 1024,
                available_bytes: 4 * 1024 * 1024 * 1024,
                ..Default::default()
            }),
            ..Default::default()
        };

        let stream = tokio_stream::iter(vec![Ok(batch)]);
        let resp = server.handle_metric_stream(stream).await.unwrap();
        assert!(resp.ok);
        assert!(resp.message.contains('1'));
    }

    #[tokio::test]
    async fn stream_logs_rejects_empty_source() {
        let server = test_server().await;
        let stream = tokio_stream::iter(vec![Ok(LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "".into(),
            sent_at: Some(valid_ts()),
            entries: vec![],
        })]);
        let err = server.handle_log_stream(stream).await.unwrap_err();
        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("source"));
    }
}
