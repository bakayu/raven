pub mod common;
pub mod model;
pub mod parser;
pub mod reader;
pub mod state;
pub mod tailer;
pub mod watcher;

use common::normalize_line;
pub use model::{LogBatch, LogEntry, LogStream};
pub use tailer::LogTailer;
