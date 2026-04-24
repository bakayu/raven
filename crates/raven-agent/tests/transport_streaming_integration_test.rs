use std::{
    fs,
    net::SocketAddr,
    path::PathBuf,
    sync::{
        Arc, Once,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use chrono::Utc;
use rcgen::{CertificateParams, DnType, KeyPair};
use secrecy::SecretString;
use tokio::{
    sync::{Mutex, Notify, mpsc, oneshot},
    time::{Instant, timeout},
};
use tonic::{
    Request, Response, Status,
    transport::{Identity, Server, ServerTlsConfig},
};

use raven_agent::{AgentConfig, AgentEvent, LogBatch, LogEntry, LogStream, transport_task};
use raven_proto::proto::raven_ingestion_server::{RavenIngestion, RavenIngestionServer};
use raven_proto::proto::{
    HeartbeatRequest, HeartbeatResponse, LogBatch as ProtoLogBatch, MetricBatch, RegisterRequest,
    RegisterResponse, StreamResponse,
};

const TEST_TOKEN: &str = "rvn_test_token";
static RUSTLS_PROVIDER_INIT: Once = Once::new();

#[derive(Debug, Default)]
struct MockState {
    logs: Mutex<Vec<ProtoLogBatch>>,
    register_calls: AtomicUsize,
    heartbeat_calls: AtomicUsize,
    notify: Notify,
}

#[derive(Debug, Clone)]
struct MockIngestion {
    expected_token: String,
    state: Arc<MockState>,
}

impl MockIngestion {
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

#[tonic::async_trait]
impl RavenIngestion for MockIngestion {
    async fn register(
        &self,
        request: Request<RegisterRequest>,
    ) -> Result<Response<RegisterResponse>, Status> {
        self.authorize(&request)?;
        self.state.register_calls.fetch_add(1, Ordering::SeqCst);
        self.state.notify.notify_waiters();

        Ok(Response::new(RegisterResponse {
            ok: true,
            message: "registered".to_string(),
        }))
    }

    async fn stream_metrics(
        &self,
        request: Request<tonic::Streaming<MetricBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request)?;

        let mut stream = request.into_inner();
        while stream.message().await?.is_some() {}

        Ok(Response::new(StreamResponse {
            ok: true,
            message: "metrics stream done".to_string(),
        }))
    }

    async fn stream_logs(
        &self,
        request: Request<tonic::Streaming<ProtoLogBatch>>,
    ) -> Result<Response<StreamResponse>, Status> {
        self.authorize(&request)?;

        let mut stream = request.into_inner();
        while let Some(batch) = stream.message().await? {
            self.state.logs.lock().await.push(batch);
            self.state.notify.notify_waiters();
        }

        Ok(Response::new(StreamResponse {
            ok: true,
            message: "logs stream done".to_string(),
        }))
    }

    async fn heartbeat(
        &self,
        request: Request<HeartbeatRequest>,
    ) -> Result<Response<HeartbeatResponse>, Status> {
        self.authorize(&request)?;

        self.state.heartbeat_calls.fetch_add(1, Ordering::SeqCst);
        self.state.notify.notify_waiters();

        Ok(Response::new(HeartbeatResponse {
            ok: true,
            message: "heartbeat ok".to_string(),
        }))
    }
}

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();

    let path = std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), nanos));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn unused_local_addr() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ephemeral listener");
    let addr = listener.local_addr().expect("get local addr");
    drop(listener);
    addr
}

fn write_tls_identity(root: &PathBuf) -> (PathBuf, PathBuf) {
    let mut params = CertificateParams::new(vec!["localhost".to_string()]).expect("cert params");
    params
        .distinguished_name
        .push(DnType::CommonName, "localhost");

    let key_pair = KeyPair::generate().expect("generate key pair");
    let cert = params
        .self_signed(&key_pair)
        .expect("create self signed certificate");

    let cert_path = root.join("server-cert.pem");
    let key_path = root.join("server-key.pem");

    fs::write(&cert_path, cert.pem()).expect("write cert pem");
    fs::write(&key_path, key_pair.serialize_pem()).expect("write key pem");

    (cert_path, key_path)
}

