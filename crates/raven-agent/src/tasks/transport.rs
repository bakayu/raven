use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, anyhow, bail};
use chrono::Utc;
use prost::Message;
use prost_types::Timestamp;
use secrecy::ExposeSecret;
use tokio::{sync::mpsc, task::JoinHandle};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{
    Request,
    metadata::MetadataValue,
    transport::{Certificate, Channel, ClientTlsConfig, Endpoint},
};
use tracing::{debug, info, warn};

use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;
use raven_proto::proto::{
    CpuMetrics, DeviceIoMetrics, FilesystemMetrics, HeartbeatRequest, LoadAverage,
    LogBatch as ProtoLogBatch, LogEntry as ProtoLogEntry, LogStream as ProtoLogStream,
    MemoryMetrics, MetricBatch, NetworkInterfaceMetrics, NetworkTotals, RegisterRequest,
    StreamResponse,
};

use crate::{AgentConfig, AgentEvent, LogBatch, LogStream, StatsSnapshot};

/// Path where the `agent_id` is stored on the system.
pub const AGENT_ID_PATH: &str = "/var/lib/raven/agent-id";

const DEFAULT_WAL_PATH: &str = "/var/lib/raven/transport.wal";
const WAL_PATH_ENV: &str = "RAVEN_AGENT_WAL_PATH";
const BYTES_PER_MB: u64 = 1024 * 1024;
const WAL_RECORD_TYPE_METRICS: u8 = 1;
const WAL_RECORD_TYPE_LOGS: u8 = 2;

#[derive(Debug, Clone)]
struct AgentIdentity {
    agent_id: String,
    hostname: String,
    os: String,
    agent_version: String,
    log_files: Vec<String>,
}

#[derive(Debug)]
struct StreamConnection {
    heartbeat_client: RavenIngestionClient<Channel>,
    metrics_sender: mpsc::Sender<MetricBatch>,
    logs_sender: mpsc::Sender<ProtoLogBatch>,
    metrics_handle: JoinHandle<Result<StreamResponse, tonic::Status>>,
    logs_handle: JoinHandle<Result<StreamResponse, tonic::Status>>,
}

impl StreamConnection {
    fn abort_streams(&mut self) {
        self.metrics_handle.abort();
        self.logs_handle.abort();
    }
}

#[derive(Debug, Clone)]
enum WalRecord {
    Metrics(MetricBatch),
    Logs(ProtoLogBatch),
}

#[derive(Debug, Clone)]
struct WalStore {
    path: PathBuf,
    max_bytes: u64,
}

