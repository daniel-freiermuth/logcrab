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

## Related tools and inspiration

### Chipmunk
https://github.com/esrlabs/chipmunk
DLT analyser with one supported filter. Saved filters with color-coding. Graphing. Written in Rust and Electron. Great DLT crate.

#### How it compares
- only single filter pane
- no anomaly detection
- only DLT

### CatSpy
https://github.com/Gegenbauer/CatSpy
Adb logcat analysis tool. One filter tool with vertical vertical split.

#### Caveats
- Pagination
- Bookmarks not saved
- Selected line reset on filter change
- View only synced one-way

### Lnav
https://github.com/tstack/lnav
https://github.com/javierhz/lnav-logcat-Android
Command line log analyzer. Doesn't support logcat or DLT off-the-shelf.

### toolong
https://github.com/Textualize/toolong

### dlt-viewer
https://github.com/COVESA/dlt-viewer

### DLT message analyzer
https://github.com/svlad-90/DLT-Message-Analyzer

### Lognote
https://github.com/cdcsgit/lognote

### AngleGrinder
https://github.com/rcoh/angle-grinder

### Pidcat
https://github.com/JakeWharton/pidcat

### Rustycat
https://github.com/cesarferreira/rustycat

### Netdata
https://github.com/netdata/netdata
OS metric dashboard that just works™ and has anomaly detection included.

## Contributing

Contributions are welcome! Areas for improvement:
- Additional log format support
- New anomaly detection algorithms
- Performance optimizations
- UI enhancements (keyboard shortcuts, themes, etc.)

## Author

Built with ❤️ for developers who deserve decent log analysis tools
