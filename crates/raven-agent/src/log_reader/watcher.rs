use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{Context, anyhow};
use inotify::{Event, EventMask, EventStream, Inotify, WatchDescriptor, WatchMask};
use tokio_stream::StreamExt;

/// Wrapper for inotify events
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TailerEvent {
    Modified(PathBuf),
    Rotated(PathBuf),
    Created(PathBuf),
}

/// Inotify wrapper for tracked files and parent directories
pub struct Watcher {
    stream: EventStream<Vec<u8>>,

    file_wd_to_path: HashMap<WatchDescriptor, PathBuf>,
    file_path_to_wd: HashMap<PathBuf, WatchDescriptor>,

    dir_wd_to_path: HashMap<WatchDescriptor, PathBuf>,
    dir_path_to_wd: HashMap<PathBuf, WatchDescriptor>,
}

impl Watcher {
    pub fn new() -> anyhow::Result<Self> {
        let inotify = Inotify::init().context("failed to initialize inotify")?;
        let stream = inotify
            .into_event_stream(vec![0; 64 * 1024])
            .context("failed to convert inotify into async stream")?;

        Ok(Self {
            stream,
            file_wd_to_path: HashMap::new(),
            file_path_to_wd: HashMap::new(),
            dir_wd_to_path: HashMap::new(),
            dir_path_to_wd: HashMap::new(),
        })
    }

    /// Watch a single file path
    pub fn add_file(&mut self, path: &Path) -> anyhow::Result<()> {
        let normalized = path.to_path_buf();
        if self.file_path_to_wd.contains_key(&normalized) {
            return Ok(());
        }

        let mask = WatchMask::MODIFY
            | WatchMask::CLOSE_WRITE
            | WatchMask::MOVE_SELF
            | WatchMask::DELETE_SELF
            | WatchMask::ATTRIB;

        let wd = self
            .stream
            .watches()
            .add(path, mask)
            .with_context(|| format!("failed to watch file path {}", path.display()))?;

        self.file_wd_to_path.insert(wd.clone(), normalized.clone());
        self.file_path_to_wd.insert(normalized, wd);

        Ok(())
    }

    /// Watch a parent directory to detect log file creation/rotation.
    pub fn add_parent_dir(&mut self, path: &Path) -> anyhow::Result<()> {
        let normalized = path.to_path_buf();
        if self.dir_path_to_wd.contains_key(&normalized) {
            return Ok(());
        }

        let mask = WatchMask::CREATE | WatchMask::MOVED_TO;

        let wd = self
            .stream
            .watches()
            .add(path, mask)
            .with_context(|| format!("failed to watch parent directory {}", path.display()))?;

        self.dir_wd_to_path.insert(wd.clone(), normalized.clone());
        self.dir_path_to_wd.insert(normalized, wd);

        Ok(())
    }

    /// Read one or more fs events and normalize them.
    /// Return once at least one normalized event is produced.
    pub async fn next_events(&mut self) -> anyhow::Result<Vec<TailerEvent>> {
        let mut dedup: HashSet<TailerEvent> = HashSet::new();

        while dedup.is_empty() {
            let next = self
                .stream
                .next()
                .await
                .ok_or_else(|| anyhow!("inotify event stream ended unexpectedly"))?;

            let event = next.context("failed reading inotify event from async stream")?;
            self.map_event(event, &mut dedup);
        }

        Ok(dedup.into_iter().collect())
    }

    /// Convert one raw inotify event to one or more normalized tailer events.
    fn map_event(&mut self, event: Event<OsString>, dedup: &mut HashSet<TailerEvent>) {
        if event.mask.contains(EventMask::IGNORED) {
            self.remove_watch_mappings(&event.wd);
            return;
        }

        if let Some(path) = self.file_wd_to_path.get(&event.wd).cloned() {
            if event.mask.contains(EventMask::MOVE_SELF)
                || event.mask.contains(EventMask::DELETE_SELF)
            {
                self.remove_watch_mappings(&event.wd);
                dedup.insert(TailerEvent::Rotated(path));
                return;
            }

            if event.mask.contains(EventMask::MODIFY)
                || event.mask.contains(EventMask::CLOSE_WRITE)
                || event.mask.contains(EventMask::ATTRIB)
            {
                dedup.insert(TailerEvent::Modified(path));
            }

            return;
        }

        if let Some(dir_path) = self.dir_wd_to_path.get(&event.wd).cloned()
            && (event.mask.contains(EventMask::CREATE) || event.mask.contains(EventMask::MOVED_TO))
            && event.name.is_some()
        {
            let name = event.name.expect("checked is_some");
            dedup.insert(TailerEvent::Created(dir_path.join(name)));
        }
    }

    /// Cleanup map entries when inotify sends `IGNORED`.
    fn remove_watch_mappings(&mut self, wd: &WatchDescriptor) {
        if let Some(path) = self.file_wd_to_path.remove(wd) {
            self.file_path_to_wd.remove(&path);
        }

        if let Some(path) = self.dir_wd_to_path.remove(wd) {
            self.dir_path_to_wd.remove(&path);
        }
    }
}