impl WalStore {
    fn new(path: PathBuf, max_bytes: u64) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create WAL directory: {}", parent.display()))?;
        }

        if !path.exists() {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to initialize WAL file: {}", path.display()))?;
        }

        Ok(Self {
            path,
            max_bytes: max_bytes.max(1),
        })
    }

    fn append(&self, record: &WalRecord) -> anyhow::Result<()> {
        let encoded = encode_wal_record(record)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open WAL file: {}", self.path.display()))?;

        file.write_all(&encoded)
            .with_context(|| format!("failed to append WAL record: {}", self.path.display()))?;
        file.flush()
            .with_context(|| format!("failed to flush WAL file: {}", self.path.display()))?;

        self.enforce_cap()
    }

    fn load_all(&self) -> anyhow::Result<Vec<WalRecord>> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to read WAL file: {}", self.path.display()));
            }
        };

        let mut records = Vec::new();
        let mut cursor = 0usize;

        while cursor + 5 <= bytes.len() {
            let kind = bytes[cursor];
            let len_start = cursor + 1;
            let len_end = len_start + 4;
            let payload_len = u32::from_le_bytes(
                bytes[len_start..len_end]
                    .try_into()
                    .expect("slice length checked"),
            ) as usize;

            let payload_start = len_end;
            let Some(payload_end) = payload_start.checked_add(payload_len) else {
                warn!(path = %self.path.display(), "WAL payload length overflow, truncating read");
                break;
            };

            if payload_end > bytes.len() {
                warn!(
                    path = %self.path.display(),
                    "WAL record is truncated, stopping decode"
                );
                break;
            }

            let payload = &bytes[payload_start..payload_end];

            match decode_wal_record(kind, payload) {
                Ok(Some(record)) => records.push(record),
                Ok(None) => {
                    warn!(kind, "unknown WAL record type, skipping");
                }
                Err(err) => {
                    warn!(error = %err, "failed to decode WAL record, stopping replay decode");
                    break;
                }
            }

            cursor = payload_end;
        }

        Ok(records)
    }

    fn clear(&self) -> anyhow::Result<()> {
        fs::write(&self.path, [])
            .with_context(|| format!("failed to clear WAL file: {}", self.path.display()))
    }

    fn rewrite_records(&self, records: &[WalRecord]) -> anyhow::Result<()> {
        let mut encoded = Vec::new();
        for record in records {
            encoded.extend_from_slice(&encode_wal_record(record)?);
        }

        fs::write(&self.path, encoded)
            .with_context(|| format!("failed to rewrite WAL file: {}", self.path.display()))
    }

    fn enforce_cap(&self) -> anyhow::Result<()> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(()),
            Err(err) => {
                return Err(err).with_context(|| {
                    format!("failed to read WAL for cap check: {}", self.path.display())
                });
            }
        };

        if (bytes.len() as u64) <= self.max_bytes {
            return Ok(());
        }

        let ranges = record_ranges(&bytes);
        if ranges.is_empty() {
            return self.clear();
        }

        let mut drop_count = 0usize;
        let mut remaining_size = bytes.len();

        while drop_count < ranges.len() && (remaining_size as u64) > self.max_bytes {
            let (start, end) = ranges[drop_count];
            remaining_size = remaining_size.saturating_sub(end.saturating_sub(start));
            drop_count += 1;
        }

        let remaining_bytes = if drop_count >= ranges.len() {
            Vec::new()
        } else {
            bytes[ranges[drop_count].0..].to_vec()
        };

        fs::write(&self.path, remaining_bytes)
            .with_context(|| format!("failed to enforce WAL cap: {}", self.path.display()))
    }
}

