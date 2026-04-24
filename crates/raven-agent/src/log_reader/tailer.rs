use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow};
use chrono::Utc;
use glob::glob;
use tokio::sync::{Mutex, mpsc};

use super::model::{LogBatch, LogEntry, LogStream};
use super::parser::{parse_docker_json, parse_plain};
use super::reader::read_new_lines;
use super::state::WatchedFile;
use super::watcher::{TailerEvent, Watcher};
use crate::configuration::{LogFormat, LogSource, Stream};

const DEFAULT_BATCH_SIZE: usize = 100;

/// Main log tailer runtime.
///
/// Owns watcher state, tracked files and output channel.
pub struct LogTailer {
    watcher: Watcher,
    sources: Vec<LogSource>,
    files: HashMap<PathBuf, WatchedFile>,
    tx: mpsc::Sender<LogBatch>,
    rx_for_drop_oldest: Option<Arc<Mutex<mpsc::Receiver<LogBatch>>>>,
    batch_size: usize,
}

impl LogTailer {
    /// Build a new tailer from configured log sources.
    ///
    /// - Expands glob patterns.
    /// - Initializes watch entries.
    /// - Attempts to open files at startup from end-of-file (tail behavior).
    pub fn new(sources: Vec<LogSource>, tx: mpsc::Sender<LogBatch>) -> anyhow::Result<Self> {
        Self::build(sources, tx, None)
    }

    /// Build a new tailer with overflow behavior that drops oldest queued batches
    /// before sending new ones, keeping producer-side sends non-blocking.
    pub fn new_with_drop_oldest(
        sources: Vec<LogSource>,
        tx: mpsc::Sender<LogBatch>,
        rx_for_drop_oldest: Arc<Mutex<mpsc::Receiver<LogBatch>>>,
    ) -> anyhow::Result<Self> {
        Self::build(sources, tx, Some(rx_for_drop_oldest))
    }

    fn build(
        sources: Vec<LogSource>,
        tx: mpsc::Sender<LogBatch>,
        rx_for_drop_oldest: Option<Arc<Mutex<mpsc::Receiver<LogBatch>>>>,
    ) -> anyhow::Result<Self> {
        let mut tailer = Self {
            watcher: Watcher::new()?,
            sources,
            files: HashMap::new(),
            tx,
            rx_for_drop_oldest,
            batch_size: DEFAULT_BATCH_SIZE,
        };

        tailer.sync_sources(true)?;

        Ok(tailer)
    }

    /// Main async event loop:
    /// - read filesystem events
    /// - update file state
    /// - parse new lines
    /// - emit log batches
    pub async fn run(mut self) -> anyhow::Result<()> {
        loop {
            let events = self.watcher.next_events().await?;

            for event in events {
                match event {
                    TailerEvent::Modified(path) => self.handle_modified(&path).await?,
                    TailerEvent::Rotated(path) => self.handle_rotated(&path)?,
                    TailerEvent::Created(path) => self.handle_created(&path)?,
                }
            }
        }
    }

    /// Process a "modified" event:
    /// - ensure file handle exists
    /// - detect inode changes / truncation
    /// - read newly appended lines
    /// - parse and send batches
    async fn handle_modified(&mut self, path: &Path) -> anyhow::Result<()> {
        let mut should_add_watch = false;

        let maybe_batch = {
            let Some(wf) = self.files.get_mut(path) else {
                return Ok(());
            };

            if wf.file.is_none() {
                if wf.try_open(false)? {
                    should_add_watch = true;
                } else {
                    return Ok(());
                }
            }

            if wf.inode_changed_on_disk()? {
                let reopened = wf.reopen(false)?;
                should_add_watch |= reopened;
                if !reopened {
                    return Ok(());
                }
            }

            wf.handle_truncate()?;

            let Some(file) = wf.file.as_mut() else {
                return Ok(());
            };

            let lines = read_new_lines(file, &mut wf.offset, &mut wf.partial)?;
            if lines.is_empty() {
                return Ok(());
            }

            let entries = parse_lines_for_file(
                &wf.source,
                &wf.path,
                &wf.format,
                wf.configured_stream,
                lines,
            );

            if entries.is_empty() {
                return Ok(());
            }

            Some(LogBatch {
                source: wf.source.clone(),
                entries,
            })
        };

        if should_add_watch {
            self.watcher.add_file(path)?;
        }

        if let Some(batch) = maybe_batch {
            self.send_batches(batch).await?;
        }

        Ok(())
    }

    /// Process a rotation event by reopening the path.
    fn handle_rotated(&mut self, path: &Path) -> anyhow::Result<()> {
        let reopened = {
            let Some(wf) = self.files.get_mut(path) else {
                return Ok(());
            };
            wf.reopen(false)?
        };

        if reopened {
            self.watcher.add_file(path)?;
        }

        Ok(())
    }

