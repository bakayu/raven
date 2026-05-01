use std::time::Duration;

use reqwest::{Client, Url};
use serde_json::Value;
use tracing::{debug, warn};

use raven_proto::proto::MetricBatch;

use crate::error::{AppError, AppResult};

#[derive(Debug)]
pub struct VictoriaMetricsClient {
    client: Client,
    base_url: String,
}

impl VictoriaMetricsClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    #[tracing::instrument(
        name = "write metrics to victoriametrics",
        skip(self, batch),
        fields(hostname = %batch.hostname)
    )]
    pub async fn write(&self, batch: &MetricBatch) -> AppResult<()> {
        let body = build_prometheus_lines(batch);

        if body.is_empty() {
            debug!(hostname = %batch.hostname, "skipping empty metric batch");
            return Ok(());
        }

        let url = format!("{}/api/v1/import/prometheus", self.base_url);

        debug!(url = %url, body_len = body.len(), "posting metrics batch");

        let response = self
            .client
            .post(&url)
            .header("Content-Type", "text/plain")
            .body(body)
            .send()
            .await
            .map_err(|e| {
                warn!(
                    error = %e,
                    hostname = %batch.hostname,
                    url = %url,
                    "failed to send metrics batch to VictoriaMetrics"
                );
                AppError::VictoriaMetrics(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            warn!(
                hostname = %batch.hostname,
                url = %url,
                status = %status,
                response_body = %body,
                "VictoriaMetrics rejected metrics batch"
            );

            return Err(AppError::VictoriaMetrics(format!(
                "unexpected status {status}: {body}"
            )));
        }

        Ok(())
    }

    pub async fn query_range(&self, query: &str) -> AppResult<Value> {
        let mut url = Url::parse(&format!("{}/api/v1/query_range", self.base_url))
            .map_err(|e| AppError::VictoriaMetrics(e.to_string()))?;
        url.query_pairs_mut().append_pair("query", query);
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| AppError::VictoriaMetrics(e.to_string()))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| AppError::VictoriaMetrics(e.to_string()))?;
        if !status.is_success() {
            return Err(AppError::VictoriaMetrics(format!(
                "unexpected status {status}: {body}"
            )));
        }

        serde_json::from_str(&body).map_err(AppError::from)
    }
}

