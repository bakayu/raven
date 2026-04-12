use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use chrono::Utc;
use glob::glob;
use tokio::sync::mpsc;

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
    files: HashMap<PathBuf, WatchedFile>,
    tx: mpsc::Sender<LogBatch>,
    batch_size: usize,
}

impl LogTailer {
    /// Build a new tailer from configured log sources.
    ///
    /// - Expands glob patterns.
    /// - Initializes watch entries.
    /// - Attempts to open files at startup from end-of-file (tail behavior).
    pub fn new(sources: Vec<LogSource>, tx: mpsc::Sender<LogBatch>) -> anyhow::Result<Self> {
        let mut watcher = Watcher::new()?;
        let mut files = HashMap::new();

        for source in sources {
            let paths = resolve_source_paths(&source.path)
                .with_context(|| format!("failed to resolve log source path {}", source.path))?;

            for path in paths {
                let mut watched = WatchedFile::new(
                    source.name.clone(),
                    path.clone(),
                    source.format.clone(),
                    source.stream,
                );

                let _ = watched.try_open(true)?;

                if let Some(parent) = path.parent() {
                    watcher.add_parent_dir(parent).with_context(|| {
                        format!("failed to watch parent dir {}", parent.display())
                    })?;
                }

                if watched.file.is_some() {
                    watcher
                        .add_file(&path)
                        .with_context(|| format!("failed to watch file {}", path.display()))?;
                }

                files.insert(path, watched);
            }
        }

        Ok(Self {
            watcher,
            files,
            tx,
            batch_size: DEFAULT_BATCH_SIZE,
        })
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
    fn handle_created(&mut self, path: &Path) -> anyhow::Result<()> {
        let opened = {
            let Some(wf) = self.files.get_mut(path) else {
                return Ok(());
            };

            if wf.file.is_none() {
                wf.try_open(false)?
            } else {
                false
            }
        };

        if opened {
            self.watcher.add_file(path)?;
        }

        Ok(())
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

            self.tx
                .send(out)
                .await
                .map_err(|_| anyhow!("log channel closed"))?;
        }

        Ok(())
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
    let has_glob = pattern.contains('*') || pattern.contains('?') || pattern.contains('[');

    if !has_glob {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
