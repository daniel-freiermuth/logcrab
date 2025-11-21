# DecentLog 🔍

An intelligent log anomaly explorer built with Rust and egui. DecentLog helps developers and system administrators debug by automatically highlighting unusual, novel, or interesting log lines in large log files.

## Features

- 🚀 **High Performance**: Handles 100-500 MB log files efficiently
- 🎨 **Visual Anomaly Detection**: Color-coded visualization (white → pink → orange → red)
- 🔍 **Live Regex Search**: Real-time search with regex support and match highlighting
- 📌 **Bookmarks**: Right-click to bookmark important lines (persisted to `.bookmarks` file)
- 👁️ **Dual-Panel View**: Context view shows surrounding lines for better understanding
- 🧵 **Multi-Format Support**: Supports Android logcat and generic log formats
- 🧠 **Smart Scoring**: Multi-dimensional anomaly detection including:
  - Template rarity (structural uniqueness)
  - Temporal patterns (time-based analysis)
  - Message entropy (information content)
  - Severity transitions (ERROR/FATAL detection)
- 📊 **Real-time Processing**: Single-pass processing with online scoring
- 🎯 **No Training Required**: Works immediately on any log file
- 🔬 **Optional Profiling**: Built-in CPU and RAM profiling for performance analysis

## Architecture

DecentLog uses a sophisticated multi-component anomaly scoring system:

### Anomaly Scoring Components

1. **Rarity Scorer** (weight: 3.0)
   - Identifies structurally unique log lines
   - Uses template normalization (numbers → `<NUM>`, UUIDs → `<UUID>`, etc.)
   - Tracks frequency of normalized patterns

2. **Temporal Scorer** (weight: 2.0)
   - Detects time-based anomalies
   - Adaptive 30-second window
   - Identifies burst patterns and long absences

3. **Entropy Scorer** (weight: 1.5)
   - Measures information content
   - Detects unusual message patterns
   - Identifies messages with abnormal length or complexity

4. **Severity Scorer** (weight: 2.5)
   - Boosts ERROR and FATAL log levels
   - Detects sudden severity transitions
   - Tracks log level patterns over time

### Supported Log Formats

- **Android Logcat**:
  - Threadtime: `MM-DD HH:MM:SS.mmm PID TID L TAG: message`
  - Time: `MM-DD HH:MM:SS.mmm L/TAG(PID): message`
  - Brief: `L/TAG(PID): message`
  - Long: `[ MM-DD HH:MM:SS.mmm PID: TID L/TAG ]`

- **Generic Formats**:
  - ISO 8601 timestamps
  - Syslog format
  - Custom bracketed timestamps
  - Auto-detection of log levels (ERROR, WARN, INFO, DEBUG, etc.)

## Building

### Standard Build
```bash
cargo build --release
```

The binary will be available at `target/release/decentlog`

### Build with Profiling
```bash
# CPU profiling only
cargo build --release --features cpu-profiling

# RAM profiling only
cargo build --release --features ram-profiling

# Both
cargo build --release --features profiling
```

See [PROFILING.md](PROFILING.md) for detailed profiling instructions.

## Usage

### Basic Usage

1. Run the application:
   ```bash
   cargo run --release
   
   # Or open a file directly
   cargo run --release path/to/logfile.txt
   ```

2. Click "Open Log File" or use the File menu to load a log file

3. Use the search bar to filter logs:
   - Enter any regex pattern (e.g., `ERROR|FATAL`, `\d+\.\d+\.\d+\.\d+` for IPs)
   - Matching text is highlighted in **yellow** within each line
   - Only matching lines are shown
   - Invalid regex shows an error message
   - Click "Clear" to reset the search

4. View color-coded anomaly scores:
   - **White** (0-30): Normal, expected log lines
   - **Pink** (30-60): Slightly unusual
   - **Orange** (60-80): Suspicious, worth investigating
   - **Red** (80-100): Highly anomalous, likely important

