/// `LogCrab` - A polyscopic anomaly explorer
///
/// Copyright (C) 2026 Daniel Freiermuth
///
/// This program is free software: you can redistribute it and/or modify
/// it under the terms of the GNU General Public License as published by
/// the Free Software Foundation, either version 3 of the License, or
/// (at your option) any later version.
///
/// This program is distributed in the hope that it will be useful,
/// but WITHOUT ANY WARRANTY; without even the implied warranty of
/// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
/// GNU General Public License for more details.
///
/// You should have received a copy of the GNU General Public License
/// along with this program.  If not, see <https://www.gnu.org/licenses/>.
mod anomaly;
mod config;
mod core;
mod filetype;
mod input;
mod parser;
mod ui;

use clap::Parser;
use egui::IconData;
use std::path::PathBuf;
use ui::app::LogCrabApp;

#[cfg(feature = "ram-profiling")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

#[derive(Parser, Debug)]
#[command(name = "logcrab")]
#[command(author = "LogCrab Team")]
#[command(version = "0.1.0")]
#[command(about = "Analyze log files with anomaly detection and pattern matching", long_about = None)]
struct Args {
    /// Path(s) to log file(s) to open
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Path for the DHAT heap profiling output (only used when built with --features ram-profiling)
    #[cfg(feature = "ram-profiling")]
    #[arg(
        long = "profile-output",
        value_name = "PROFILE_FILE",
        default_value = "dhat-heap.json"
    )]
    profile_output: PathBuf,
}

fn main() -> eframe::Result<()> {
    // Initialize tracing subscriber with millisecond precision timestamps.
    // Set RUST_LOG environment variable to override (e.g., RUST_LOG=debug)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();

    tracing::info!(
        "LogCrab starting up (version {})",
        env!("CARGO_PKG_VERSION")
    );

    #[cfg(feature = "ram-profiling")]
    let _profiler = {
        let args_early = Args::parse();
        tracing::info!(
            "RAM profiling enabled, output: {}",
            args_early.profile_output.display()
        );
        dhat::Profiler::builder()
            .file_name(args_early.profile_output)
            .build()
    };

    #[cfg(feature = "cpu-profiling")]
    {
        tracing::info!("CPU profiling enabled with Tracy - run Tracy profiler to connect");
    }

    let args = Args::parse();

    if !args.files.is_empty() {
        tracing::info!("Opening {} file(s) from command line", args.files.len());
        for file in &args.files {
            tracing::info!("  - {}", file.display());
        }
    }

    // Load app icon
    let icon_data = eframe::icon_data::from_png_bytes(include_bytes!("../logo.png"))
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to load app icon: {e}");
            IconData::default()
        });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(icon_data),
        ..Default::default()
    };

    eframe::run_native(
        "LogCrab - Log Anomaly Explorer",
        native_options,
        Box::new(move |cc| Ok(Box::new(LogCrabApp::new(cc, args.files)))),
    )
}