async fn start_mock_server(
    addr: SocketAddr,
    tls_identity: Option<(PathBuf, PathBuf)>,
) -> (
    Arc<MockState>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<(), tonic::transport::Error>>,
) {
    let state = Arc::new(MockState::default());
    let service = MockIngestion {
        expected_token: TEST_TOKEN.to_string(),
        state: state.clone(),
    };

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let handle = tokio::spawn(async move {
        let mut builder = Server::builder();

        if let Some((cert_path, key_path)) = tls_identity {
            let cert = fs::read(&cert_path).expect("read cert pem");
            let key = fs::read(&key_path).expect("read key pem");
            let identity = Identity::from_pem(cert, key);

            builder = builder
                .tls_config(ServerTlsConfig::new().identity(identity))
                .expect("configure server TLS");
        }

        builder
            .add_service(RavenIngestionServer::new(service))
            .serve_with_shutdown(addr, async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    (state, shutdown_tx, handle)
}

fn test_config(addr: SocketAddr, wal_path: &PathBuf) -> AgentConfig {
    let mut cfg = AgentConfig::default();

    cfg.server.address = addr.to_string();
    cfg.server.token = SecretString::new(TEST_TOKEN.to_string().into());
    cfg.server.tls = false;
    cfg.server.tls_domain_name = None;
    cfg.server.tls_ca_cert_path = None;

    cfg.transport.batch_size = 1;
    cfg.transport.flush_interval_seconds = 1;
    cfg.transport.retry_max_interval_seconds = 1;
    cfg.transport.wal_max_size_mb = 4;
    cfg.transport.wal_path = Some(wal_path.display().to_string());
    cfg.transport.heartbeat_interval_seconds = 1;
    cfg.transport.channel_capacity = 64;

    cfg
}

fn log_event(line: String) -> AgentEvent {
    AgentEvent::Logs(LogBatch {
        source: "app-out".to_string(),
        entries: vec![LogEntry {
            source: "app-out".to_string(),
            path: PathBuf::from("/tmp/app-out.log"),
            line,
            stream: LogStream::Stdout,
            timestamp: Utc::now(),
        }],
    })
}

fn ensure_rustls_provider() {
    RUSTLS_PROVIDER_INIT.call_once(|| {
        let provider = rustls::crypto::ring::default_provider();
        provider
            .install_default()
            .expect("install rustls crypto provider");
    });
}

async fn wait_for_registers(state: &Arc<MockState>, minimum: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        if state.register_calls.load(Ordering::SeqCst) >= minimum {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for register calls"
        );

        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = timeout(
            remaining.min(Duration::from_millis(200)),
            state.notify.notified(),
        )
        .await;
    }
}

async fn wait_for_heartbeats(state: &Arc<MockState>, minimum: usize) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        if state.heartbeat_calls.load(Ordering::SeqCst) >= minimum {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for heartbeat calls"
        );

        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = timeout(
            remaining.min(Duration::from_millis(200)),
            state.notify.notified(),
        )
        .await;
    }
}

async fn wait_for_log_batches(state: &Arc<MockState>, minimum: usize) -> Vec<ProtoLogBatch> {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        let batches = state.logs.lock().await.clone();
        if batches.len() >= minimum {
            return batches;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for log batches, wanted {minimum}"
        );

        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = timeout(
            remaining.min(Duration::from_millis(200)),
            state.notify.notified(),
        )
        .await;
    }
}

async fn wait_for_log_line_prefix(state: &Arc<MockState>, prefix: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        let batches = state.logs.lock().await;
        let found = batches
            .iter()
            .flat_map(|batch| batch.entries.iter())
            .any(|entry| entry.line.starts_with(prefix));
        drop(batches);

        if found {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for log line prefix: {prefix}"
        );

        let remaining = deadline.saturating_duration_since(Instant::now());
        let _ = timeout(
            remaining.min(Duration::from_millis(200)),
            state.notify.notified(),
        )
        .await;
    }
}

