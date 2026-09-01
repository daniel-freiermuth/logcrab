# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] - 2026-06-08

### Added

- Option to show MAC addresses in PCAP views.

## [1.1.0] - 2026-06-08

### Added

- SOME/IP message decoding.
- Option to hide duplicate log lines.

### Fixed

- PID display for logcat logs.

## [1.0.0] - 2026-05-28

### Added

- Session history.
- Configurable rendering of rare rows in grey.
- Sidecar scoring integration, including ML score and attention explanation views.
- `logcrab-export` binary for exporting log data.
- LogCrab MCP server.

### Changed

- Migrated the sidecar protocol to gRPC.
- Removed the model-status field from the sidecar protocol.
- Normalized the file-type registry API.

### Fixed

- Restored the option to enable ML scoring.
- Color histogram values by ML score.
- Widened the score column so supplemental information remains visible.

## 0.x release history

Pre-1.0 release notes were reconstructed from version tags and commit history.


## 0.36.2 - 2026-05-06

### Fixed

- introduce global config versioning.
- Prevent global config corruption.

## 0.36.1 - 2026-04-23

### Fixed

- Introduce filetype state versioning.

## 0.36.0 - 2026-04-22

### Added

- Parse kernel logs from bugreport.

## 0.35.3 - 2026-04-21

### Fixed

- Handle invalid chars gracefully for dmesg and logcat files.

## 0.35.2 - 2026-04-15

### Fixed

- Save bookmark annotation when unfocusing edit field.

## 0.35.1 - 2026-04-09

### Fixed

- Guard file drop hover by focus.

## 0.35.0 - 2026-03-26

### Added

- Add support for BT subprotocols: RFCOMM, HFP, AVCTP, AVRCP.

## 0.34.2 - 2026-03-25

### Fixed

- Catch errors while templating log lines.

## 0.34.1 - 2026-03-20

### Fixed

- Always show original timestamps when calibrating time.

## 0.34.0 - 2026-03-18

### Added

- Don't auto-close error and warning toasts.
- Close scoring toasts right after completion.
- Open a file read-only if already opened by another instance (instead of not opening at all).
- Support for OpenTelemetry logs.

### Fixed

- Don't use the existing progress toast, but create a new one for warnings.

## 0.33.0 - 2026-03-17

### Added

- Skip non-utf8 chars in bugreports.

## 0.32.0 - 2026-03-17

### Added

- Disallow overwriting crabfiles of newer version.
- Add support for two more timestamp formats.
- Skip non-utf8 characters in generic files.
- add support for bracketed time.
- Locate logs without year in 1970.
- Also show source in bookmark table.

### Fixed

- Don't stop loading bugreports, dmesg and generic files early when lines are skipped.

## 0.31.0 - 2026-03-16

### Added

- add delta and relative timestamp display modes.

### Fixed

- Don't stop loading files early when lines are skipped.

## 0.30.0 - 2026-03-12

### Added

- Export bookmarked logs into file.

## 0.29.0 - 2026-03-12

### Added

- add support for dmseg logs.

## 0.28.0 - 2026-03-05

### Added

- Show spinner while histogram is calculating.
- Decode selected multicast packets as SOME/IP.

### Fixed

- Cancel zoom-boxes that were released outside the window.
- Don't freeze timeline zoombox when pointer leaves window.
- Search on display_message.

## 0.27.1 - 2026-03-04

### Changed

- Maintenance release.

## 0.27.0 - 2026-03-03

### Added

- Allow calibrating storage-time mode dlt files.
- Make storage timestamp default source for dlt.
- Improve loading speed of text files.
- Use magic bytes instead of file extension to detect type.
- Export result into file.

### Fixed

- PKGBUILD should only build default features.

## 0.26.1 - 2026-02-24

### Fixed

- Stable source ids and monotonic version.
- Drain filter results.
- Align fallback col count to normal case.
- use calibrated timestamps in timeline.
- Remove races and potential deadlocks.
- Catch when calibrating timestamps near the representable boundary.
- Check both direction when finding closest line.

## 0.26.0 - 2026-02-19

### Added

- Add menu to remove files from the session.
- Add about window.
- Calibrate-time window tweaks.
- Option to apply DLT calibration for all apps of given ECU.

### Fixed

- Also draw closest-line hint on bookmarked lines.
- Bookmark annotation edit field focus handling.
- Reload dlt files when time-source is changed.
- Format timediff nicely.
- Show current and storage time when syncing dlt times.
- Keep lock when reloading dlt files.
- Also cancel ongoing scoring threads when reloading dlt files.
- Draw hover even when cursor over label.
- Handle clicks centrally and everywhere on a row.
- clippy.

## 0.25.1 - 2026-02-18

### Fixed

- parsing of btsnoop timestamps.

## 0.25.0 - 2026-02-16

### Added

- Lock on crab file to prevent concurrent editing.
- Support for BT snoop logs.
- add l2cap parsing.

## 0.24.1 - 2026-02-13

### Changed

- Maintenance release.

## 0.24.0 - 2026-02-12

### Added

- Parse TCP packets a bit more detailed.
- Show hint for closest line in filtered view.
- Show line hint even in bookmark view.
- Time-shift all kinds of input data.
- allow removing bookmark annotation.

### Fixed

- Also show 128-bit and raw values.
- Slice UTF8 strings properly in toasts.
- Don't overwrite default shortcuts with partial shortcut configs.
- Process tasks fairly.
- Only request focus once.
- Track the store version consistently through requests.
- cpu-profiling.

## 0.23.0 - 2026-02-10

### Added

