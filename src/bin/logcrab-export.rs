// LogCrab - GPL-3.0-or-later
// Copyright (C) 2026 Daniel Freiermuth

//! `logcrab-export` — convert log files to NDJSON for training and inference.
//!
//! Each output line is a JSON object with the fields:
//! ```text
//! { "line_number": N, "timestamp_unix_ms": M, "message": "...", "source_file": "name.log", "filetype": "logcat" }
//! ```
//!
//! Format is auto-detected using the macro-generated `export_dispatch` in
//! `logcrab::core::log_store`, which mirrors the detection logic of the viewer
//! but has no UI dependencies. Config and file-state default to their `Default`
//! values so timestamps are raw and uncalibrated.
//!
//! Usage: `logcrab-export <file1> [file2 ...]`

use std::io::{self, BufWriter, Write};
use std::path::Path;

use clap::Parser;

// ─── CLI ─────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "logcrab-export")]
#[command(author = "LogCrab Team")]
#[command(version)]
#[command(
    about = "Export log files to NDJSON for sidecar scoring and training",
    long_about = "Reads one or more log files, auto-detects the format, and writes one JSON \
                  object per line to stdout. Each object carries the canonical message text \
                  (TAG:text for logcat, body for DLT, etc.), the raw uncalibrated timestamp, \
                  the source file name, and the detected filetype slug."
)]
struct Args {
    /// Log file(s) to export
    #[arg(value_name = "FILE", required = true)]
    files: Vec<std::path::PathBuf>,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let stdout = io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    let mut had_error = false;

    for path in &args.files {
        if let Err(e) = export_file(path, &mut out) {
            eprintln!("logcrab-export: {}: {e:#}", path.display());
            had_error = true;
        }
    }

    out.flush()?;

    if had_error {
        std::process::exit(1);
    }

    Ok(())
}

fn export_file(path: &Path, out: &mut impl Write) -> anyhow::Result<()> {
    logcrab::core::log_store::export_dispatch(path, out)
}

