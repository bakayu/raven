use chrono::{TimeZone, Utc};
use futures::StreamExt;
use tonic::{Request, Response, Status};
use tracing::{debug, info};

use raven_proto::proto::raven_ingestion_server::RavenIngestion;
use raven_proto::proto::{
    HeartbeatRequest, HeartbeatResponse, LogBatch, LogStream as ProtoLogStream, MetricBatch,
    RegisterRequest, RegisterResponse, StreamResponse,
};

use crate::AgentState;
use crate::db::agents::{update_last_seen, upsert_agent};
use crate::db::tokens::validate_agent_token;
use crate::grpc::extract_bearer_token;
use crate::state::AppState;

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
    async fn authorize<T>(&self, request: &Request<T>) -> Result<String, Status> {
        let token = extract_bearer_token(request)?;

        validate_agent_token(&self.state.db.write, &token)
            .await
            .map_err(Status::from)?
            .ok_or_else(|| Status::unauthenticated("Invalid token"))
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

            self.state
                .vm_client
                .write(&batch)
                .await
                .map_err(Status::from)?;

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

            let _ = self.state.log_tx.send(batch.clone());

            self.state
                .ch_client
                .write_logs(&batch)
                .await
                .map_err(Status::from)?;

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
        let token_id = self.authorize(&request).await?;

        let ip = request.remote_addr().map(|addr| addr.ip().to_string());
        let req = request.into_inner();

        require_fields(&[
            ("agent_id", &req.agent_id),
            ("hostname", &req.hostname),
            ("os", &req.os),
            ("agent_version", &req.agent_version),
        ])?;

        // persist to SQLite
        upsert_agent(&self.state.db.write, &req, token_id.as_str(), ip.as_deref())
            .await
            .map_err(Status::from)?;

        // update live state
        self.state.agents.insert(
            token_id.clone(),
            AgentState {
                agent_id: token_id,
                hostname: req.hostname.clone(),
                last_heartbeat: Utc::now(),
            },
        );

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
        let token_id = self.authorize(&request).await?;
        let req = request.into_inner();

        require_fields(&[("agent_id", &req.agent_id), ("hostname", &req.hostname)])?;

        let sent_at = parse_timestamp(req.sent_at.as_ref(), "sent_at")?;

        // persist to SQLite
        update_last_seen(&self.state.db.write, token_id.as_str(), &req.hostname).await?;

        // update live state
        if let Some(mut entry) = self.state.agents.get_mut(&token_id) {
            entry.last_heartbeat = Utc::now();
        }

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
    use crate::db::{tokens::create_agent_token, users};
    use prost_types::Timestamp;
    use raven_proto::proto::raven_ingestion_server::RavenIngestion;
    use tonic::Code;

    const TEST_TOKEN: &str = "rvn_test_token";

    async fn test_server() -> RavenServer {
        let server = RavenServer::new(AppState::for_test().await);
        seed_test_agent_token(&server).await;
        server
    }

    async fn seed_test_agent_token(server: &RavenServer) {
        let owner_id = users::create(
            &server.state.db.write,
            "grpc-agent-owner",
            "hash123",
            "admin",
        )
        .await
        .expect("create owner user");

        create_agent_token(&server.state.db.write, "grpc-agent", TEST_TOKEN, &owner_id)
            .await
            .expect("create agent token");
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
    async fn heartbeat_rejects_missing_auth() {
        let server = test_server().await;

        let req = Request::new(HeartbeatRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(valid_ts()),
        });

        let err = server.heartbeat(req).await.unwrap_err();
        assert_eq!(err.code(), Code::Unauthenticated);
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

        let register_req = with_auth(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });

        server
            .register(register_req)
            .await
            .expect("register should succeed");

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
    async fn stream_metrics_rejects_missing_timestamp() {
        let server = test_server().await;

        let batch = MetricBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: None,
            ..Default::default()
        };

        let stream = tokio_stream::iter(vec![Ok(batch)]);
        let err = server.handle_metric_stream(stream).await.unwrap_err();

        assert_eq!(err.code(), Code::InvalidArgument);
        assert!(err.message().contains("sent_at"));
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

    #[tokio::test]
    async fn stream_logs_counts_entries_correctly() {
        use raven_proto::proto::{LogEntry, LogStream};

        let server = test_server().await;

        let entries = vec![
            LogEntry {
                source: "app".into(),
                path: "/var/log/app.log".into(),
                line: "info: started".into(),
                stream: LogStream::Stdout as i32,
                timestamp: Some(valid_ts()),
            },
            LogEntry {
                source: "app".into(),
                path: "/var/log/app.log".into(),
                line: "error: something failed".into(),
                stream: LogStream::Stderr as i32,
                timestamp: Some(valid_ts()),
            },
        ];

        let stream = tokio_stream::iter(vec![Ok(LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "app".into(),
            sent_at: Some(valid_ts()),
            entries,
        })]);

        let resp = server.handle_log_stream(stream).await.unwrap();

        assert!(resp.ok);
        assert!(resp.message.contains('1'));
        assert!(resp.message.contains('2'));
    }

    #[tokio::test]
    async fn heartbeat_updates_last_seen_at_after_register() {
        let server = test_server().await;

        let register_req = with_auth(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });

        server
            .register(register_req)
            .await
            .expect("register should succeed");

        let token_id = validate_agent_token(&server.state.db.read, TEST_TOKEN)
            .await
            .expect("token validation should work")
            .expect("token should exist");

        let old_value = "2000-01-01T00:00:00Z";
        sqlx::query("UPDATE agents SET last_seen_at = ? WHERE token_id = ? AND hostname = ?")
            .bind(old_value)
            .bind(&token_id)
            .bind("host1")
            .execute(&server.state.db.write)
            .await
            .expect("force old last_seen_at");

        let heartbeat_req = with_auth(HeartbeatRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(valid_ts()),
        });

        server
            .heartbeat(heartbeat_req)
            .await
            .expect("heartbeat should succeed");

        let updated: String = sqlx::query_scalar(
            "SELECT last_seen_at FROM agents WHERE token_id = ? AND hostname = ?",
        )
        .bind(&token_id)
        .bind("host1")
        .fetch_one(&server.state.db.read)
        .await
        .expect("fetch updated last_seen_at");

        assert_ne!(updated, old_value);
    }

    #[tokio::test]
    async fn register_populates_live_agent_state() {
        let server = test_server().await;
        let req = with_auth(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });

        server.register(req).await.expect("register should succeed");

        let token_id = validate_agent_token(&server.state.db.read, TEST_TOKEN)
            .await
            .expect("token validation")
            .expect("token exists");

        let entry = server
            .state
            .agents
            .get(&token_id)
            .expect("agent should be in live map");
        assert_eq!(entry.hostname, "host1");
        assert_eq!(entry.agent_id, token_id);
    }

    #[tokio::test]
    async fn heartbeat_updates_live_agent_state_timestamp() {
        let server = test_server().await;
        let register_req = with_auth(RegisterRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            os: "linux".into(),
            agent_version: "0.1.0".into(),
            log_files: vec![],
        });
        server
            .register(register_req)
            .await
            .expect("register should succeed");

        let token_id = validate_agent_token(&server.state.db.read, TEST_TOKEN)
            .await
            .expect("token validation")
            .expect("token exists");

        let old = chrono::DateTime::parse_from_rfc3339("2000-01-01T00:00:00Z")
            .expect("valid timestamp")
            .with_timezone(&Utc);

        {
            let mut entry = server
                .state
                .agents
                .get_mut(&token_id)
                .expect("live map entry");
            entry.last_heartbeat = old;
        }

        let hb = with_auth(HeartbeatRequest {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(valid_ts()),
        });
        server
            .heartbeat(hb)
            .await
            .expect("heartbeat should succeed");

        let updated = server.state.agents.get(&token_id).expect("live map entry");
        assert!(updated.last_heartbeat > old);
    }

    #[tokio::test]
    async fn stream_logs_broadcasts_batches() {
        use raven_proto::proto::{LogBatch, LogEntry, LogStream};
        use tokio::time::{Duration, timeout};

        let server = test_server().await;
        let mut rx = server.state.log_tx.subscribe();

        let batch = LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "app".into(),
            sent_at: Some(valid_ts()),
            entries: vec![LogEntry {
                source: "app".into(),
                path: "/var/log/app.log".into(),
                line: "hello".into(),
                stream: LogStream::Stdout as i32,
                timestamp: Some(valid_ts()),
            }],
        };

        let stream = tokio_stream::iter(vec![Ok(batch.clone())]);
        let _ = server.handle_log_stream(stream).await.unwrap();

        let received = timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("recv timeout")
            .expect("recv ok");

        assert_eq!(received.hostname, batch.hostname);
    }
}
