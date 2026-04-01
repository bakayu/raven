use std::{sync::Arc, time::Duration};

use chrono::Utc;
use prost_types::Timestamp;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use raven_proto::proto::raven_ingestion_client::RavenIngestionClient;
use raven_proto::proto::{
    CpuMetrics, DeviceIoMetrics, FilesystemMetrics, HeartbeatRequest, LoadAverage, MemoryMetrics,
    MetricBatch, NetworkInterfaceMetrics, NetworkTotals, RegisterRequest,
};

use crate::{AgentConfig, AgentEvent, StatsSnapshot};

pub async fn transport_task(mut rx: mpsc::Receiver<AgentEvent>, cfg: Arc<AgentConfig>) {
    let max_backoff = Duration::from_secs(cfg.transport.retry_max_interval_seconds.max(1));
    let mut backoff = Duration::from_secs(1);

    loop {
        let endpoint = format!("http://{}", cfg.server.address);

        match RavenIngestionClient::connect(endpoint.clone()).await {
            Ok(mut client) => {
                info!(server = %cfg.server.address, "transport connected");
                backoff = Duration::from_secs(1);

                let hostname = hostname::get()
                    .ok()
                    .map(|v| v.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                // TODO: agent_id is same as hostname for now, should be changed to some
                // form of UUID later.
                let agent_id = hostname.clone();

                let register_request = RegisterRequest {
                    agent_id: agent_id.clone(),
                    hostname: hostname.clone(),
                    os: std::env::consts::OS.to_string(),
                    agent_version: env!("CARGO_PKG_VERSION").to_string(),
                    log_files: cfg.logs.iter().map(|l| l.path.clone()).collect(),
                };

                let register_ok = match client.register(register_request).await {
                    Ok(response) => {
                        let body = response.into_inner();
                        if body.ok {
                            info!(agent_id = %agent_id, hostname = %hostname, "register successful");
                            true
                        } else {
                            warn!(message = %body.message, "register failed");
                            false
                        }
                    }
                    Err(err) => {
                        warn!(error = %err, "register send failed");
                        false
                    }
                };

                if !register_ok {
                    warn!(
                        retry_seconds = backoff.as_secs(),
                        "register not accepted; backing off before reconnect",
                    );
                } else {
                    while let Some(event) = rx.recv().await {
                        match event {
                            AgentEvent::Heartbeat { agent_id, hostname } => {
                                let now = Utc::now();

                                let request = HeartbeatRequest {
                                    agent_id,
                                    hostname,
                                    sent_at: Some(Timestamp {
                                        seconds: now.timestamp(),
                                        nanos: now.timestamp_subsec_nanos() as i32,
                                    }),
                                };

                                if let Err(error_value) = client.heartbeat(request).await {
                                    warn!(error = %error_value, "heartbeat send failed; reconnecting");
                                    break;
                                }
                            }
                            AgentEvent::Metrics(snapshot) => {
                                let request = metric_batch_from_snapshot(
                                    &agent_id,
                                    &hostname,
                                    snapshot.clone(),
                                );

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
                        }
                    }

                    if rx.is_closed() {
                        info!("transport exiting: receiver closed");
                        return;
                    }
                }
            }
            Err(error_value) => {
                warn!(
                    error = %error_value,
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

/// Map `MetricBatch` from `StatsSnapshot`
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