- Let filter table stick to bottom.
- add support for +0100 and +01:00 as timezone specifier.
- 1st pcap support.
- significantly improve file load speed.

## 0.22.2 - 2026-02-06

### Fixed

- Don't truncate label in table. Truncation is handled by column.
- Set drag_to_scroll(false) for tables.

## 0.22.1 - 2026-02-05

### Added

- Show open files in window title.

### Fixed

- Enable double/tripple-click selection in table!.
- also enable double/triple-click in bookmark view.

## 0.22.0 - 2026-02-05

### Added

- Finally negative filtering.

## 0.21.1 - 2026-02-05

### Fixed

- Also parse milliseconds in slog2 logs.

## 0.21.0 - 2026-02-04

### Added

- Remove default bookmark annotation. Show hint when annotation empty.
- /ref: Move annotation column to front in bookmark view.

### Fixed

- Draw selection marker also for timeline-ranges  <1s.
- Reduce timeline min drag zoom distance.

## 0.20.0 - 2026-02-03

### Added

- Increase width of message column slightly.
- Zoomable timeline!.
- Remove "Hide Jan 1st" view option, now that the user can zoom the timeline.

## 0.19.0 - 2026-01-30

### Added

- Rename "Sync time" →"Calibrate time".

### Fixed

- Maintain proper sane ordering of lines, even when they are not yet loaded.
- Maintain calibration per App instead of per Context.

## 0.18.0 - 2026-01-29

### Added

- calibrate dlt time per file, ecu, context.

## 0.17.0 - 2026-01-22

### Added

- Timeline markers for bookmarks.

### Fixed

- Also use autogenerated titles for timeline markers.

## 0.16.0 - 2026-01-20

### Added

- Add right-click for context-menu in table. Bookmark-toggle moded to middle-click.
- Allow manual time sync of dlt files.

### Fixed

- Scrub on floating precision instead of second precision.

## 0.15.0 - 2026-01-19

### Fixed

- When scrubbing, jump to nearest point in time, not next-following.

## 0.14.0 - 2026-01-18

### Added

- Allow opening more than one file from command line.

### Fixed

- Proper fix for the empty HistogramData problem.

## 0.13.1 - 2026-01-16

### Changed

- Maintenance release.

## 0.13.0 - 2026-01-04

### Added

- Nice automatic filter tab title.
- Click-to-bookmark also selects.
- Show source file in own column.
- Close button on toasts.
- match more "time out" variants.
- /perf: Histogram worker for snappy UI.

### Fixed

- Keep focus on search text field.
- /feat: Only highlight matches in message column.
- Some timestamp parsing was broken.
- Ignore invalid DLT storage times instead of crash.
- Pending rebind while keyboard settings window closed stayed alive.
- Stop FilterWorker when app exits.

## 0.12.0 - 2025-12-19

### Added

- add more profiling.
- even more profiling.
- Improve jumping performance with large files.
- Sort bookmarks.

### Fixed

- /ref: Only update filter cache once per frame.
- Scroll when visiting a filter the first time and on query change.

## 0.11.0 - 2025-12-18

### Added

- Load multiple files into session.
- Allow many files initially.
- Don't open an already-opened file again.

### Fixed

- Replace unicode char by text.

## 0.10.0 - 2025-12-15

### Added

- Highligths!.
- make timeline selection marker precise.
- Also respond to dragging in timeline making it a scrubber.

### Fixed

- Save crab file when timeline markers are toggled.
- GUI id clash when filter name was same.
- Request filter update for timeline markers.
- auto-grow message column.

## 0.9.0 - 2025-12-15

### Added

- Show fulll date in time column.
- Introduce crab file versioning.
- Import many crab filter files at once.
- Remember file locations.
- Find bugreport year in header.
- Take crab-filter files via drag'n'drop.

### Fixed

- Grow table with pane.
- Interpret logcat time as local.

## 0.8.0 - 2025-12-14

### Added

- Load files progressively.
- Toasts! Yum.

### Fixed

- profiling.

## 0.7.0 - 2025-12-12

### Added

- Histogram markers!.
- parallel filtering.
- Bright mode!.

## 0.6.0 - 2025-12-11

### Added

- Add support for kernel timestamps.
- more precise time parsing.
- Add support for dlt's U64.
- Add support for U8, U16, I32.
- Show progress while loading DLT files.

### Fixed

- Stop editing favorite name when loosing focus.
- Jump-by-click in histogram was off.
- Align case-insensitive behavior with VS Code.

## 0.5.0 - 2025-12-05

### Added

- Introduce match highlight-blending.
- Draw histogram as stacked bars showing anomaly distribution.
- Start with horizontal split.
- Allow hiding Jan 1 in histogram.

### Fixed

- Also draw remaining line after last highlight.
- Don't skew scoring by skipped lines.

## 0.4.0 - 2025-12-04

### Added

- Add toggle to show/hide global highlighting per filter.

## 0.3.0 - 2025-12-04

### Added

- Only show hover  if row clipped.

## 0.2.0 - 2025-12-04

### Added

- Favorites can have names!.
- Highlight filter matches in all tabs.
- Also store color in crab file.
- Import/Export functionality for Crab-Filters.
- Small color indicator in tab title.
- Add desktop file.
- Allow opening .crab files.
- History for regex text field.
- Implement Drag and Drop.

### Fixed

- Skip 0 bytes while file parsing.
- Additional timeformats.

## 0.1.0 - 2025-12-01

### Added

- Initial LogCrab release with anomaly scoring, regex search, bookmarking, and multiple filter views.
- Support for DLT logs, dockable views, keyboard shortcuts, and persisted filters and key bindings.
- Background filtering and anomaly scoring for responsive loading of large log files.
