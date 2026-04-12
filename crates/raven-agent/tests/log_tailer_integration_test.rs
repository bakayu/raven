use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tokio::time::timeout;

use raven_agent::{AgentConfig, LogStream, LogTailer};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();

    let path = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[tokio::test]
async fn log_tailer_emits_plain_log_batch_on_append() {
    let root = unique_temp_dir("raven_log_tailer_it");
    let log_path = root.join("app-out.log");
    let cfg_path = root.join("agent.toml");

    // Seed file before watcher starts.
    fs::write(&log_path, "old-line-before-tailer\n").expect("write seed log");

    let toml = format!(
        r#"
[server]
address = "127.0.0.1:9090"
token = "rvn_dev_token"
tls = false

[metrics]
interval_seconds = 10

[transport]
batch_size = 100
flush_interval_seconds = 5
retry_max_interval_seconds = 60
wal_max_size_mb = 100
heartbeat_interval_seconds = 30
channel_capacity = 256

[logging]
level = "debug"
service_name = "raven-agent"

[[logs]]
name = "app-out"
path = "{}"
format = "plain"
stream = "stdout"
"#,
        log_path.display()
    );

    fs::write(&cfg_path, toml).expect("write config");

    let cfg = AgentConfig::load(&cfg_path).expect("load config");

    let (tx, mut rx) = mpsc::channel(128);
    let tailer = LogTailer::new(cfg.logs.clone(), tx).expect("build tailer");

    let handle = tokio::spawn(async move { tailer.run().await });

    // Give watcher a moment to register watches.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // Append new lines after watcher startup.
    {
        let mut f = OpenOptions::new()
            .append(true)
            .open(&log_path)
            .expect("open log for append");
        writeln!(f, "hello-one").expect("append line 1");
        writeln!(f, "hello-two").expect("append line 2");
        f.sync_all().expect("sync appended log");
    }

    let batch = timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("timed out waiting for batch")
        .expect("channel closed");

    assert_eq!(batch.source, "app-out");
    assert!(!batch.entries.is_empty());

    let lines: Vec<_> = batch.entries.iter().map(|e| e.line.as_str()).collect();
    assert!(lines.contains(&"hello-one"));
    assert!(lines.contains(&"hello-two"));

    for entry in &batch.entries {
        assert_eq!(entry.stream, LogStream::Stdout);
        assert_eq!(entry.path, log_path);
    }

    handle.abort();
    let _ = handle.await;

    let _ = fs::remove_dir_all(root);
}
