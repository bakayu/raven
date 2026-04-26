use chrono::{TimeZone, Utc};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use tracing::{debug, warn};

use raven_proto::proto::{LogBatch, LogStream as ProtoLogStream};

use crate::error::{AppError, AppResult};

#[derive(Debug)]
pub struct ClickHouseClient {
    client: Client,
    base_url: String,
}

impl ClickHouseClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Run once on startup, creates the logs table if it doesn't exist.
    pub async fn ensure_schema(&self) -> AppResult<()> {
        let ddl = r#"
            CREATE TABLE IF NOT EXISTS logs (
                timestamp   DateTime64(3, 'UTC'),
                hostname    LowCardinality(String),
                app         LowCardinality(String),
                file        String,
                stream      LowCardinality(String),
                line        String,
                INDEX idx_line line TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
            )
            ENGINE = MergeTree()
            ORDER BY (hostname, app, timestamp)
            PARTITION BY toYYYYMM(timestamp)
            TTL timestamp + INTERVAL 30 DAY
            SETTINGS index_granularity = 8192
        "#;

        self.execute_ddl(ddl).await
    }

    #[tracing::instrument(
        name = "ClickHouse write_logs",
        skip(self, batch),
        fields(hostname = %batch.hostname)
    )]
    pub async fn write_logs(&self, batch: &LogBatch) -> AppResult<()> {
        if batch.entries.is_empty() {
            debug!(hostname = %batch.hostname, "skipping empty log batch");
            return Ok(());
        }

        let rows = build_json_rows(batch);

        if rows.is_empty() {
            return Ok(());
        }

        let row_count = rows.len();
        let body = rows
            .iter()
            .map(|r| r.to_string())
            .collect::<Vec<_>>()
            .join("\n");

        let mut url = Url::parse(&self.base_url).map_err(|e| {
            warn!(
                error = %e,
                base_url = %self.base_url,
                "invalid ClickHouse base url"
            );
            AppError::ClickHouse(e.to_string())
        })?;

        url.query_pairs_mut()
            .append_pair("query", "INSERT INTO logs FORMAT JSONEachRow");

        let url_str = url.as_str().to_string();

        let response = self
            .client
            .post(url)
            .header("Content-Type", "application/x-ndjson")
            .body(body)
            .send()
            .await
            .map_err(|e| {
                warn!(
                    error = %e,
                    hostname = %batch.hostname,
                    url = %url_str,
                    "Failed to send logs to ClickHouse"
                );

                AppError::ClickHouse(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            warn!(
                hostname = %batch.hostname,
                url = %url_str,
                status = %status,
                response_body = %body,
                "Failed to write logs to ClickHouse"
            );

            return Err(AppError::ClickHouse(format!(
                "unexpected status {status}: {body}"
            )));
        }

        debug!(
            hostname = %batch.hostname,
            rows = row_count,
            "wrote log batch to ClickHouse"
        );

        Ok(())
    }

    #[tracing::instrument(name = "ClickHouse execute_ddl", skip(self, query))]
    async fn execute_ddl(&self, query: &str) -> AppResult<()> {
        let response = self
            .client
            .post(&self.base_url)
            .header("Content-Type", "text/plain")
            .body(query.to_string())
            .send()
            .await
            .map_err(|e| {
                warn!(
                    error = %e,
                    url = %&self.base_url,
                    "failed to execute query with ClickHouse"
                );
                AppError::ClickHouse(e.to_string())
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();

            warn!(
                url = %&self.base_url,
                status = %status,
                response_body = %body,
                "ClickHouse rejected log batch"
            );

            return Err(AppError::ClickHouse(format!(
                "unexpected status {status}: {body}"
            )));
        }

        Ok(())
    }
}

/// JSON row builder
fn build_json_rows(batch: &LogBatch) -> Vec<Value> {
    batch
        .entries
        .iter()
        .filter_map(|entry| {
            let timestamp = entry
                .timestamp
                .as_ref()
                .or(batch.sent_at.as_ref())
                .and_then(|ts| Utc.timestamp_opt(ts.seconds, ts.nanos as u32).single())
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S%.3f").to_string())?;

            let stream = match ProtoLogStream::try_from(entry.stream) {
                Ok(ProtoLogStream::Stdout) => "stdout",
                Ok(ProtoLogStream::Stderr) => "stderr",
                _ => "unknown",
            };

            Some(json!({
                "timestamp": timestamp,
                "hostname":  batch.hostname.clone(),
                "app":       batch.source.clone(),
                "file":      entry.path.clone(),
                "stream":    stream,
                "line":      entry.line.clone(),
            }))
        })
        .collect()
}