pub async fn transport_task(mut rx: mpsc::Receiver<AgentEvent>, cfg: Arc<AgentConfig>) {
    let identity = build_identity(&cfg);
    let bearer_token = cfg.server.token.expose_secret().to_string();

    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(cfg.transport.retry_max_interval_seconds.max(1));
    let mut next_reconnect_at = tokio::time::Instant::now();

    let batch_size = cfg.transport.batch_size.max(1);

    let mut flush_interval = tokio::time::interval(Duration::from_secs(
        cfg.transport.flush_interval_seconds.max(1),
    ));
    flush_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    let mut pending_metrics = Vec::new();
    let mut pending_logs = Vec::new();

    let mut wal = init_wal_store(
        &cfg,
        cfg.transport.wal_max_size_mb.saturating_mul(BYTES_PER_MB),
    );

    let mut connection: Option<StreamConnection> = None;

    loop {
        if connection.is_none() {
            let sleep = tokio::time::sleep_until(next_reconnect_at);
            tokio::pin!(sleep);

            tokio::select! {
                maybe_event = rx.recv() => {
                    let Some(event) = maybe_event else {
                        info!("transport exiting: receiver closed");
                        return;
                    };

                    if let Err(err) = buffer_event_to_wal(&mut wal, event, &identity) {
                        warn!(error = %err, "failed to buffer event in WAL while disconnected");
                    }
                }
                _ = &mut sleep => {
                    match connect_streaming_client(&cfg, &identity, &bearer_token).await {
                        Ok(mut conn) => {
                            info!(server = %cfg.server.address, "transport connected");
                            backoff = Duration::from_secs(1);

                            if let Err(err) = replay_wal(&wal, &conn).await {
                                warn!(error = %err, "WAL replay failed; reconnecting");
                                conn.abort_streams();
                                next_reconnect_at = tokio::time::Instant::now() + backoff;
                                backoff = (backoff * 2).min(max_backoff);
                                continue;
                            }

                            if let Err(err) =
                                flush_pending_batches(&conn, &mut pending_metrics, &mut pending_logs)
                                    .await
                            {
                                warn!(error = %err, "failed to flush pending batches after reconnect");
                                persist_pending_to_wal(&mut wal, &mut pending_metrics, &mut pending_logs);
                                conn.abort_streams();
                                next_reconnect_at = tokio::time::Instant::now() + backoff;
                                backoff = (backoff * 2).min(max_backoff);
                                continue;
                            }

                            connection = Some(conn);
                        }
                        Err(err) => {
                            warn!(
                                error = %err,
                                server = %cfg.server.address,
                                retry_seconds = backoff.as_secs(),
                                "transport connect failed"
                            );
                            next_reconnect_at = tokio::time::Instant::now() + backoff;
                            backoff = (backoff * 2).min(max_backoff);
                        }
                    }
                }
            }

            continue;
        }

        let mut should_disconnect = false;

        {
            let conn = connection.as_mut().expect("connection must exist");

            tokio::select! {
                _ = flush_interval.tick() => {
                    if let Err(err) = flush_pending_batches(conn, &mut pending_metrics, &mut pending_logs).await {
                        warn!(error = %err, "stream flush failed");
                        should_disconnect = true;
                    }
                }
                maybe_event = rx.recv() => {
                    match maybe_event {
                        None => {
                            persist_pending_to_wal(&mut wal, &mut pending_metrics, &mut pending_logs);
                            info!("transport exiting: receiver closed");
                            conn.abort_streams();
                            return;
                        }
                        Some(event) => {
                            if let Err(err) = handle_connected_event(
                                conn,
                                event,
                                &identity,
                                &bearer_token,
                                batch_size,
                                &mut pending_metrics,
                                &mut pending_logs,
                            )
                            .await
                            {
                                warn!(error = %err, "transport send failed; reconnecting");
                                should_disconnect = true;
                            }
                        }
                    }
                }
                metrics_result = &mut conn.metrics_handle => {
                    match metrics_result {
                        Ok(Ok(response)) => {
                            if response.ok {
                                warn!(message = %response.message, "metrics stream completed");
                            } else {
                                warn!(message = %response.message, "metrics stream closed with server rejection");
                            }
                        }
                        Ok(Err(err)) => {
                            warn!(error = %err, "metrics stream failed");
                        }
                        Err(err) => {
                            warn!(error = %err, "metrics stream task join failed");
                        }
                    }

                    should_disconnect = true;
                }
                logs_result = &mut conn.logs_handle => {
                    match logs_result {
                        Ok(Ok(response)) => {
                            if response.ok {
                                warn!(message = %response.message, "logs stream completed");
                            } else {
                                warn!(message = %response.message, "logs stream closed with server rejection");
                            }
                        }
                        Ok(Err(err)) => {
                            warn!(error = %err, "logs stream failed");
                        }
                        Err(err) => {
                            warn!(error = %err, "logs stream task join failed");
                        }
                    }

                    should_disconnect = true;
                }
            }
        }

        if should_disconnect {
            persist_pending_to_wal(&mut wal, &mut pending_metrics, &mut pending_logs);

            if let Some(mut conn) = connection.take() {
                conn.abort_streams();
            }

            next_reconnect_at = tokio::time::Instant::now() + backoff;
            backoff = (backoff * 2).min(max_backoff);
        }
    }
}

fn init_wal_store(cfg: &AgentConfig, max_bytes: u64) -> Option<WalStore> {
    if max_bytes == 0 {
        warn!("WAL disabled because wal_max_size_mb resolved to 0 bytes");
        return None;
    }

    let wal_path = resolve_wal_path(cfg);

    match WalStore::new(wal_path.clone(), max_bytes) {
        Ok(store) => {
            info!(
                wal_path = %wal_path.display(),
                wal_max_size_bytes = max_bytes,
                "WAL initialized"
            );
            Some(store)
        }
        Err(err) => {
            warn!(
                wal_path = %wal_path.display(),
                error = %err,
                "WAL unavailable; disconnected batches will be dropped"
            );
            None
        }
    }
}

fn resolve_wal_path(cfg: &AgentConfig) -> PathBuf {
    if let Some(path) = cfg.transport.wal_path.as_deref()
        && !path.trim().is_empty()
    {
        return PathBuf::from(path);
    }

    std::env::var(WAL_PATH_ENV)
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DEFAULT_WAL_PATH))
}

