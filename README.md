# LogCrab 🦀

An intelligent log anomaly explorer built with Rust and egui. LogCrab helps developers and system administrators debug by automatically highlighting unusual, novel, or interesting log lines in large log files.

<img width="1920" height="1016" alt="image" src="https://github.com/user-attachments/assets/ed0bec6e-243f-491b-8361-4c6f25d63249" />

## Features

- **Visual Anomaly Detection**: Color-coded visualization
- **Live Regex Search**: Real-time filter with regex support and match highlighting
- **Bookmarks**: Right-click to bookmark important lines
- **Multi-Panel View**: Multiple filters show the logs from different perspectives for better understanding
- **Multi-Format Support**: Supports Android logcat, DLT files and generic log formats
- **No Training Required**: Works immediately on any log file

## Getting started

```bash
// Install rustup with your system package manager
$ rustup toolchain install stable // Install cargo
$ cd logcrab
$ env -u WAYLAND_DISPLAY cargo run --release // as of now, it is recommended to run via Xwayland due to known limitations
```

### Desktop Integration

To add LogCrab to your application menu:

```bash
# Build the release binary
cargo build --release

# Update the .desktop file with the correct path to the binary
sed "s|<logcrab-binary>|$(pwd)/target/release/logcrab|g" logcrab.desktop > ~/.local/share/applications/logcrab.desktop

# Install the icon
mkdir -p ~/.local/share/icons/hicolor/256x256/apps
cp logo.png ~/.local/share/icons/hicolor/256x256/apps/logcrab.png

# Register .crab file type
mkdir -p ~/.local/share/mime/packages
cp logcrab-mime.xml ~/.local/share/mime/packages/
update-mime-database ~/.local/share/mime
```

After installation, LogCrab will appear in your application launcher and can open log files and `.crab` files directly.

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

## Known bugs

### Drag and Drop only works when using Xwayland
Due to this not yet being implemented.
https://github.com/rust-windowing/winit/issues/1881

### Wayland protocol error
When using the Wayland compositor, you might find the program exited after leaving the computer alone for a while.
Particularly when there was a change in the display setup like screenlocking or suspending.
```
[2025-11-28T11:48:43.736Z INFO  tracing::span] read_socket;
wl_registry@2: error 0: invalid global wl_output (151)
wl_registry@2: error 0: invalid global wl_output (151)
Protocol error 0 on object wl_registry@2: 
Error: WinitEventLoop(ExitFailure(1))
```
The problem seems to be a Wayland protocol error by some party upon which the GUI library exits.

## Author

Built with ❤️ for developers who deserve decent log analysis tools
