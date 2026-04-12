use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::model::{LogEntry, LogStream};
use crate::log_reader::common::normalize_line;

/// Struct to deserialize froms from dockerjson files
#[derive(Debug, Deserialize)]
struct DockerJsonLine {
    log: String,
    stream: String,
    time: DateTime<Utc>,
}

/// Parse plain files, need to pass configured_stream to determine
/// if the log goes to Stdout or Stderr and obserted_at as timestamp.
/// Return one `LogEntry`
pub fn parse_plain(
    source: &str,
    path: &Path,
    raw: &str,
    configured_stream: LogStream,
    observed_at: DateTime<Utc>,
) -> Option<LogEntry> {
    Some(LogEntry {
        source: source.to_string(),
        path: path.to_path_buf(),
        line: normalize_line(raw),
        stream: configured_stream,
        timestamp: observed_at,
    })
}

/// Parse dockerjson files, stream and timestamp are directly determined
/// from the log file.
/// Return one `LogEntry`.
pub fn parse_docker_json(source: &str, path: &Path, raw: &str) -> Option<LogEntry> {
    let parsed: DockerJsonLine = serde_json::from_str(raw).ok()?;

    let stream = match parsed.stream.trim().to_ascii_lowercase().as_str() {
        "stdout" => LogStream::Stdout,
        "stderr" => LogStream::Stderr,
        _ => return None,
    };

    Some(LogEntry {
        source: source.to_string(),
        path: path.to_path_buf(),
        line: normalize_line(&parsed.log),
        stream,
        timestamp: parsed.time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_plain_maps_fields() {
        let ts: DateTime<Utc> = "2026-04-12T10:11:12Z".parse().expect("valid ts");
        let path = Path::new("/tmp/test.log");

        let entry =
            parse_plain("nginx", path, "hello world\n", LogStream::Stdout, ts).expect("entry");

        assert_eq!(entry.source, "nginx");
        assert_eq!(entry.path, path);
        assert_eq!(entry.line, "hello world");
        assert_eq!(entry.stream, LogStream::Stdout);
        assert_eq!(entry.timestamp, ts);
    }

    #[test]
    fn parse_docker_json_stdout() {
        let raw = r#"{"log":"started\n","stream":"stdout","time":"2026-04-12T10:11:12Z"}"#;
        let path = Path::new("/var/lib/docker/containers/a/a-json.log");

        let entry = parse_docker_json("api", path, raw).expect("entry");

        assert_eq!(entry.source, "api");
        assert_eq!(entry.path, path);
        assert_eq!(entry.line, "started");
        assert_eq!(entry.stream, LogStream::Stdout);
        assert_eq!(entry.timestamp.to_rfc3339(), "2026-04-12T10:11:12+00:00");
    }

    #[test]
    fn parse_docker_json_stderr() {
        let raw = r#"{"log":"boom","stream":"stderr","time":"2026-04-12T10:11:12Z"}"#;
        let entry = parse_docker_json("api", Path::new("/tmp/x.log"), raw).expect("entry");
        assert_eq!(entry.stream, LogStream::Stderr);
    }

    #[test]
    fn parse_docker_json_invalid_json_returns_none() {
        let raw = r#"{"log":"x","stream":"stdout","time":"bad-time"}"#;
        assert!(parse_docker_json("api", Path::new("/tmp/x.log"), raw).is_none());
    }

    #[test]
    fn parse_docker_json_unknown_stream_returns_none() {
        let raw = r#"{"log":"x","stream":"other","time":"2026-04-12T10:11:12Z"}"#;
        assert!(parse_docker_json("api", Path::new("/tmp/x.log"), raw).is_none());
    }
}