fn append_wal_record(wal: &mut Option<WalStore>, record: WalRecord) -> anyhow::Result<()> {
    if let Some(store) = wal {
        store.append(&record)
    } else {
        Ok(())
    }
}

fn buffer_event_to_wal(
    wal: &mut Option<WalStore>,
    event: AgentEvent,
    identity: &AgentIdentity,
) -> anyhow::Result<()> {
    match event {
        AgentEvent::Metrics(snapshot) => append_wal_record(
            wal,
            WalRecord::Metrics(metric_batch_from_snapshot(
                &identity.agent_id,
                &identity.hostname,
                snapshot,
            )),
        ),
        AgentEvent::Logs(batch) => append_wal_record(
            wal,
            WalRecord::Logs(log_batch_from_snapshot(
                &identity.agent_id,
                &identity.hostname,
                batch,
            )),
        ),
        AgentEvent::Heartbeat { .. } | AgentEvent::Inventory(_) => Ok(()),
    }
}

fn persist_pending_to_wal(
    wal: &mut Option<WalStore>,
    pending_metrics: &mut Vec<MetricBatch>,
    pending_logs: &mut Vec<ProtoLogBatch>,
) {
    for batch in pending_metrics.drain(..) {
        if let Err(err) = append_wal_record(wal, WalRecord::Metrics(batch)) {
            warn!(error = %err, "failed to persist pending metrics batch into WAL");
        }
    }

    for batch in pending_logs.drain(..) {
        if let Err(err) = append_wal_record(wal, WalRecord::Logs(batch)) {
            warn!(error = %err, "failed to persist pending logs batch into WAL");
        }
    }
}

async fn replay_wal(wal: &Option<WalStore>, connection: &StreamConnection) -> anyhow::Result<()> {
    let Some(store) = wal else {
        return Ok(());
    };

    let records = store.load_all()?;
    if records.is_empty() {
        return Ok(());
    }

    info!(
        records = records.len(),
        "replaying WAL records before live stream"
    );

    for (index, record) in records.iter().cloned().enumerate() {
        if let Err(err) = send_record(connection, record).await {
            store.rewrite_records(&records[index..])?;
            return Err(err);
        }
    }

    store.clear()?;
    info!("WAL replay complete");

    Ok(())
}

async fn handle_connected_event(
    connection: &mut StreamConnection,
    event: AgentEvent,
    identity: &AgentIdentity,
    bearer_token: &str,
    batch_size: usize,
    pending_metrics: &mut Vec<MetricBatch>,
    pending_logs: &mut Vec<ProtoLogBatch>,
) -> anyhow::Result<()> {
    match event {
        AgentEvent::Heartbeat { hostname, .. } => {
            let now = Utc::now();
            let payload = HeartbeatRequest {
                agent_id: identity.agent_id.clone(),
                hostname,
                sent_at: Some(Timestamp {
                    seconds: now.timestamp(),
                    nanos: now.timestamp_subsec_nanos() as i32,
                }),
            };

            let request = auth_request(payload, bearer_token)?;
            connection
                .heartbeat_client
                .heartbeat(request)
                .await
                .context("failed to send heartbeat")?;
        }
        AgentEvent::Metrics(snapshot) => {
            pending_metrics.push(metric_batch_from_snapshot(
                &identity.agent_id,
                &identity.hostname,
                snapshot,
            ));

            if pending_metrics.len() >= batch_size {
                flush_metrics(connection, pending_metrics).await?;
            }
        }
        AgentEvent::Logs(batch) => {
            pending_logs.push(log_batch_from_snapshot(
                &identity.agent_id,
                &identity.hostname,
                batch,
            ));

            if pending_logs.len() >= batch_size {
                flush_logs(connection, pending_logs).await?;
            }
        }
        AgentEvent::Inventory(snapshot) => {
            debug!(?snapshot, "inventory event queued for transport");
        }
    }

    Ok(())
}

