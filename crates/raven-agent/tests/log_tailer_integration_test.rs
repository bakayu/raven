use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc;
use tokio::time::{Instant, timeout};

use raven_agent::{AgentConfig, LogBatch, LogStream, LogTailer};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time went backwards")
        .as_nanos();

    let path = std::env::temp_dir().join(format!("{}_{}_{}", prefix, std::process::id(), nanos));
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

fn plain_log_config_toml(log_name: &str, log_path: &str) -> String {
    format!(
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
channel_capacity = 4096

[logging]
level = "debug"
service_name = "raven-agent"

[[logs]]
name = "{}"
path = "{}"
format = "plain"
stream = "stdout"
"#,
        log_name, log_path
    )
}

async fn recv_until_line(
    rx: &mut mpsc::Receiver<LogBatch>,
    expected_line: &str,
    deadline: Duration,
) -> LogBatch {
    let timeout_at = Instant::now() + deadline;

    loop {
        let now = Instant::now();
        assert!(
            now < timeout_at,
            "timed out waiting for line '{}'",
            expected_line
        );

        let wait_for = timeout_at - now;
        let batch = timeout(wait_for, rx.recv())
            .await
            .expect("timed out waiting for batch")
            .expect("channel closed");

        if batch
            .entries
            .iter()
            .any(|entry| entry.line == expected_line)
        {
            return batch;
        }
    }
}

#[tokio::test]
async fn log_tailer_emits_plain_log_batch_on_append() {
    let root = unique_temp_dir("raven_log_tailer_it");
    let log_path = root.join("app-out.log");
    let cfg_path = root.join("agent.toml");

    // Seed file before watcher starts.
    fs::write(&log_path, "old-line-before-tailer\n").expect("write seed log");

    let toml = plain_log_config_toml("app-out", &log_path.display().to_string());

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

#[tokio::test]
async fn log_tailer_continues_after_rotation() {
    let root = unique_temp_dir("raven_log_tailer_rotation_it");
    let log_path = root.join("app-out.log");
    let rotated_path = root.join("app-out.log.1");
    let cfg_path = root.join("agent.toml");

    fs::write(&log_path, "seed-before-start\n").expect("write seed log");
    fs::write(
        &cfg_path,
        plain_log_config_toml("app-out", &log_path.display().to_string()),
    )
    .expect("write config");

    let cfg = AgentConfig::load(&cfg_path).expect("load config");
    let (tx, mut rx) = mpsc::channel(128);
    let tailer = LogTailer::new(cfg.logs.clone(), tx).expect("build tailer");
    let handle = tokio::spawn(async move { tailer.run().await });

    tokio::time::sleep(Duration::from_millis(150)).await;

    {
        let mut f = OpenOptions::new()
            .append(true)
            .open(&log_path)
            .expect("open log for append");
        writeln!(f, "before-rotate").expect("append before rotation");
        f.sync_all().expect("sync before rotation");
    }

    let before_batch = recv_until_line(&mut rx, "before-rotate", Duration::from_secs(4)).await;
    assert_eq!(before_batch.source, "app-out");

    fs::rename(&log_path, &rotated_path).expect("rotate file");
    fs::write(&log_path, "").expect("create new active log file");

    // Allow created/rotated events to be processed before appending.
    tokio::time::sleep(Duration::from_millis(250)).await;

    {
        let mut f = OpenOptions::new()
            .append(true)
            .open(&log_path)
            .expect("open recreated log for append");
        writeln!(f, "after-rotate").expect("append after rotation");
        f.sync_all().expect("sync after rotation");
    }

    let after_batch = recv_until_line(&mut rx, "after-rotate", Duration::from_secs(5)).await;
    assert_eq!(after_batch.source, "app-out");
    assert!(
        after_batch
            .entries
            .iter()
            .any(|entry| entry.line == "after-rotate" && entry.path == log_path)
    );

    handle.abort();
    let _ = handle.await;
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn log_tailer_discovers_glob_file_created_after_start() {
    let root = unique_temp_dir("raven_log_tailer_glob_it");
    let containers_dir = root.join("containers");
    fs::create_dir_all(&containers_dir).expect("create containers root");

    let pattern = containers_dir.join("abc123*").join("*.log");
    let cfg_path = root.join("agent.toml");

    fs::write(
        &cfg_path,
        plain_log_config_toml("docker-ish", &pattern.display().to_string()),
    )
    .expect("write config");

    let cfg = AgentConfig::load(&cfg_path).expect("load config");
    let (tx, mut rx) = mpsc::channel(128);
    let tailer = LogTailer::new(cfg.logs.clone(), tx).expect("build tailer");
    let handle = tokio::spawn(async move { tailer.run().await });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let late_dir = containers_dir.join("abc123-live");
    fs::create_dir_all(&late_dir).expect("create matching glob directory");

    // Allow tailer to observe the new directory and attach a watch.
    tokio::time::sleep(Duration::from_millis(250)).await;

    let late_log = late_dir.join("runtime.log");
    fs::write(&late_log, "").expect("create late log file");

    // Allow tailer to observe file creation and open it.
    tokio::time::sleep(Duration::from_millis(250)).await;

    {
        let mut f = OpenOptions::new()
            .append(true)
            .open(&late_log)
            .expect("open late log for append");
        writeln!(f, "late-created-line").expect("append line");
        f.sync_all().expect("sync appended line");
    }

    let batch = recv_until_line(&mut rx, "late-created-line", Duration::from_secs(5)).await;
    assert_eq!(batch.source, "docker-ish");
    assert!(
        batch
            .entries
            .iter()
            .any(|entry| entry.line == "late-created-line" && entry.path == late_log)
    );

    handle.abort();
    let _ = handle.await;
    let _ = fs::remove_dir_all(root);
}
