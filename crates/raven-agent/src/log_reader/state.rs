use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use crate::configuration::{LogFormat, Stream};

/// A wrapper over watched files
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

impl WatchedFile {
    pub fn new(
        source: String,
        path: PathBuf,
        format: LogFormat,
        configured_stream: Option<Stream>,
    ) -> Self {
        Self {
            source,
            path,
            format,
            configured_stream,
            file: None,
            offset: 0,
            partial: String::new(),
            inode: None,
        }
    }

    /// oopens file and initializes inode and offset
    pub fn try_open(&mut self, start_at_end: bool) -> io::Result<bool> {
        match OpenOptions::new().read(true).open(&self.path) {
            Ok(file) => {
                let metadata = file.metadata()?;
                let len = metadata.len();
                self.inode = Some(metadata.ino());
                self.offset = if start_at_end {
                    len
                } else {
                    self.offset.min(len)
                };
                self.partial.clear();
                self.file = Some(file);

                Ok(true)
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                self.file = None;
                self.inode = None;
                self.offset = 0;
                self.partial.clear();

                Ok(false)
            }
            Err(err) => Err(err),
        }
    }

    /// close old handle, clear partial, reset offset and reopen if exsits
    pub fn reopen(&mut self, start_at_end: bool) -> io::Result<bool> {
        self.file = None;
        self.inode = None;
        self.partial.clear();
        self.offset = 0;

        self.try_open(start_at_end)
    }

    /// if metadata len is less than offset, reset offset to zero
    pub fn handle_truncate(&mut self) -> io::Result<()> {
        let Some(file) = self.file.as_ref() else {
            return Ok(());
        };

        let len = file.metadata()?.len();
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }

        Ok(())
    }

    /// compared inode of open file and current path
    pub fn inode_changed_on_disk(&self) -> io::Result<bool> {
        let Some(prev_inode) = self.inode else {
            return Ok(false);
        };

        match std::fs::metadata(&self.path) {
            Ok(meta) => Ok(meta.ino() != prev_inode),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(true),
            Err(err) => Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();

        std::env::temp_dir().join(format!("{}_{}_{}.log", name, std::process::id(), nanos))
    }

    #[test]
    fn try_open_existing_file_sets_inode_and_offset() {
        let path = temp_path("state_open");
        fs::write(&path, "abc\n").expect("write");

        let mut wf = WatchedFile::new(
            "src".to_string(),
            path.clone(),
            LogFormat::Plain,
            Some(Stream::Stdout),
        );

        let opened = wf.try_open(true).expect("open");
        assert!(opened);
        assert!(wf.file.is_some());
        assert!(wf.inode.is_some());
        assert_eq!(wf.offset, fs::metadata(&path).expect("meta").len());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn try_open_missing_returns_false() {
        let path = temp_path("state_missing");
        let mut wf = WatchedFile::new("src".to_string(), path, LogFormat::Plain, None);

        let opened = wf.try_open(true).expect("open");
        assert!(!opened);
        assert!(wf.file.is_none());
        assert!(wf.inode.is_none());
        assert_eq!(wf.offset, 0);
    }

    #[test]
    fn handle_truncate_resets_offset() {
        let path = temp_path("state_truncate");
        fs::write(&path, "line1\nline2\n").expect("write");

        let mut wf = WatchedFile::new(
            "src".to_string(),
            path.clone(),
            LogFormat::Plain,
            Some(Stream::Stdout),
        );
        wf.try_open(false).expect("open");
        wf.offset = fs::metadata(&path).expect("meta").len();

        fs::write(&path, "x\n").expect("truncate + write");
        wf.handle_truncate().expect("truncate check");

        assert_eq!(wf.offset, 0);

        let _ = fs::remove_file(path);
    }

    #[test]
    fn inode_changed_on_disk_detects_rotation() {
        let path = temp_path("state_rotate");
        fs::write(&path, "a\n").expect("write");

        let mut wf = WatchedFile::new(
            "src".to_string(),
            path.clone(),
            LogFormat::Plain,
            Some(Stream::Stdout),
        );
        wf.try_open(false).expect("open");

        let rotated = path.with_extension("log.1");
        fs::rename(&path, &rotated).expect("rename");
        fs::write(&path, "new\n").expect("recreate");

        assert!(wf.inode_changed_on_disk().expect("inode check"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(rotated);
    }
}
