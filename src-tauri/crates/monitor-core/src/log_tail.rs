use std::{
    collections::BTreeMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

use chrono::{DateTime, Utc};

use crate::LogChunk;

#[derive(Default)]
pub(crate) struct LogTail {
    positions: BTreeMap<PathBuf, u64>,
}

impl LogTail {
    pub(crate) fn retain_paths(&mut self, keep: &std::collections::BTreeSet<PathBuf>) {
        self.positions.retain(|path, _| keep.contains(path));
    }

    pub(crate) fn poll(
        &mut self,
        path: &Path,
        initial_bytes: u64,
    ) -> std::io::Result<Option<LogChunk>> {
        if !path.exists() {
            self.positions.remove(path);
            return Ok(None);
        }
        let len = std::fs::metadata(path)?.len();
        let previous = self.positions.get(path).copied();
        let start = match previous {
            Some(position) if position <= len => position,
            Some(_) => 0,
            None => len.saturating_sub(initial_bytes),
        };
        let chunk = read_log_chunk(path, start)?;
        self.positions.insert(path.to_owned(), chunk.end);
        if chunk.text.is_empty() {
            Ok(None)
        } else {
            Ok(Some(chunk))
        }
    }
}

pub fn read_log_chunk(path: &Path, from: u64) -> std::io::Result<LogChunk> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let mut start = from.min(len);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if start > 0 {
        let skip = bytes
            .iter()
            .take(3)
            .take_while(|&&byte| byte & 0b1100_0000 == 0b1000_0000)
            .count();
        start += skip as u64;
        bytes.drain(..skip);
    }
    let valid = match std::str::from_utf8(&bytes) {
        Ok(_) => bytes.len(),
        Err(error) => error.valid_up_to(),
    };
    let text = String::from_utf8_lossy(&bytes[..valid]).into_owned();
    Ok(LogChunk {
        text,
        start,
        end: start + valid as u64,
        truncated_before: from > 0 || from > len,
    })
}

pub(crate) fn modified_at(path: &Path) -> Option<DateTime<Utc>> {
    std::fs::metadata(path)
        .ok()?
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
}

pub(crate) fn latest_message(log: &str) -> Option<String> {
    let lines: Vec<_> = log.lines().collect();
    let mut result = None;
    for (index, line) in lines.iter().enumerate() {
        if !line.starts_with('[') {
            continue;
        }
        let Some((_, message)) = line.split_once("] ") else {
            continue;
        };
        let text = if message == "Assistant message" || message == "Final output" {
            let body = lines.get(index + 1).copied().unwrap_or("");
            if body.is_empty() {
                message.to_owned()
            } else {
                format!("{message}: {body}")
            }
        } else {
            message.to_owned()
        };
        result = Some(text.chars().take(200).collect());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_recreate_delete_and_multibyte() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fiction.log");
        let mut tail = LogTail::default();
        std::fs::write(&path, "[2026-01-01T00:00:00Z] Hello\n").unwrap();
        assert!(
            tail.poll(&path, 1024)
                .unwrap()
                .unwrap()
                .text
                .contains("Hello")
        );
        std::fs::write(&path, "[2026-01-01T00:00:00Z] Hello\n日").unwrap();
        assert_eq!(tail.poll(&path, 1024).unwrap().unwrap().text, "日");
        std::fs::write(&path, "x").unwrap();
        assert_eq!(tail.poll(&path, 1024).unwrap().unwrap().text, "x");
        std::fs::remove_file(&path).unwrap();
        assert!(tail.poll(&path, 1024).unwrap().is_none());
        std::fs::write(&path, "日").unwrap();
        assert_eq!(read_log_chunk(&path, 1).unwrap().text, "");
        std::fs::write(&path, [b'x', 0xE6]).unwrap();
        let mut split_tail = LogTail::default();
        let first = split_tail.poll(&path, 1024).unwrap().unwrap();
        assert_eq!(first.text, "x");
        assert_eq!(first.end, 1);
        std::fs::write(&path, [b'x', 0xE6, 0x97, 0xA5]).unwrap();
        assert_eq!(split_tail.poll(&path, 1024).unwrap().unwrap().text, "日");
    }

    #[test]
    fn block_message() {
        assert_eq!(
            latest_message("[2026] Starting.\n\n[2026] Assistant message\n架空の本文\n"),
            Some("Assistant message: 架空の本文".into())
        );
    }
}
