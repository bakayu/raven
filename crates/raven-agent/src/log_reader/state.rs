use std::fs::File;
use std::path::PathBuf;

use crate::configuration::{LogFormat, Stream};

pub struct WatchedFile {
    pub source: String,
    pub path: PathBuf,
    pub format: LogFormat,
    pub configured_stream: Option<Stream>,
    pub file: Option<File>,
    pub offset: u64,
    pub partial: String,
    pub inode: Option<u64>,
}
