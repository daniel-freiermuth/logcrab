mod parser;
mod anomaly;
mod ui;
mod app;

use app::LogOwlApp;
use clap::Parser;
use std::path::PathBuf;

#[cfg(feature = "ram-profiling")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// LogOwl - An intelligent log anomaly explorer
#[derive(Parser, Debug)]
#[command(name = "logowl")]
#[command(author = "LogOwl Team")]
#[command(version = "0.1.0")]
#[command(about = "Analyze log files with anomaly detection and pattern matching", long_about = None)]
struct Args {
    /// Path to the log file to open
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,
    
    /// Path for the DHAT heap profiling output (only used when built with --features ram-profiling)
    #[cfg(feature = "ram-profiling")]
    #[arg(long = "profile-output", value_name = "PROFILE_FILE", default_value = "dhat-heap.json")]
    profile_output: PathBuf,
}

fn main() -> eframe::Result<()> {
    #[cfg(feature = "ram-profiling")]
    let _profiler = {
        let args_early = Args::parse();
        dhat::Profiler::builder()
            .file_name(args_early.profile_output.clone())
            .build()
    };
    
    let args = Args::parse();
    
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 800.0])
            .with_min_inner_size([800.0, 600.0])
            .with_icon(
                eframe::icon_data::from_png_bytes(&[])
                    .unwrap_or_default()
            ),
        ..Default::default()
    };
    
    eframe::run_native(
        "LogOwl - Log Anomaly Explorer",
        native_options,
        Box::new(move |cc| Ok(Box::new(LogOwlApp::new(cc, args.file)))),
    )
}