async fn flush_pending_batches(
    connection: &StreamConnection,
    pending_metrics: &mut Vec<MetricBatch>,
    pending_logs: &mut Vec<ProtoLogBatch>,
) -> anyhow::Result<()> {
    flush_metrics(connection, pending_metrics).await?;
    flush_logs(connection, pending_logs).await?;
    Ok(())
}

async fn flush_metrics(
    connection: &StreamConnection,
    pending_metrics: &mut Vec<MetricBatch>,
) -> anyhow::Result<()> {
    if pending_metrics.is_empty() {
        return Ok(());
    }

    let mut pending = std::mem::take(pending_metrics).into_iter();

    while let Some(batch) = pending.next() {
        if let Err(send_error) = connection.metrics_sender.send(batch).await {
            pending_metrics.push(send_error.0);
            pending_metrics.extend(pending);
            bail!("metrics stream sender closed");
        }
    }

    Ok(())
}

async fn flush_logs(
    connection: &StreamConnection,
    pending_logs: &mut Vec<ProtoLogBatch>,
) -> anyhow::Result<()> {
    if pending_logs.is_empty() {
        return Ok(());
    }

    let mut pending = std::mem::take(pending_logs).into_iter();

    while let Some(batch) = pending.next() {
        if let Err(send_error) = connection.logs_sender.send(batch).await {
            pending_logs.push(send_error.0);
            pending_logs.extend(pending);
            bail!("logs stream sender closed");
        }
    }

    Ok(())
}

async fn send_record(connection: &StreamConnection, record: WalRecord) -> anyhow::Result<()> {
    match record {
        WalRecord::Metrics(batch) => connection
            .metrics_sender
            .send(batch)
            .await
            .map_err(|_| anyhow!("metrics stream sender closed during WAL replay")),
        WalRecord::Logs(batch) => connection
            .logs_sender
            .send(batch)
            .await
            .map_err(|_| anyhow!("logs stream sender closed during WAL replay")),
    }
}

async fn connect_streaming_client(
    cfg: &AgentConfig,
    identity: &AgentIdentity,
    bearer_token: &str,
) -> anyhow::Result<StreamConnection> {
    let mut client = connect_client(cfg).await?;
    register_agent(&mut client, identity, bearer_token).await?;

    info!(
        agent_id = %identity.agent_id,
        hostname = %identity.hostname,
        "register successful"
    );

    let stream_capacity = cfg
        .transport
        .channel_capacity
        .max(cfg.transport.batch_size.max(1));

    let (metrics_sender, metrics_receiver) = mpsc::channel(stream_capacity);
    let (logs_sender, logs_receiver) = mpsc::channel(stream_capacity);

    let mut metrics_client = client.clone();
    let metrics_request = auth_request(ReceiverStream::new(metrics_receiver), bearer_token)?;
    let metrics_handle = tokio::spawn(async move {
        metrics_client
            .stream_metrics(metrics_request)
            .await
            .map(|response| response.into_inner())
    });

    let mut logs_client = client.clone();
    let logs_request = auth_request(ReceiverStream::new(logs_receiver), bearer_token)?;
    let logs_handle = tokio::spawn(async move {
        logs_client
            .stream_logs(logs_request)
            .await
            .map(|response| response.into_inner())
    });

    Ok(StreamConnection {
        heartbeat_client: client,
        metrics_sender,
        logs_sender,
        metrics_handle,
        logs_handle,
    })
}

fn build_identity(cfg: &AgentConfig) -> AgentIdentity {
    let hostname = hostname::get()
        .ok()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let agent_id = resolve_agent_id(&hostname);

    AgentIdentity {
        agent_id,
        hostname,
        os: std::env::consts::OS.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        log_files: cfg.logs.iter().map(|source| source.path.clone()).collect(),
    }
}

fn resolve_agent_id(hostname: &str) -> String {
    let id_path = Path::new(AGENT_ID_PATH);

    let persisted_uuid =
        read_persisted_uuid(id_path).or_else(|| generate_and_persist_uuid(id_path));

    persisted_uuid
        .or(read_machine_id())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| hostname.to_string())
}

fn read_persisted_uuid(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let trimmed = raw.trim();

    if uuid::Uuid::parse_str(trimmed).is_ok() {
        Some(trimmed.to_string())
    } else {
        None
    }
}