5. **Bookmark important lines**:
   - Right-click any row to bookmark it (★ appears)
   - Bookmarked rows are highlighted in golden/brown color
   - Bookmarks are saved to `filename.bookmarks` and persist across sessions

6. **Use dual-panel view**:
   - Left panel: Context view showing ±50 lines around selected line
   - Right panel: Filtered view with search results
   - Click any line to select it and update context view

### Command-Line Options

```bash
# Open a specific file
decentlog logfile.txt

# Custom DHAT output location (with ram-profiling feature)
decentlog logfile.txt --profile-output=custom-profile.json

# Show help
decentlog --help
```

## Search Examples

- `ERROR|FATAL` - Find all errors and fatal messages
- `\d+\.\d+\.\d+\.\d+` - Find IP addresses
- `timeout|failed|exception` - Find common error patterns (case-insensitive)
- `PID:\s*\d+` - Find lines with PID information
- `^11-20 08:15` - Find logs from specific time
- `(connect|disconnect)ion` - Find connection-related events

## Example

A sample log file is provided in `sample_log.txt`. This demonstrates:
- Normal repeated operations (low scores)
- Errors and exceptions (high scores)
- Rare events (high scores)
- Mixed log formats (all parsed correctly)

## Design Principles

1. **Single-Pass Processing**: Score lines before updating statistics
2. **No External Baseline**: Learn from the file being analyzed
3. **Extensible Architecture**: Easy to add new scoring components
4. **Performance First**: Handle large files without preprocessing
5. **Future-Ready**: Architecture designed for embedding support (v2)

## Project Structure

```
src/
├── main.rs              # Application entry point
├── app.rs               # Main GUI application logic
├── parser/              # Log parsing
│   ├── mod.rs
│   ├── line.rs          # LogLine data structure
│   ├── logcat.rs        # Android logcat parsers
│   └── generic.rs       # Generic log parser
├── anomaly/             # Anomaly detection
│   ├── mod.rs
│   ├── scorer.rs        # Scoring trait and composite scorer
│   ├── rarity.rs        # Template rarity detection
│   ├── temporal.rs      # Time-based anomaly detection
│   └── entropy.rs       # Entropy and severity scoring
└── ui/                  # User interface
    ├── mod.rs
    └── log_view.rs      # Scrollable log viewer widget
```

## Dependencies

- `eframe` / `egui`: Cross-platform GUI framework
- `chrono`: Date and time handling
- `regex`: Pattern matching for log parsing
- `ahash`: Fast hashing for frequency tracking
- `rfd`: Native file dialogs

## Roadmap

### v1.0 (Current)
- ✅ Multi-format log parsing
- ✅ Multi-component anomaly scoring
- ✅ Color-coded visualization
- ✅ Large file support
- ✅ Dual-panel view (context + filtered)
- ✅ Bookmarking with persistence
- ✅ Command-line file argument
- ✅ Optional CPU/RAM profiling

### v2.0 (Future)
- [ ] Embedding-based similarity detection
- [ ] Export anomaly reports
- [ ] Custom scoring weights configuration
- [ ] Log pattern learning
- [ ] Comparison mode (before/after)
- [ ] Timeline visualization
- [ ] Export bookmarks/annotations

## Profiling

DecentLog includes optional profiling support:

- **CPU Profiling**: Interactive puffin flamegraph viewer (Menu → Profiling → Show CPU Profiler)
- **RAM Profiling**: DHAT heap profiling with detailed allocation tracking

Build with `--features cpu-profiling` `--features ram-profiling` or see [PROFILING.md](PROFILING.md) for details.

## License

MIT

## Contributing

Contributions are welcome! Areas for improvement:
- Additional log format support
- New anomaly detection algorithms
- Performance optimizations
- UI enhancements (keyboard shortcuts, themes, etc.)
- Export formats (JSON, CSV, etc.)
- Documentation and examples

## Author

Built with ❤️ for developers who deserve decent log analysis tools
