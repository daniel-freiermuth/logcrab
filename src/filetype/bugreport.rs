// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

use chrono::Datelike;
use fancy_regex::Regex;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use super::logcat::{parse_logcat_line, LogcatLogLine};
use crate::filetype::{InputFileType, TextFileType};

// ============================================================================
// Bugreport parsing utilities
// ============================================================================

static DUMPSTATE_HEADER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"==\s*dumpstate:\s*(\d{4})-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}")
        .expect("valid regex literal")
});

/// Detect the year from bugreport header lines.
/// Scans the provided content (should be first few KB) for dumpstate header.
pub fn detect_year_from_header(content: &str) -> Option<i32> {
    for line in content.lines().take(50) {
        if let Ok(Some(caps)) = DUMPSTATE_HEADER.captures(line) {
            if let Ok(year) = caps[1].parse::<i32>() {
                tracing::info!("Detected year {year} from bugreport dumpstate header");
                return Some(year);
            }
        }
    }
    None
}

// ============================================================================
// BugreportFileType
// ============================================================================

/// Stateful reader for Android bugreport files.
///
/// Bugreport files contain logcat lines prefixed with a dumpstate header that
/// encodes the capture year. Must be registered **before** [`super::logcat::LogcatFileType`]
/// so that `looks_like` is checked first (bugreport ⊂ logcat pattern-space).
pub struct BugreportFileType {
    reader: BufReader<File>,
    year: i32,
    line_number: usize,
    bytes_read: u64,
}

impl BugreportFileType {
    /// Open a bugreport file for pull-based reading.
    ///
    /// Reads the first 4 KB to extract the capture year from the dumpstate header.
    /// Falls back to the current calendar year if no header is found.
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        use anyhow::Context as _;
        let mut file =
            File::open(path).with_context(|| format!("Failed to open {}", path.display()))?;

        let mut preview_buf = [0u8; 4096];
        let preview_n = file.read(&mut preview_buf).unwrap_or(0);
        let preview = String::from_utf8_lossy(&preview_buf[..preview_n]);
        let year = detect_year_from_header(&preview).unwrap_or_else(|| {
            tracing::warn!(
                "No dumpstate year found in {}, using current year",
                path.display()
            );
            chrono::Local::now().year()
        });

        file.seek(SeekFrom::Start(0))
            .with_context(|| format!("Failed to seek {}", path.display()))?;

        Ok(Self {
            reader: BufReader::new(file),
            year,
            line_number: 0,
            bytes_read: 0,
        })
    }
}

impl InputFileType for BugreportFileType {
    /// Bugreport files are logcat lines, so the line type is [`LogcatLogLine`].
    type LineType = LogcatLogLine;

    const FILE_EXTENSIONS: &'static [&'static str] = &["txt", "zip"];

    fn open(
        path: &::std::path::Path,
        _config: (),
        _file_state: std::sync::Arc<crate::filetype::logcat::LogcatFileState>,
    ) -> anyhow::Result<Self> {
        Self::open(path)
    }

    fn read(&mut self, lines_to_read: usize) -> anyhow::Result<Vec<Self::LineType>> {
        let mut result = Vec::with_capacity(lines_to_read);
        let mut buf = String::new();
        for _ in 0..lines_to_read {
            buf.clear();
            match self.reader.read_line(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    self.bytes_read += n as u64;
                    self.line_number += 1;
                    let raw = buf.trim_end_matches(['\n', '\r']).to_string();
                    if let Some(line) = parse_logcat_line(raw, self.line_number, self.year) {
                        result.push(line);
                    }
                }
                Err(e) => return Err(anyhow::anyhow!("Read error: {e}")),
            }
        }
        Ok(result)
    }

    fn bytes_consumed(&self) -> u64 {
        self.bytes_read
    }
}

impl TextFileType for BugreportFileType {
    /// Returns `true` when the file starts with an Android `dumpstate` header that
    /// includes an explicit year.
    fn looks_like(file: &mut dyn std::io::Read) -> bool {
        let mut buf = [0u8; 4096];
        let n = file.read(&mut buf).unwrap_or(0);
        let sample = String::from_utf8_lossy(&buf[..n]);
        detect_year_from_header(&sample).is_some()
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_year_from_header() {
        let content = "\n    ========================================================\n    == dumpstate: 2024-11-27 14:08:01\n    ========================================================\n    ";
        assert_eq!(detect_year_from_header(content), Some(2024));
    }

    #[test]
    fn test_detect_year_from_header_missing() {
        assert_eq!(
            detect_year_from_header("plain logcat output\n11-20 14:23:45.123 msg"),
            None
        );
    }
}
