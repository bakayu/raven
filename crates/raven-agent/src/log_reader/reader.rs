use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

use crate::log_reader::common::normalize_line;

/// Read contents of a file from an offset to EOF.
/// Trailing incomplete segments are kept in `partial`.
///
/// Returns `Vec<String>`
pub fn read_new_lines(
    file: &mut File,
    offset: &mut u64,
    partial: &mut String,
) -> io::Result<Vec<String>> {
    file.seek(SeekFrom::Start(*offset))?;
    let mut bytes = Vec::new();
    let read_len = file.read_to_end(&mut bytes)?;
    if read_len == 0 {
        return Ok(Vec::new());
    }

    *offset = offset.saturating_add(read_len as u64);

    let chunk = String::from_utf8_lossy(&bytes);
    let mut combined = String::with_capacity(partial.len() + chunk.len());
    combined.push_str(partial);
    combined.push_str(&chunk);
    partial.clear();

    let mut lines = Vec::new();

    for segment in combined.split_inclusive('\n') {
        if segment.ends_with('\n') {
            lines.push(normalize_line(segment));
        } else {
            partial.push_str(segment);
        }
    }

    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
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
    fn reads_complete_lines_and_updates_offset() {
        let path = temp_path("reader_complete");
        fs::write(&path, "line1\nline2\n").expect("write test file");

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("open reader");
        let mut offset = 0_u64;
        let mut partial = String::new();

        let lines = read_new_lines(&mut file, &mut offset, &mut partial).expect("read lines");
        assert_eq!(lines, vec!["line1".to_string(), "line2".to_string()]);
        assert!(partial.is_empty());
        assert_eq!(offset, fs::metadata(&path).expect("metadata").len());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn second_read_returns_only_new_bytes() {
        let path = temp_path("reader_incremental");
        fs::write(&path, "line1\n").expect("write test file");

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("open reader");
        let mut offset = 0_u64;
        let mut partial = String::new();

        let first = read_new_lines(&mut file, &mut offset, &mut partial).expect("first read");
        assert_eq!(first, vec!["line1".to_string()]);

        {
            let mut appender = OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open appender");
            write!(appender, "line2\n").expect("append line");
            appender.sync_all().expect("sync append");
        }

        let second = read_new_lines(&mut file, &mut offset, &mut partial).expect("second read");
        assert_eq!(second, vec!["line2".to_string()]);
        assert!(partial.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn buffers_partial_line_and_completes_later() {
        let path = temp_path("reader_partial");
        fs::write(&path, "par").expect("write partial");

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("open reader");
        let mut offset = 0_u64;
        let mut partial = String::new();

        let first = read_new_lines(&mut file, &mut offset, &mut partial).expect("first read");
        assert!(first.is_empty());
        assert_eq!(partial, "par");

        {
            let mut appender = OpenOptions::new()
                .append(true)
                .open(&path)
                .expect("open appender");
            write!(appender, "tial\nnext\n").expect("append rest");
            appender.sync_all().expect("sync append");
        }

        let second = read_new_lines(&mut file, &mut offset, &mut partial).expect("second read");
        assert_eq!(second, vec!["partial".to_string(), "next".to_string()]);
        assert!(partial.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn trims_crlf_line_endings() {
        let path = temp_path("reader_crlf");
        fs::write(&path, "a\r\nb\r\n").expect("write test file");

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("open reader");
        let mut offset = 0_u64;
        let mut partial = String::new();

        let lines = read_new_lines(&mut file, &mut offset, &mut partial).expect("read lines");
        assert_eq!(lines, vec!["a".to_string(), "b".to_string()]);
        assert!(partial.is_empty());

        let _ = fs::remove_file(path);
    }

    #[test]
    fn no_new_bytes_returns_empty_without_mutating_partial() {
        let path = temp_path("reader_empty");
        fs::write(&path, "").expect("write empty file");

        let mut file = OpenOptions::new()
            .read(true)
            .open(&path)
            .expect("open reader");
        let mut offset = 0_u64;
        let mut partial = "carry".to_string();

        let lines = read_new_lines(&mut file, &mut offset, &mut partial).expect("read lines");
        assert!(lines.is_empty());
        assert_eq!(offset, 0);
        assert_eq!(partial, "carry");

        let _ = fs::remove_file(path);
    }
}
