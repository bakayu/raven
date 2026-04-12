use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use super::model::{LogEntry, LogStream};

#[derive(Debug, Deserialize)]
struct DockerJsonLine {
    log: String,
    stream: String,
    time: DateTime<Utc>,
}

pub fn parse_plain(
    source: &str,
    path: &Path,
    raw: &str,
    configured_stream: LogStream,
    observed_at: DateTime<Utc>,
) -> Option<LogEntry> {
    todo!()
}

pub fn parse_docker_json(source: &str, path: &Path, raw: &str) -> Option<LogEntry> {
    todo!()
}