    /// Process a created event,
    /// if this path is one we track and it is not opened yet, try opening it.
    fn handle_created(&mut self, _path: &Path) -> anyhow::Result<()> {
        // Re-scan configured sources so files that were absent at startup
        // (especially glob matches) are discovered as soon as they appear.
        self.sync_sources(false)
    }

    /// Send batches in fixed-size chunks to keep channel payload bounded.
    async fn send_batches(&self, batch: LogBatch) -> anyhow::Result<()> {
        if batch.entries.is_empty() {
            return Ok(());
        }

        let chunk_size = self.batch_size.max(1);
        for chunk in batch.entries.chunks(chunk_size) {
            let out = LogBatch {
                source: batch.source.clone(),
                entries: chunk.to_vec(),
            };

            self.send_one(out).await?;
        }

        Ok(())
    }

    fn sync_sources(&mut self, start_at_end: bool) -> anyhow::Result<()> {
        for source in self.sources.clone() {
            self.watch_source_discovery_dirs(&source)?;

            let paths = resolve_source_paths(&source.path)
                .with_context(|| format!("failed to resolve log source path {}", source.path))?;

            for path in paths {
                self.ensure_watched_file(&source, path, start_at_end)?;
            }
        }

        Ok(())
    }

    fn watch_source_discovery_dirs(&mut self, source: &LogSource) -> anyhow::Result<()> {
        let dirs = discovery_dirs_for_pattern(&source.path)
            .with_context(|| format!("failed to resolve discovery dirs for {}", source.path))?;

        for dir in dirs {
            if !dir.exists() {
                continue;
            }

            self.watcher.add_parent_dir(&dir).with_context(|| {
                format!(
                    "failed to watch parent dir {} for source pattern {}",
                    dir.display(),
                    source.path
                )
            })?;
        }

        Ok(())
    }

    fn ensure_watched_file(
        &mut self,
        source: &LogSource,
        path: PathBuf,
        start_at_end: bool,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent()
            && parent.exists()
        {
            self.watcher
                .add_parent_dir(parent)
                .with_context(|| format!("failed to watch parent dir {}", parent.display()))?;
        }

        if let Some(wf) = self.files.get_mut(&path) {
            if wf.file.is_none() {
                let opened = wf.try_open(start_at_end)?;
                if opened {
                    self.watcher.add_file(&path)?;
                }
            }

            return Ok(());
        }

        let mut watched = WatchedFile::new(
            source.name.clone(),
            path.clone(),
            source.format.clone(),
            source.stream,
        );

        let opened = watched.try_open(start_at_end)?;

        if opened {
            self.watcher
                .add_file(&path)
                .with_context(|| format!("failed to watch file {}", path.display()))?;
        }

        self.files.insert(path, watched);

        Ok(())
    }

    async fn send_one(&self, mut out: LogBatch) -> anyhow::Result<()> {
        if let Some(rx_for_drop_oldest) = &self.rx_for_drop_oldest {
            loop {
                match self.tx.try_send(out) {
                    Ok(()) => return Ok(()),
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Err(anyhow!("log channel closed"));
                    }
                    Err(mpsc::error::TrySendError::Full(returned)) => {
                        out = returned;

                        let mut rx = rx_for_drop_oldest.lock().await;
                        match rx.try_recv() {
                            Ok(_) => {}
                            Err(mpsc::error::TryRecvError::Empty) => {
                                drop(rx);
                                tokio::task::yield_now().await;
                            }
                            Err(mpsc::error::TryRecvError::Disconnected) => {
                                return Err(anyhow!("log channel closed"));
                            }
                        }
                    }
                }
            }
        }

        self.tx
            .send(out)
            .await
            .map_err(|_| anyhow!("log channel closed"))
    }
}

/// Parse a set of raw lines for one watched file.
fn parse_lines_for_file(
    source: &str,
    path: &Path,
    format: &LogFormat,
    configured_stream: Option<Stream>,
    lines: Vec<String>,
) -> Vec<LogEntry> {
    lines
        .into_iter()
        .filter_map(|line| match format {
            LogFormat::Plain => {
                let stream = configured_stream.map(map_config_stream)?;
                parse_plain(source, path, &line, stream, Utc::now())
            }
            LogFormat::DockerJson => parse_docker_json(source, path, &line),
        })
        .collect()
}

/// Map configuration stream enum to log model stream enum.
fn map_config_stream(stream: Stream) -> LogStream {
    match stream {
        Stream::Stdout => LogStream::Stdout,
        Stream::Stderr => LogStream::Stderr,
    }
}