/// Build prometheus plaintext from MetricBatch
fn build_prometheus_lines(batch: &MetricBatch) -> String {
    let mut lines = Vec::new();

    let hostname = &batch.hostname;
    let ts_ms = batch
        .sent_at
        .as_ref()
        .map(|t| t.seconds * 1000 + (t.nanos as i64 / 1_000_000))
        .unwrap_or(0);

    // CPU
    if let Some(cpu) = &batch.cpu {
        lines.push(metric(
            "raven_cpu_usage_percent",
            &[("hostname", hostname)],
            cpu.total_usage_percent,
            ts_ms,
        ));

        for (i, usage) in cpu.per_core_usage_percent.iter().enumerate() {
            lines.push(metric(
                "raven_cpu_core_usage_percent",
                &[("hostname", hostname), ("core", &i.to_string())],
                *usage,
                ts_ms,
            ));
        }
    }

    // Memory
    if let Some(mem) = &batch.memory {
        let mem_metrics = [
            ("raven_memory_total_bytes", mem.total_bytes as f64),
            ("raven_memory_used_bytes", mem.used_bytes as f64),
            ("raven_memory_available_bytes", mem.available_bytes as f64),
            ("raven_memory_buffers_bytes", mem.buffers_bytes as f64),
            ("raven_memory_cached_bytes", mem.cached_bytes as f64),
            ("raven_swap_total_bytes", mem.swap_total_bytes as f64),
            ("raven_swap_used_bytes", mem.swap_used_bytes as f64),
            ("raven_swap_cached_bytes", mem.swap_cached_bytes as f64),
        ];

        for (name, value) in &mem_metrics {
            lines.push(metric(name, &[("hostname", hostname)], *value, ts_ms));
        }
    }

    // Disk IO
    for disk in &batch.disk_io {
        let labels = [
            ("hostname", hostname.as_str()),
            ("device", disk.device.as_str()),
        ];
        lines.push(metric(
            "raven_disk_read_bytes_per_sec",
            &labels,
            disk.read_bytes_per_sec,
            ts_ms,
        ));
        lines.push(metric(
            "raven_disk_write_bytes_per_sec",
            &labels,
            disk.write_bytes_per_sec,
            ts_ms,
        ));
        lines.push(metric(
            "raven_disk_read_iops",
            &labels,
            disk.read_iops,
            ts_ms,
        ));
        lines.push(metric(
            "raven_disk_write_iops",
            &labels,
            disk.write_iops,
            ts_ms,
        ));
    }

    // Filesystems
    for fs in &batch.filesystems {
        let labels = [
            ("hostname", hostname.as_str()),
            ("source", &fs.source),
            ("fstype", &fs.fs_type),
        ];
        lines.push(metric(
            "raven_fs_total_bytes",
            &labels,
            fs.total_bytes as f64,
            ts_ms,
        ));
        lines.push(metric(
            "raven_fs_used_bytes",
            &labels,
            fs.used_bytes as f64,
            ts_ms,
        ));
        lines.push(metric(
            "raven_fs_free_bytes",
            &labels,
            fs.free_bytes as f64,
            ts_ms,
        ));
        lines.push(metric(
            "raven_fs_avail_bytes",
            &labels,
            fs.avail_bytes as f64,
            ts_ms,
        ));
        lines.push(metric(
            "raven_fs_used_percent",
            &labels,
            fs.used_percent,
            ts_ms,
        ));
    }

    // Network
    if let Some(net) = &batch.network_total {
        lines.push(metric(
            "raven_net_rx_bytes_per_sec",
            &[("hostname", hostname)],
            net.rx_bytes_per_sec,
            ts_ms,
        ));
        lines.push(metric(
            "raven_net_tx_bytes_per_sec",
            &[("hostname", hostname)],
            net.tx_bytes_per_sec,
            ts_ms,
        ));
    }

    for iface in &batch.network_interfaces {
        let labels = [("hostname", hostname.as_str()), ("interface", &iface.name)];
        lines.push(metric(
            "raven_net_iface_rx_bytes_per_sec",
            &labels,
            iface.rx_bytes_per_sec,
            ts_ms,
        ));
        lines.push(metric(
            "raven_net_iface_tx_bytes_per_sec",
            &labels,
            iface.tx_bytes_per_sec,
            ts_ms,
        ));
    }

    // Load Average
    if let Some(load) = &batch.load_average {
        lines.push(metric(
            "raven_load_avg_1m",
            &[("hostname", hostname)],
            load.one_min,
            ts_ms,
        ));
        lines.push(metric(
            "raven_load_avg_5m",
            &[("hostname", hostname)],
            load.five_min,
            ts_ms,
        ));
        lines.push(metric(
            "raven_load_avg_15m",
            &[("hostname", hostname)],
            load.fifteen_min,
            ts_ms,
        ));
    }

    lines.join("\n")
}

/// Formats a single Prometheus plaintext line
/// metric_name{label1="value",...} value timestamp_ms
fn metric(name: &str, labels: &[(&str, &str)], value: f64, ts_ms: i64) -> String {
    let label_str = labels
        .iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(",");

    if ts_ms > 0 {
        format!("{name}{{{label_str}}} {value} {ts_ms}")
    } else {
        format!("{name}{{{label_str}}} {value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use prost_types::Timestamp;
    use raven_proto::proto::{CpuMetrics, MetricBatch};

    fn ts(seconds: i64) -> Timestamp {
        Timestamp { seconds, nanos: 0 }
    }

    #[test]
    fn build_prometheus_lines_includes_cpu_and_timestamp() {
        let batch = MetricBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(ts(1_700_000_000)),
            cpu: Some(CpuMetrics {
                total_usage_percent: 42.5,
                per_core_usage_percent: vec![40.0, 45.0],
            }),
            ..Default::default()
        };

        let lines = build_prometheus_lines(&batch);

        let ts_ms = 1_700_000_000i64 * 1000;
        assert!(lines.contains(&format!(
            "raven_cpu_usage_percent{{hostname=\"host1\"}} 42.5 {ts_ms}"
        )));
        assert!(lines.contains("raven_cpu_core_usage_percent"));
    }

    #[tokio::test]
    async fn write_posts_to_import_endpoint() {
        let server = MockServer::start_async().await;

        let mock = server.mock(|when, then| {
            when.method(POST).path("/api/v1/import/prometheus");
            // .header("content-type", "text/plain");
            then.status(200);
        });

        let client = VictoriaMetricsClient::new(&server.base_url());

        let batch = MetricBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            sent_at: Some(ts(1_700_000_000)),
            cpu: Some(CpuMetrics {
                total_usage_percent: 42.5,
                per_core_usage_percent: vec![40.0, 45.0],
            }),
            ..Default::default()
        };

        client.write(&batch).await.expect("write metrics");
        mock.assert();
    }
}
