use std::{fs, sync::Arc, time::Duration};

use anyhow::{Context, anyhow, bail};
use chrono::Utc;
use prost_types::Timestamp;
use secrecy::ExposeSecret;
use tokio::sync::mpsc;
use tonic::{Request, metadata::MetadataValue, transport::Channel};
use tracing::{debug, info, warn};

use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;
use raven_proto::proto::{
    CpuMetrics, DeviceIoMetrics, FilesystemMetrics, HeartbeatRequest, LoadAverage,
    LogBatch as ProtoLogBatch, LogEntry as ProtoLogEntry, LogStream as ProtoLogStream,
    MemoryMetrics, MetricBatch, NetworkInterfaceMetrics, NetworkTotals, RegisterRequest,
};

use crate::{AgentConfig, AgentEvent, LogBatch, LogStream, StatsSnapshot};

#[derive(Debug, Clone)]
struct AgentIdentity {
    agent_id: String,
    hostname: String,
    os: String,
    agent_version: String,
    log_files: Vec<String>,
}

pub async fn transport_task(mut rx: mpsc::Receiver<AgentEvent>, cfg: Arc<AgentConfig>) {
    let max_backoff = Duration::from_secs(cfg.transport.retry_max_interval_seconds.max(1));
    let mut backoff = Duration::from_secs(1);

    let identity = build_identity(&cfg);
    let bearer_token = cfg.server.token.expose_secret().to_string();

    loop {
        match connect_client(&cfg).await {
            Ok(mut client) => {
                info!(server = %cfg.server.address, "transport connected");

                let registered = match register_agent(&mut client, &identity, &bearer_token).await {
                    Ok(()) => {
                        info!(
                            agent_id = %identity.agent_id,
                            hostname = %identity.hostname,
                            "register successful"
                        );
                        true
                    }
                    Err(err) => {
                        warn!(
                            error = %err,
                            retry_seconds = backoff.as_secs(),
                            "register failed; reconnecting after backoff"
                        );
                        false
                    }
                };

                if registered {
                    backoff = Duration::from_secs(1);

                    while let Some(event) = rx.recv().await {
                        match event {
                            AgentEvent::Heartbeat { agent_id, hostname } => {
                                let now = Utc::now();

                                let payload = HeartbeatRequest {
                                    agent_id,
                                    hostname,
                                    sent_at: Some(Timestamp {
                                        seconds: now.timestamp(),
                                        nanos: now.timestamp_subsec_nanos() as i32,
                                    }),
                                };

                                let request = match auth_request(payload, &bearer_token) {
                                    Ok(request) => request,
                                    Err(err) => {
                                        warn!(error = %err, "failed to build heartbeat request");
                                        break;
                                    }
                                };

                                if let Err(err) = client.heartbeat(request).await {
                                    warn!(error = %err, "heartbeat send failed; reconnecting");
                                    break;
                                }
                            }
                            AgentEvent::Metrics(snapshot) => {
                                let payload = metric_batch_from_snapshot(
                                    &identity.agent_id,
                                    &identity.hostname,
                                    snapshot.clone(),
                                );

                                let request = match auth_request(payload, &bearer_token) {
                                    Ok(request) => request,
                                    Err(err) => {
                                        warn!(error = %err, "failed to build metrics request");
                                        break;
                                    }
                                };

                                match client.ingest_metrics(request).await {
                                    Ok(response) => {
                                        let body = response.into_inner();
                                        if !body.ok {
                                            warn!(message = %body.message, "metrics rejected by server");
                                        } else {
                                            debug!(?snapshot, "metrics batch sent");
                                        }
                                    }
                                    Err(err) => {
                                        warn!(error = %err, "metrics send failed; reconnecting");
                                        break;
                                    }
                                }
                            }
                            AgentEvent::Inventory(snapshot) => {
                                debug!(?snapshot, "inventory event queued for transport");
                            }
                            AgentEvent::Logs(batch) => {
                                let payload = log_batch_from_snapshot(
                                    &identity.agent_id,
                                    &identity.hostname,
                                    batch,
                                );

                                let request = match auth_request(payload, &bearer_token) {
                                    Ok(request) => request,
                                    Err(err) => {
                                        warn!(error = %err, "failed to build logs request");
                                        break;
                                    }
                                };

                                match client.ingest_logs(request).await {
                                    Ok(response) => {
                                        let body = response.into_inner();
                                        if !body.ok {
                                            warn!(message = %body.message, "logs rejected by server");
                                        } else {
                                            debug!("log batch sent");
                                        }
                                    }
                                    Err(err) => {
                                        warn!(error = %err, "logs send failed; reconnecting");
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if rx.is_closed() {
                        info!("transport exiting: receiver closed");
                        return;
                    }
                }
            }
            Err(err) => {
                warn!(
                    error = %err,
                    server = %cfg.server.address,
                    retry_seconds = backoff.as_secs(),
                    "transport connect failed"
                );
            }
        }

        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(max_backoff);
    }
}

fn build_identity(cfg: &AgentConfig) -> AgentIdentity {
    let hostname = hostname::get()
        .ok()
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let agent_id = read_machine_id().unwrap_or_else(|| hostname.clone());

    AgentIdentity {
        agent_id,
        hostname,
        os: std::env::consts::OS.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        log_files: cfg.logs.iter().map(|source| source.path.clone()).collect(),
    }
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

    let channel = tonic::transport::Endpoint::from_shared(endpoint.clone())
        .with_context(|| format!("invalid server endpoint: {endpoint}"))?
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
