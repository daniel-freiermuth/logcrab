# LogCrab 🦀

An intelligent log anomaly explorer built with Rust and egui. LogCrab helps developers and system administrators debug by automatically highlighting unusual, novel, or interesting log lines in large log files.

## Features

- **Visual Anomaly Detection**: Color-coded visualization
- **Live Regex Search**: Real-time filter with regex support and match highlighting
- **Bookmarks**: Right-click to bookmark important lines
- **Multi-Panel View**: Multiple filters show the logs from different perspectives for better understanding
- **Multi-Format Support**: Supports Android logcat, DLT files and generic log formats
- **No Training Required**: Works immediately on any log file

## Getting started

```bash
// Install rustup
$ rustup toolchain install stable
$ cd logcrab
$ cargo run --release
```

## Anomaly Scoring Components

1. **Rarity Scorer**
   - Identifies structurally unique log lines
   - Uses template normalization (numbers → `<NUM>`, UUIDs → `<UUID>`, etc.)
   - Tracks frequency of normalized patterns

2. **Temporal Scorer**
   - Detects time-based anomalies
   - Adaptive 30-second window
   - Identifies burst patterns and long absences

3. **Entropy Scorer**
   - Measures information content
   - Detects unusual message patterns
   - Identifies messages with abnormal length or complexity

4. **Severity Scorer**
   - Boosts ERROR and FATAL log levels
   - Detects sudden severity transitions
   - Tracks log level patterns over time

## Contributing

Contributions are welcome! Areas for improvement:
- Additional log format support
- New anomaly detection algorithms
- Performance optimizations
- UI enhancements (keyboard shortcuts, themes, etc.)

## Author

Built with ❤️ for developers who deserve decent log analysis tools
