use std::path::PathBuf;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub source: String,
    pub path: PathBuf,
    pub line: String,
    pub stream: LogStream,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct LogBatch {
    pub source: String,
    pub entries: Vec<LogEntry>,
}