async fn wait_for_file_non_empty(path: &PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        if fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) > 0 {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for non-empty file: {}",
            path.display()
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_file_size_at_most(path: &PathBuf, max_size: u64) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        if fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) <= max_size {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for file cap at {} bytes: {}",
            max_size,
            path.display()
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn wait_for_file_empty(path: &PathBuf) {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        if fs::metadata(path).map(|meta| meta.len()).unwrap_or(0) == 0 {
            return;
        }

        assert!(
            Instant::now() < deadline,
            "timed out waiting for empty file: {}",
            path.display()
        );

        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn collect_log_lines(batches: &[ProtoLogBatch]) -> Vec<String> {
    let mut lines = Vec::new();

    for batch in batches {
        for entry in &batch.entries {
            lines.push(entry.line.clone());
        }
    }

    lines
}

#[tokio::test]
async fn transport_streams_logs_and_heartbeat_over_tls() {
    ensure_rustls_provider();

    let root = unique_temp_dir("raven_transport_tls_it");
    let wal_path = root.join("transport.wal");
    let (cert_path, key_path) = write_tls_identity(&root);

    let addr = unused_local_addr();
    let (state, shutdown_tx, server_handle) =
        start_mock_server(addr, Some((cert_path.clone(), key_path))).await;

    let mut cfg = test_config(addr, &wal_path);
    cfg.server.tls = true;
    cfg.server.tls_domain_name = Some("localhost".to_string());
    cfg.server.tls_ca_cert_path = Some(cert_path.display().to_string());

    let (tx, rx) = mpsc::channel(cfg.transport.channel_capacity);
    let transport_handle = tokio::spawn(transport_task(rx, Arc::new(cfg)));

    wait_for_registers(&state, 1).await;

    tx.send(log_event("tls-log-line".to_string()))
        .await
        .expect("send tls log event");
    tx.send(AgentEvent::Heartbeat {
        agent_id: "unused-agent-id".to_string(),
        hostname: "host-a".to_string(),
    })
    .await
    .expect("send heartbeat event");

    let logs = wait_for_log_batches(&state, 1).await;
    let lines = collect_log_lines(&logs);

    assert!(
        lines.iter().any(|line| line == "tls-log-line"),
        "expected streamed log line over TLS"
    );

    wait_for_heartbeats(&state, 1).await;

    drop(tx);
    timeout(Duration::from_secs(5), transport_handle)
        .await
        .expect("transport task shutdown timeout")
        .expect("transport task join failure");

    let _ = shutdown_tx.send(());
    let server_result = server_handle.await.expect("server task join failure");
    assert!(
        server_result.is_ok(),
        "server exited with error: {server_result:?}"
    );

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn transport_replays_wal_after_server_recovers() {
    let root = unique_temp_dir("raven_transport_replay_it");
    let wal_path = root.join("transport.wal");
    let addr = unused_local_addr();

    let cfg = test_config(addr, &wal_path);

    let (tx, rx) = mpsc::channel(cfg.transport.channel_capacity);
    let transport_handle = tokio::spawn(transport_task(rx, Arc::new(cfg)));

    tx.send(log_event("wal-log-1".to_string()))
        .await
        .expect("send first WAL log");
    tx.send(log_event("wal-log-2".to_string()))
        .await
        .expect("send second WAL log");

    wait_for_file_non_empty(&wal_path).await;

    let (state, shutdown_tx, server_handle) = start_mock_server(addr, None).await;

    wait_for_registers(&state, 1).await;
    let logs = wait_for_log_batches(&state, 2).await;
    let lines = collect_log_lines(&logs);

    assert!(lines.iter().any(|line| line == "wal-log-1"));
    assert!(lines.iter().any(|line| line == "wal-log-2"));

    wait_for_file_empty(&wal_path).await;

    drop(tx);
    timeout(Duration::from_secs(5), transport_handle)
        .await
        .expect("transport task shutdown timeout")
        .expect("transport task join failure");

    let _ = shutdown_tx.send(());
    let server_result = server_handle.await.expect("server task join failure");
    assert!(
        server_result.is_ok(),
        "server exited with error: {server_result:?}"
    );

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn transport_enforces_wal_cap_with_drop_oldest() {
    let root = unique_temp_dir("raven_transport_wal_cap_it");
    let wal_path = root.join("transport.wal");
    let addr = unused_local_addr();

    let mut cfg = test_config(addr, &wal_path);
    cfg.transport.wal_max_size_mb = 1;

    let (tx, rx) = mpsc::channel(cfg.transport.channel_capacity);
    let transport_handle = tokio::spawn(transport_task(rx, Arc::new(cfg)));

    let payload = "x".repeat(420_000);
    tx.send(log_event(format!("first-{payload}")))
        .await
        .expect("send first oversized log");
    tx.send(log_event(format!("second-{payload}")))
        .await
        .expect("send second oversized log");
    tx.send(log_event(format!("third-{payload}")))
        .await
        .expect("send third oversized log");

    wait_for_file_non_empty(&wal_path).await;
    wait_for_file_size_at_most(&wal_path, 1_048_576).await;

    let (state, shutdown_tx, server_handle) = start_mock_server(addr, None).await;

    wait_for_registers(&state, 1).await;
    wait_for_log_line_prefix(&state, "third-").await;
    let logs = wait_for_log_batches(&state, 1).await;
    let lines = collect_log_lines(&logs);

    assert!(
        !lines.iter().any(|line| line.starts_with("first-")),
        "oldest record should be dropped when WAL cap is exceeded"
    );
    assert!(
        lines.iter().any(|line| line.starts_with("third-")),
        "latest record should survive WAL cap enforcement"
    );

    drop(tx);
    timeout(Duration::from_secs(5), transport_handle)
        .await
        .expect("transport task shutdown timeout")
        .expect("transport task join failure");

    let _ = shutdown_tx.send(());
    let server_result = server_handle.await.expect("server task join failure");
    assert!(
        server_result.is_ok(),
        "server exited with error: {server_result:?}"
    );

    let _ = fs::remove_dir_all(root);
}