/// Expand log source path.
///
/// - For literal paths: returns that one path.
/// - For glob patterns: returns all matched paths.
/// - If glob has no current match: returns empty list (tailer will wait for Created events on watched dirs).
fn resolve_source_paths(pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    if !is_glob_pattern(pattern) {
        return Ok(vec![PathBuf::from(pattern)]);
    }

    let mut out = Vec::new();
    for item in glob(pattern).with_context(|| format!("invalid glob pattern {}", pattern))? {
        match item {
            Ok(path) => out.push(path),
            Err(err) => return Err(anyhow!(err)),
        }
    }

    Ok(out)
}

fn discovery_dirs_for_pattern(pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    let base = base_watch_dir_for_pattern(pattern);
    if seen.insert(base.clone()) {
        out.push(base);
    }

    let parent_pattern = Path::new(pattern)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    if !is_glob_pattern(&parent_pattern) {
        let parent = PathBuf::from(parent_pattern);
        if seen.insert(parent.clone()) {
            out.push(parent);
        }

        return Ok(out);
    }

    for item in glob(&parent_pattern)
        .with_context(|| format!("invalid parent glob pattern {}", parent_pattern))?
    {
        let path = item.map_err(|err| anyhow!(err))?;

        if !path.is_dir() {
            continue;
        }

        if seen.insert(path.clone()) {
            out.push(path);
        }
    }

    Ok(out)
}

fn base_watch_dir_for_pattern(pattern: &str) -> PathBuf {
    let Some(glob_index) = first_glob_index(pattern) else {
        return Path::new(pattern)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    };

    let prefix = &pattern[..glob_index];
    if prefix.is_empty() {
        return PathBuf::from(".");
    }

    Path::new(prefix)
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_glob_pattern(pattern: &str) -> bool {
    first_glob_index(pattern).is_some()
}

fn first_glob_index(pattern: &str) -> Option<usize> {
    pattern
        .char_indices()
        .find_map(|(index, ch)| matches!(ch, '*' | '?' | '[').then_some(index))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use chrono::Utc;
    use tokio::sync::{Mutex, mpsc};

    use super::*;

    fn test_entry(line: &str) -> LogEntry {
        LogEntry {
            source: "app".to_string(),
            path: PathBuf::from("/tmp/app.log"),
            stream: LogStream::Stdout,
            timestamp: Utc::now(),
            line: line.to_string(),
        }
    }

    #[test]
    fn parse_lines_plain_uses_configured_stream() {
        let entries = parse_lines_for_file(
            "nginx",
            Path::new("/tmp/nginx.log"),
            &LogFormat::Plain,
            Some(Stream::Stdout),
            vec!["hello".to_string()],
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].stream, LogStream::Stdout);
        assert_eq!(entries[0].line, "hello");
    }

    #[test]
    fn parse_lines_docker_uses_embedded_stream() {
        let entries = parse_lines_for_file(
            "api",
            Path::new("/tmp/docker.log"),
            &LogFormat::DockerJson,
            None,
            vec![r#"{"log":"oops\n","stream":"stderr","time":"2026-04-12T10:11:12Z"}"#.to_string()],
        );

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].stream, LogStream::Stderr);
        assert_eq!(entries[0].line, "oops");
    }

    #[test]
    fn resolve_source_paths_literal() {
        let out = resolve_source_paths("/var/log/nginx/access.log").expect("resolve");
        assert_eq!(out, vec![PathBuf::from("/var/log/nginx/access.log")]);
    }

    #[test]
    fn discovery_dirs_include_glob_base_dir() {
        let dirs = discovery_dirs_for_pattern("/var/lib/docker/containers/abc123*/*.log")
            .expect("resolve discovery dirs");

        assert!(dirs.contains(&PathBuf::from("/var/lib/docker/containers")));
    }

    #[tokio::test]
    async fn send_batches_drop_oldest_when_full() {
        let (tx, rx) = mpsc::channel(2);
        let rx = Arc::new(Mutex::new(rx));

        let tailer =
            LogTailer::new_with_drop_oldest(Vec::new(), tx, rx.clone()).expect("build tailer");

        tailer
            .send_batches(LogBatch {
                source: "app".to_string(),
                entries: vec![test_entry("one")],
            })
            .await
            .expect("send batch one");

        tailer
            .send_batches(LogBatch {
                source: "app".to_string(),
                entries: vec![test_entry("two")],
            })
            .await
            .expect("send batch two");

        tailer
            .send_batches(LogBatch {
                source: "app".to_string(),
                entries: vec![test_entry("three")],
            })
            .await
            .expect("send batch three");

        let mut guard = rx.lock().await;
        let first = guard.try_recv().expect("first batch exists");
        let second = guard.try_recv().expect("second batch exists");

        assert_eq!(first.entries[0].line, "two");
        assert_eq!(second.entries[0].line, "three");
    }
}