fn generate_and_persist_uuid(path: &Path) -> Option<String> {
    let generated = uuid::Uuid::new_v4().to_string();

    match persist_agent_id(path, &generated) {
        Ok(()) => Some(generated),
        Err(err) => {
            warn!(
                path = %path.display(),
                error = %err,
                "failed to persisted generated agent id; falling back"
            );
            None
        }
    }
}

fn persist_agent_id(path: &Path, agent_id: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, format!("{agent_id}\n"))?;
    Ok(())
}

fn read_machine_id() -> Option<String> {
    for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
        if let Ok(contents) = fs::read_to_string(path) {
            let id = contents.trim().to_string();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

fn build_register_request(identity: &AgentIdentity) -> RegisterRequest {
    RegisterRequest {
        agent_id: identity.agent_id.clone(),
        hostname: identity.hostname.clone(),
        os: identity.os.clone(),
        agent_version: identity.agent_version.clone(),
        log_files: identity.log_files.clone(),
    }
}

fn auth_request<T>(payload: T, bearer_token: &str) -> anyhow::Result<Request<T>> {
    let mut request = Request::new(payload);
    let header = MetadataValue::try_from(format!("Bearer {bearer_token}"))
        .context("invalid authorization metadata value")?;
    request.metadata_mut().insert("authorization", header);
    Ok(request)
}

async fn connect_client(cfg: &AgentConfig) -> anyhow::Result<RavenIngestionClient<Channel>> {
    let scheme = if cfg.server.tls { "https" } else { "http" };
    let endpoint = format!("{scheme}://{}", cfg.server.address);

    let mut endpoint = Endpoint::from_shared(endpoint.clone())
        .with_context(|| format!("invalid server endpoint: {endpoint}"))?;

    if cfg.server.tls {
        let mut tls_config = ClientTlsConfig::new();

        if let Some(domain_name) = cfg.server.tls_domain_name.as_deref() {
            tls_config = tls_config.domain_name(domain_name.to_string());
        }

        if let Some(ca_cert_path) = cfg.server.tls_ca_cert_path.as_deref() {
            let ca_cert = fs::read(ca_cert_path)
                .with_context(|| format!("failed to read TLS CA certificate: {ca_cert_path}"))?;
            tls_config = tls_config.ca_certificate(Certificate::from_pem(ca_cert));
        }

        endpoint = endpoint
            .tls_config(tls_config)
            .context("failed to configure TLS for transport endpoint")?;
    }

    let channel = endpoint
        .connect()
        .await
        .with_context(|| format!("failed to connect to {}", cfg.server.address))?;

    Ok(RavenIngestionClient::new(channel))
}

async fn register_agent(
    client: &mut RavenIngestionClient<Channel>,
    identity: &AgentIdentity,
    bearer_token: &str,
) -> anyhow::Result<()> {
    let request = auth_request(build_register_request(identity), bearer_token)?;

    let response = tokio::time::timeout(Duration::from_secs(5), client.register(request))
        .await
        .map_err(|_| anyhow!("register timed out"))??;

    let body = response.into_inner();

    if !body.ok {
        bail!("register rejected: {}", body.message);
    }

    Ok(())
}

/// Map MetricBatch from StatsSnapshot
fn metric_batch_from_snapshot(
    agent_id: &str,
    hostname: &str,
    snaphost: StatsSnapshot,
) -> MetricBatch {
    let now = Utc::now();

    MetricBatch {
        agent_id: agent_id.into(),
        hostname: hostname.into(),
        sent_at: Some(Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        }),

        cpu: Some(CpuMetrics {
            total_usage_percent: snaphost.cpu.total,
            per_core_usage_percent: snaphost.cpu.per_core,
        }),

        memory: Some(MemoryMetrics {
            total_bytes: snaphost.memory.physical_memory.total,
            used_bytes: snaphost.memory.physical_memory.used,
            available_bytes: snaphost.memory.physical_memory.available,
            swap_total_bytes: snaphost.memory.swap.total,
            swap_used_bytes: snaphost.memory.swap.used,
            swap_cached_bytes: snaphost.memory.swap.cached,
            buffers_bytes: snaphost.memory.buffer,
            cached_bytes: snaphost.memory.cache,
            zswap_bytes: snaphost.memory.zswap,
        }),

        disk_io: snaphost
            .disk
            .devices_io
            .into_iter()
            .map(|device_io_stats| DeviceIoMetrics {
                device: device_io_stats.device,
                read_bytes_per_sec: device_io_stats.read_bytes_per_sec,
                write_bytes_per_sec: device_io_stats.write_bytes_per_sec,
                read_iops: device_io_stats.read_iops,
                write_iops: device_io_stats.write_iops,
            })
            .collect(),

        filesystems: snaphost
            .disk
            .filesystems
            .into_iter()
            .map(|filesystem_telemetry| FilesystemMetrics {
                source: filesystem_telemetry.source,
                fs_type: String::new(),
                parent_device: None,
                total_bytes: filesystem_telemetry.total_bytes,
                used_bytes: filesystem_telemetry.used_bytes,
                free_bytes: filesystem_telemetry.free_bytes,
                avail_bytes: filesystem_telemetry.avail_bytes,
                used_percent: filesystem_telemetry.used_percent,
            })
            .collect(),

        network_total: Some(NetworkTotals {
            rx_bytes_per_sec: snaphost.network.total.rx_bytes_per_sec,
            tx_bytes_per_sec: snaphost.network.total.tx_bytes_per_sec,
        }),

        network_interfaces: snaphost
            .network
            .interfaces
            .into_iter()
            .map(|interface_network_rate| NetworkInterfaceMetrics {
                name: interface_network_rate.name,
                rx_bytes_per_sec: interface_network_rate.rate.rx_bytes_per_sec,
                tx_bytes_per_sec: interface_network_rate.rate.tx_bytes_per_sec,
            })
            .collect(),

        load_average: Some(LoadAverage {
            one_min: snaphost.loadavg.one,
            five_min: snaphost.loadavg.five,
            fifteen_min: snaphost.loadavg.fifteen,
        }),
    }
}

fn to_proto_timestamp(ts: chrono::DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: ts.timestamp(),
        nanos: ts.timestamp_subsec_nanos() as i32,
    }
}

