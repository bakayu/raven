use std::env;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use prost_types::Timestamp;
use raven_proto::proto::{CpuMetrics, LogBatch, LogEntry, LogStream, MetricBatch};
use raven_server::{ClickHouseClient, VictoriaMetricsClient};
use reqwest::Client;
use tokio::time::sleep;

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

fn unique_host(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();
    format!("{prefix}-{nanos}")
}

fn now_ts() -> Timestamp {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards");
    Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    }
}

#[tokio::test]
#[ignore]
async fn clickhouse_write_and_query() -> Result<()> {
    let base_url = env_or("RAVEN_DATABASE__CLICKHOUSE_URL", "http://localhost:8123");
    let client = ClickHouseClient::new(&base_url);
    client.ensure_schema().await?;

    let hostname = unique_host("itest-ch");
    let ts = now_ts();

    let batch = LogBatch {
        agent_id: "a1".into(),
        hostname: hostname.clone(),
        source: "app".into(),
        sent_at: Some(ts),
        entries: vec![LogEntry {
            source: "app".into(),
            path: "/var/log/app.log".into(),
            line: "hello from integration test".into(),
            stream: LogStream::Stdout as i32,
            timestamp: Some(ts),
        }],
    };

    client.write_logs(&batch).await?;

    // ClickHouse needs a moment to make the data queryable
    sleep(Duration::from_millis(500)).await;

    let query =
        format!("SELECT count() AS count FROM logs WHERE hostname = '{hostname}' FORMAT JSON");
    let query_url = format!("{base_url}/?query={}", urlencoding::encode(&query));

    let body = Client::new().get(&query_url).send().await?.text().await?;

    println!("clickhouse response: {body}");

    let json: serde_json::Value = serde_json::from_str(&body)?;
    let count = json["data"][0]["count"]
        .as_u64()
        .or_else(|| {
            json["data"][0]["count"]
                .as_str()
                .and_then(|s| s.parse().ok())
        })
        .unwrap_or(0);

    assert!(count >= 1, "expected at least 1 row, got {count}");
    Ok(())
}

#[tokio::test]
#[ignore]
async fn victoria_metrics_write_and_query() -> Result<()> {
    let base_url = env_or(
        "RAVEN_DATABASE__VICTORIA_METRICS_URL",
        "http://localhost:8428",
    );

    let client = VictoriaMetricsClient::new(&base_url);
    let hostname = unique_host("itest-vm");
    let ts = now_ts();

    // timestamp is 60s in the past so the sample is visible to the query
    let write_ts = prost_types::Timestamp {
        seconds: ts.seconds - 60,
        nanos: ts.nanos,
    };

    let batch = MetricBatch {
        agent_id: "a1".into(),
        hostname: hostname.clone(),
        sent_at: Some(write_ts),
        cpu: Some(CpuMetrics {
            total_usage_percent: 42.5,
            per_core_usage_percent: vec![40.0, 45.0],
        }),
        ..Default::default()
    };

    client.write(&batch).await?;

    // allow a short window for ingestion
    sleep(Duration::from_millis(1000)).await;

    let query_expr = format!("raven_cpu_usage_percent{{hostname=\"{hostname}\"}}");

    // Query a small time range around the written timestamp to avoid latency window issues.
    let start = write_ts.seconds - 10;
    let end = write_ts.seconds + 10;
    let query_url = format!(
        "{base_url}/api/v1/query_range?query={}&start={}&end={}&step=1s",
        urlencoding::encode(&query_expr),
        start,
        end
    );

    let http = Client::new();
    let mut found = false;

    for _ in 0..20 {
        let body = http.get(&query_url).send().await?.text().await?;
        println!("vm response: {body}");
        let json: serde_json::Value = serde_json::from_str(&body)?;
        let len = json["data"]["result"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| s["values"].as_array().map(|v| v.len()).unwrap_or(0))
                    .sum::<usize>()
            })
            .unwrap_or(0);
        if len > 0 {
            found = true;
            break;
        }
        sleep(Duration::from_millis(1000)).await;
    }

    assert!(found, "expected query to return a result for {hostname}");
    Ok(())
}
