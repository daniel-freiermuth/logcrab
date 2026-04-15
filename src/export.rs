// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

//! Export primitives shared by `logcrab-export` and the macro-generated
//! `export_dispatch` function in `core::log_store`.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use anyhow::Context as _;
use serde::Serialize;

use crate::filetype::{InputFileType, LineType};

/// One NDJSON record emitted by `logcrab-export`.
#[derive(Serialize)]
pub struct ExportRecord<'a> {
    pub line_number: usize,
    pub timestamp_unix_ms: i64,
    pub message: String,
    pub source_file: &'a str,
    pub filetype: &'a str,
}

const EXPORT_CHUNK: usize = 4096;

/// Read all lines from `path` using file type `FT` and write them as NDJSON.
///
/// Config and file-state are both `Default`, so timestamps are raw and
/// uncalibrated — honouring the stability invariant on [`LineType::timestamp`].
pub fn export_typed<FT: InputFileType>(
    path: &Path,
    filetype: &str,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let config = <<FT as InputFileType>::LineType as LineType>::Config::default();
    let file_state =
        Arc::new(<<FT as InputFileType>::LineType as LineType>::FileState::default());

    let mut reader = FT::open(path, config.clone(), Arc::clone(&file_state))
        .with_context(|| format!("failed to open {} as {filetype}", path.display()))?;

    let source_file = path
        .file_name()
        .unwrap_or_else(|| path.as_os_str())
        .to_string_lossy();

    loop {
        let lines = reader
            .read(EXPORT_CHUNK)
            .with_context(|| format!("read error in {}", path.display()))?;

        if lines.is_empty() {
            break;
        }

        for line in &lines {
            let ts = line.timestamp(&config, &file_state);
            let record = ExportRecord {
                line_number: line.line_number(),
                timestamp_unix_ms: ts.timestamp_millis(),
                message: line.message(),
                source_file: &source_file,
                filetype,
            };
            serde_json::to_writer(&mut *out, &record).context("failed to serialize record")?;
            writeln!(out).context("write error")?;
        }
    }

    Ok(())
}