fn log_batch_from_snapshot(agent_id: &str, hostname: &str, batch: LogBatch) -> ProtoLogBatch {
    ProtoLogBatch {
        agent_id: agent_id.to_string(),
        hostname: hostname.to_string(),
        source: batch.source,
        sent_at: Some(to_proto_timestamp(Utc::now())),
        entries: batch
            .entries
            .into_iter()
            .map(|entry| ProtoLogEntry {
                source: entry.source,
                path: entry.path.display().to_string(),
                line: entry.line,
                stream: match entry.stream {
                    LogStream::Stdout => ProtoLogStream::Stdout as i32,
                    LogStream::Stderr => ProtoLogStream::Stderr as i32,
                },
                timestamp: Some(to_proto_timestamp(entry.timestamp)),
            })
            .collect(),
    }
}

fn encode_wal_record(record: &WalRecord) -> anyhow::Result<Vec<u8>> {
    let (kind, payload) = match record {
        WalRecord::Metrics(batch) => (WAL_RECORD_TYPE_METRICS, batch.encode_to_vec()),
        WalRecord::Logs(batch) => (WAL_RECORD_TYPE_LOGS, batch.encode_to_vec()),
    };

    let payload_len = u32::try_from(payload.len()).context("WAL payload too large")?;

    let mut encoded = Vec::with_capacity(1 + 4 + payload.len());
    encoded.push(kind);
    encoded.extend_from_slice(&payload_len.to_le_bytes());
    encoded.extend_from_slice(&payload);

    Ok(encoded)
}

fn decode_wal_record(kind: u8, payload: &[u8]) -> anyhow::Result<Option<WalRecord>> {
    match kind {
        WAL_RECORD_TYPE_METRICS => {
            let batch =
                MetricBatch::decode(payload).context("failed to decode metrics WAL record")?;
            Ok(Some(WalRecord::Metrics(batch)))
        }
        WAL_RECORD_TYPE_LOGS => {
            let batch =
                ProtoLogBatch::decode(payload).context("failed to decode logs WAL record")?;
            Ok(Some(WalRecord::Logs(batch)))
        }
        _ => Ok(None),
    }
}

