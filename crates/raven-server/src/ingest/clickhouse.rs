use std::time::Duration;

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
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("reqwest client"),
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

    pub async fn query_logs(&self, sql: &str) -> AppResult<Vec<Value>> {
        let url = Url::parse(&self.base_url).map_err(|e| AppError::ClickHouse(e.to_string()))?;

        let response = self
            .client
            .post(url)
            .header("Content-Type", "text/plain")
            .header("Accept", "application/x-ndjson")
            .body(sql.to_string())
            .send()
            .await
            .map_err(|e| {
                tracing::error!("ClickHouse query failed: {}", e);
                AppError::ClickHouse(e.to_string())
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| AppError::ClickHouse(e.to_string()))?;
            
        if !status.is_success() {
            tracing::error!("ClickHouse query returned {}: {}", status, body);
            return Err(AppError::ClickHouse(format!(
                "unexpected status {status}: {body}"
            )));
        }

        let mut rows = Vec::new();
        for line in body.lines().filter(|line| !line.trim().is_empty()) {
            match serde_json::from_str(line) {
                Ok(row) => rows.push(row),
                Err(e) => {
                    tracing::error!("Failed to parse ClickHouse row '{}': {}", line, e);
                    return Err(AppError::ClickHouse(format!("parse error: {}", e)));
                }
            }
        }

        Ok(rows)
    }

    pub async fn ping(&self) -> AppResult<()> {
        self.client
            .post(&self.base_url)
            .body(String::new())
            .send()
            .await
            .map_err(|e| AppError::ClickHouse(e.to_string()))?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use httpmock::prelude::*;
    use prost_types::Timestamp;
    use raven_proto::proto::{LogBatch, LogEntry, LogStream};

    fn ts(seconds: i64) -> Timestamp {
        Timestamp { seconds, nanos: 0 }
    }

    #[test]
    fn build_json_rows_uses_entry_or_batch_timestamp_and_stream_label() {
        let batch = LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "app".into(),
            sent_at: Some(ts(1_700_000_000)),
            entries: vec![
                LogEntry {
                    source: "app".into(),
                    path: "/var/log/app.log".into(),
                    line: "hello".into(),
                    stream: LogStream::Stdout as i32,
                    timestamp: Some(ts(1_700_000_100)),
                },
                LogEntry {
                    source: "app".into(),
                    path: "/var/log/app.log".into(),
                    line: "world".into(),
                    stream: LogStream::Stderr as i32,
                    timestamp: None,
                },
            ],
        };

        let rows = build_json_rows(&batch);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["stream"], "stdout");
        assert_eq!(rows[1]["stream"], "stderr");

        let expected_sent_at = Utc
            .timestamp_opt(1_700_000_000, 0)
            .single()
            .unwrap()
            .format("%Y-%m-%d %H:%M:%S%.3f")
            .to_string();

        assert_eq!(rows[1]["timestamp"], expected_sent_at);
    }

    #[tokio::test]
    async fn write_logs_posts_ndjson_with_query() {
        let server = MockServer::start_async().await;

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .query_param("query", "INSERT INTO logs FORMAT JSONEachRow");
            // .header("content-type", "application/x-ndjson");
            then.status(200);
        });

        let client = ClickHouseClient::new(&server.base_url());

        let batch = LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "app".into(),
            sent_at: Some(ts(1_700_000_000)),
            entries: vec![LogEntry {
                source: "app".into(),
                path: "/var/log/app.log".into(),
                line: "hello".into(),
                stream: LogStream::Stdout as i32,
                timestamp: Some(ts(1_700_000_000)),
            }],
        };

        client.write_logs(&batch).await.expect("write logs");
        mock.assert();
    }

    #[tokio::test]
    async fn write_logs_returns_error_on_non_success() {
        let server = MockServer::start_async().await;

        let mock = server.mock(|when, then| {
            when.method(POST)
                .path("/")
                .query_param("query", "INSERT INTO logs FORMAT JSONEachRow");
            then.status(500).body("boom");
        });

        let client = ClickHouseClient::new(&server.base_url());

        let batch = LogBatch {
            agent_id: "a1".into(),
            hostname: "host1".into(),
            source: "app".into(),
            sent_at: Some(ts(1_700_000_000)),
            entries: vec![LogEntry {
                source: "app".into(),
                path: "/var/log/app.log".into(),
                line: "hello".into(),
                stream: LogStream::Stdout as i32,
                timestamp: Some(ts(1_700_000_000)),
            }],
        };

        let err = client.write_logs(&batch).await.expect_err("should fail");
        assert!(matches!(err, AppError::ClickHouse(_)));

        mock.assert();
    }
}