fn record_ranges(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut cursor = 0usize;
    let mut ranges = Vec::new();

    while cursor + 5 <= bytes.len() {
        let len_start = cursor + 1;
        let len_end = len_start + 4;
        let payload_len = u32::from_le_bytes(
            bytes[len_start..len_end]
                .try_into()
                .expect("slice length checked"),
        ) as usize;

        let payload_start = len_end;
        let Some(payload_end) = payload_start.checked_add(payload_len) else {
            break;
        };

        if payload_end > bytes.len() {
            break;
        }

        ranges.push((cursor, payload_end));
        cursor = payload_end;
    }

    ranges
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn unique_temp_path(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();

        std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), nanos))
    }

    #[test]
    fn build_register_request_maps_fields() {
        let identity = AgentIdentity {
            agent_id: "machine-123".to_string(),
            hostname: "host-a".to_string(),
            os: "linux".to_string(),
            agent_version: "0.1.0".to_string(),
            log_files: vec!["/var/log/app.log".to_string()],
        };

        let req = build_register_request(&identity);

        assert_eq!(req.agent_id, "machine-123");
        assert_eq!(req.hostname, "host-a");
        assert_eq!(req.os, "linux");
        assert_eq!(req.agent_version, "0.1.0");
        assert_eq!(req.log_files, vec!["/var/log/app.log"]);
    }

    #[test]
    fn auth_request_sets_bearer_metadata() {
        let request = auth_request(RegisterRequest::default(), "rvn_test_token")
            .expect("request with auth should be built");

        let value = request
            .metadata()
            .get("authorization")
            .expect("authorization metadata");
        assert_eq!(
            value.to_str().expect("metadata to str"),
            "Bearer rvn_test_token"
        );
    }

    #[test]
    fn persisted_uuid_round_trip() {
        let path = unique_temp_path("raven_agent_id");
        let generated = generate_and_persist_uuid(&path).expect("uuid should be generated");
        let loaded = read_persisted_uuid(&path).expect("uuid should be readable");

        assert_eq!(generated, loaded);

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn resolve_wal_path_prefers_config_value() {
        let mut cfg = AgentConfig::default();
        cfg.transport.wal_path = Some("/tmp/raven-config.wal".to_string());

        assert_eq!(
            resolve_wal_path(&cfg),
            PathBuf::from("/tmp/raven-config.wal")
        );
    }

    fn test_log_batch(agent_id: &str) -> ProtoLogBatch {
        ProtoLogBatch {
            agent_id: agent_id.to_string(),
            hostname: "host-a".to_string(),
            source: "app-out".to_string(),
            sent_at: None,
            entries: vec![ProtoLogEntry {
                source: "app-out".to_string(),
                path: "/tmp/app.log".to_string(),
                line: "hello".to_string(),
                stream: ProtoLogStream::Stdout as i32,
                timestamp: None,
            }],
        }
    }

    #[test]
    fn wal_cap_drops_oldest_records() {
        let path = unique_temp_path("raven_transport_wal");
        let first = WalRecord::Logs(test_log_batch("a1"));
        let second = WalRecord::Logs(test_log_batch("a2"));
        let third = WalRecord::Logs(test_log_batch("a3"));

        let record_size = encode_wal_record(&first)
            .expect("record should encode")
            .len() as u64;
        let wal = WalStore::new(path.clone(), record_size * 2 + 1).expect("WAL should initialize");

        wal.append(&first).expect("first append should work");
        wal.append(&second).expect("second append should work");
        wal.append(&third).expect("third append should work");

        let records = wal.load_all().expect("WAL should decode");
        assert_eq!(records.len(), 2);

        match &records[0] {
            WalRecord::Logs(batch) => assert_eq!(batch.agent_id, "a2"),
            WalRecord::Metrics(_) => panic!("expected logs WAL record"),
        }

        match &records[1] {
            WalRecord::Logs(batch) => assert_eq!(batch.agent_id, "a3"),
            WalRecord::Metrics(_) => panic!("expected logs WAL record"),
        }

        let _ = std::fs::remove_file(path);
    }
}
