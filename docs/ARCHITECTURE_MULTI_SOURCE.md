# Multi-Source Architecture Design

*Design discussion from December 2025*

## Overview

This document captures the architectural decisions for supporting:
- Progressive file display (show lines as they're parsed)
- Multiple log files opened simultaneously
- Stdin as a log source
- Timestamp-based merging across all sources

## Core Design Principles

1. **Boring and defensive**: Make invalid states unrepresentable, make the obvious code the correct code
2. **Indices are ephemeral, IDs are eternal**: Never store indices long-term; use stable identifiers
3. **Version numbers tell you when to recompute**: Simple invalidation model
4. **There is no "all lines" list**: Every view is a filtered view; "show all" is just an empty filter

---

## Data Structures

### LineId — Stable, Unique, Sortable

```rust
#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct LineId {
    pub timestamp: DateTime<Local>,  // Primary sort key
    pub source_id: u16,              // Tie-breaker 1
    pub line_number: u32,            // Tie-breaker 2 (original line in source)
}
```

**Properties**:
- Unique: No two lines can have the same `(timestamp, source_id, line_number)`
- Sortable: `Ord` gives timestamp-based ordering with stable tie-breaking
- Direct access: `(source_id, line_number)` is a direct index into source storage → O(1)

### LogLine

```rust
pub struct LogLine {
    pub id: LineId,
    pub raw: String,
    pub message: String,
    pub score: Option<f64>,          // Anomaly score, filled progressively
    pub template_key: String,
}
```

**Note**: `timestamp` and `line_number` are now inside `id`, not separate fields.

### SourceData

```rust
pub struct SourceData {
    pub info: SourceInfo,
    pub lines: Vec<LogLine>,         // Sorted by timestamp within source
}

pub struct SourceInfo {
    pub id: u16,
    pub name: String,                // Filename or "stdin"
    pub path: Option<PathBuf>,       // None for stdin
    pub color: Color32,              // For UI distinction
    pub status: SourceStatus,
}

pub enum SourceStatus {
    Loading { progress: f32 },
    Done,
    Streaming,                       // Live stdin
    Tailing,                         // File being watched for growth (-f/--follow)
    Error(String),
}
```

### LogStore — Single Source of Truth

```rust
pub struct LogStore {
    sources: Vec<SourceData>,
    version: u64,                    // Bumped on ANY change
}

impl LogStore {
    /// O(1) — direct access by LineId
    pub fn get(&self, id: LineId) -> &LogLine {
        &self.sources[id.source_id as usize].lines[id.line_number as usize]
    }
    
    /// Iterate all lines in timestamp order (k-way merge)
    pub fn iter_merged(&self) -> impl Iterator<Item = &LogLine> {
        // K-way merge iterator over sources
    }
    
    pub fn version(&self) -> u64 { self.version }
    
    pub fn total_lines(&self) -> usize { 
        self.sources.iter().map(|s| s.lines.len()).sum() 
    }
}
```

### FilteredView — Every Tab is a Filtered View

```rust
pub struct FilteredView {
    filter: Option<Regex>,
    matching_ids: Vec<LineId>,       // Sorted by LineId (timestamp order)
    cached_for_version: u64,
}

impl FilteredView {
    pub fn refresh_if_needed(&mut self, store: &LogStore) {
        if self.cached_for_version == store.version() {
            return;
        }
        
        self.matching_ids.clear();
        
        // Single pass: merge + filter
        for line in store.iter_merged() {
            if self.matches(line) {
                self.matching_ids.push(line.id);
            }
        }
        
        self.cached_for_version = store.version();
    }
    
    /// Access by position in this filtered view
    pub fn get(&self, index: usize, store: &LogStore) -> Option<&LogLine> {
        self.matching_ids.get(index).map(|id| store.get(*id))
    }
    
    pub fn len(&self) -> usize { self.matching_ids.len() }
    
    /// Find position of a LineId in this view (for anchoring)
    pub fn find_by_id(&self, id: LineId) -> Option<usize> {
        self.matching_ids.binary_search(&id).ok()
    }
}
```

---

## Key Behaviors

### Adding New Lines (from any source)

1. Parse line, extract timestamp
2. If no timestamp: log warning, skip line
3. Append to appropriate source's `lines` vector
4. Bump `LogStore.version`
5. Views will recompute lazily on next access

### View Anchoring (scroll position)

- Anchor is stored as `Option<LineId>`
- On data change: find anchor in new `matching_ids` via binary search
- If exact match gone (line filtered out): snap to nearest timestamp

```rust
pub struct ScrollState {
    anchor: Option<LineId>,
}

impl ScrollState {
    pub fn get_position(&self, view: &FilteredView) -> usize {
        match self.anchor {
            None => 0,
            Some(id) => {
                // Binary search; if not found, gives insertion point (nearest)
                match view.matching_ids.binary_search(&id) {
                    Ok(idx) => idx,
                    Err(idx) => idx.min(view.len().saturating_sub(1)),
                }
            }
        }
    }
    
    pub fn set_anchor(&mut self, view: &FilteredView, position: usize) {
        self.anchor = view.matching_ids.get(position).copied();
    }
}
```

### Filter Changes

1. User types new regex
2. `FilteredView.filter` updated
3. Mark `cached_for_version = 0` (force recompute)
4. On next frame: `refresh_if_needed()` iterates all sources, merges, filters
5. Anchor snaps to nearest matching line

### Progressive Scoring

- Scores stored in `LogLine.score: Option<f64>`
- Background thread iterates lines, computes scores, updates in place
- No version bump needed (scores don't affect filtering or ordering)
- UI just re-reads scores on repaint

---

## Complexity Analysis

| Operation | Complexity |
|-----------|------------|
| Get line by `LineId` | O(1) |
| Get line by position in view | O(1) |
| Find position of `LineId` in view | O(log n) |
| Find by timestamp | O(log n) |
| Append line to source | O(1) |
| Filter recompute | O(n) |
| Merge iteration | O(n log k) where k = number of sources |

---

## Settled Design Decisions

| Concern | Decision |
|---------|----------|
| View anchoring | By `LineId` (timestamp-based), snap to nearest on change |
| Tie-breaking for same timestamp | source_id, then line_number (stable, boring) |
| Timezones | Store as-is for now; plan for per-source offset later |
| Missing timestamps | Skip line, log warning |
| Stdin EOF | Stdin source closes, session stays interactive, can still add files |
| Memory model | RAM-only for now |
| Filters | Regex only, stateless, always incremental-capable |
| Source distinction in UI | Column showing source (color-coded) |
| Stdin activation | CLI flag `--stdin` |
| File tailing | CLI flag `-f`/`--follow` before filename |

---

## Tailing / Growing Files

Support for files that grow over time (like `tail -f`).

### Activation

```bash
# Follow a single file
logcrab -f app.log
logcrab --follow app.log

# Follow multiple files
logcrab -f app.log -f system.log

# Mix: follow one, static other
logcrab -f app.log system.log.old
```

The `-f`/`--follow` flag applies to the filename immediately following it.

### Implementation

```rust
struct TailingSource {
    path: PathBuf,
    file: File,
    last_position: u64,        // Where we stopped reading
    last_size: u64,            // Last known file size
    source_id: u16,
    last_line_number: u32,     // For continuing LineId sequence
}

impl TailingSource {
    fn check_for_new_lines(&mut self) -> Vec<LogLine> {
        let metadata = self.path.metadata().ok()?;
        let current_size = metadata.len();
        
        if current_size <= self.last_size {
            return vec![];  // No growth (or file truncated)
        }
        
        // Seek to where we left off
        self.file.seek(SeekFrom::Start(self.last_position));
        
        // Read new content
        let mut new_content = String::new();
        self.file.read_to_string(&mut new_content);
        
        // Parse new lines (continuing line number sequence)
        let new_lines = parse_lines(
            &new_content, 
            self.source_id, 
            self.last_line_number
        );
        
        self.last_line_number += new_lines.len() as u32;
        self.last_position = current_size;
        self.last_size = current_size;
        
        new_lines
    }
}
```

### Polling

- Check for file growth every 500ms (configurable later)
- Simple and cross-platform (no inotify/FSEvents dependency)
- Can optimize to file system notifications later if needed

### Truncation Detection

If `new_size < last_size`, the file was truncated (log rotation):
- Warn user: "File was truncated, reloading..."
- Option: Reload from start, or just continue from new content

### Integration

```rust
// In background thread or main loop
fn poll_tailing_sources(store: &mut LogStore, tailing: &mut [TailingSource]) {
    for source in tailing {
        let new_lines = source.check_for_new_lines();
        if !new_lines.is_empty() {
            store.append_lines(source.source_id, new_lines);
            // version bumped → views will refresh
        }
    }
}
```

### Persistence

- Tailed files have `.crab` files like normal files
- Saved on close (same as static files)
- On reopen without `-f`: loads as static file at that point in time

---

## Session Persistence — Spread Out Model

### Core Principle

Every log file has its own `.crab` sidecar file. No separate "session file" concept.

```
app.log       → app.log.crab       (bookmarks for app.log + filters)
system.log    → system.log.crab    (bookmarks for system.log + filters)
```

### What Goes Where

| Data | Storage |
|------|---------|
| Bookmarks | In the `.crab` file of the source they belong to (determined by `LineId.source_id`) |
| Filters | **Duplicated** to all `.crab` files (they're small, and this enables portability) |

### .crab File Format

```rust
struct CrabFile {
    version: u32,
    bookmarks: Vec<Bookmark>,        // Only bookmarks for this file's lines
    filters: Vec<SavedFilter>,       // All filters (duplicated across files)
}

struct Bookmark {
    line_id: LineId,                 // Stable identifier
    name: String,
    // ... other bookmark metadata
}
```

### On Save (Multi-File Session)

```rust
fn save_session(store: &LogStore, bookmarks: &[Bookmark], filters: &[SavedFilter]) {
    for source in &store.sources {
        // Skip stdin (no persistence)
        let Some(path) = &source.info.path else { continue };
        
        let crab_path = path.with_extension("crab");
        let crab = CrabFile {
            version: 1,
            // Only bookmarks belonging to this source
            bookmarks: bookmarks
                .iter()
                .filter(|b| b.line_id.source_id == source.info.id)
                .cloned()
                .collect(),
            // All filters (duplicated)
            filters: filters.clone(),
        };
        
        save(&crab_path, &crab);
    }
}
```

### On Load (Multi-File Session)

```rust
fn load_session(sources: &[SourceInfo]) -> (Vec<Bookmark>, Vec<SavedFilter>) {
    let mut all_bookmarks = vec![];
    let mut all_filters = vec![];
    let mut seen_filters: HashSet<FilterKey> = HashSet::new();
    
    for source in sources {
        let Some(path) = &source.path else { continue };
        let crab_path = path.with_extension("crab");
        
        let Ok(crab) = load(&crab_path) else { continue };
        
        // Collect all bookmarks
        all_bookmarks.extend(crab.bookmarks);
        
        // Merge filters (dedupe by content)
        for filter in crab.filters {
            let key = (&filter.search_text, filter.case_sensitive);
            if !seen_filters.contains(&key) {
                seen_filters.insert(key);
                all_filters.push(filter);
            }
        }
    }
    
    (all_bookmarks, all_filters)
}
```

### Filter Merge Behavior

Filters are merged from all `.crab` files, deduplicated by content:

```rust
fn filter_key(f: &SavedFilter) -> impl Hash + Eq {
    (&f.search_text, f.case_sensitive)
}
```

**Divergence scenario**:
```
Day 1: Open app.log + system.log, create filters [A, B, C], close
       → app.log.crab has [A, B, C]
       → system.log.crab has [A, B, C]

Day 2: Open just app.log, add filter D, close
       → app.log.crab has [A, B, C, D]
       → system.log.crab still has [A, B, C]

Day 3: Open app.log + system.log
       → Load & merge: [A, B, C, D]
       → On close: both .crab files now have [A, B, C, D]
```

Filters naturally propagate and converge over time. No sync logic needed.

### Handover to Colleagues

**Simple rule**: Keep `.crab` files next to log files. Share the whole folder.

```
# Zip and send:
logs/
  app.log
  app.log.crab
  system.log
  system.log.crab

# Colleague unzips, opens the log files → everything works
```

Each file is self-contained. Colleague can open just one file and still get the filters.

### Stdin Handling

- Stdin has no path → no `.crab` file
- Bookmarks on stdin lines are **ephemeral** (lost on close)
- Filters are still saved to other sources' `.crab` files
- Warning to user: "Bookmarks on stdin will not be saved"

### Bookmarks

- Changed from `line_index: usize` to `LineId`
- Always saved to the `.crab` file matching `LineId.source_id`
- Stdin bookmarks are ephemeral (can't survive session reload)

---

## Migration Path from Current Code

### Changes to `LogLine`
- Add `id: LineId` field
- Move `timestamp` and `line_number` into `LineId`
- Add `score: Option<f64>` (currently stored separately)

### Changes to `LogViewState`
- Replace `lines: Arc<Vec<LogLine>>` with `store: LogStore`
- Remove `scores: Option<Vec<f64>>` (now in LogLine)
- Change `bookmarks: HashMap<usize, Bookmark>` to `HashMap<LineId, Bookmark>`

### Changes to `FilterState`
- Replace `filtered_indices: Vec<usize>` with `matching_ids: Vec<LineId>`
- Add `cached_for_version: u64`
- Change `find_closest_timestamp_index` to use `LineId` binary search

### New Code
- `LogStore` struct with k-way merge iterator
- `SourceData` and `SourceInfo` structs
- Source loading abstraction (file vs stdin)

---

## Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                          LogStore                                │
│  version: u64                                                    │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐              │
│  │  Source 0   │  │  Source 1   │  │  Source 2   │              │
│  │  (file A)   │  │  (file B)   │  │  (stdin)    │              │
│  │ Vec<LogLine>│  │ Vec<LogLine>│  │ Vec<LogLine>│              │
│  └─────────────┘  └─────────────┘  └─────────────┘              │
│         │                │                │                      │
│         └────────────────┼────────────────┘                      │
│                          ▼                                       │
│                   iter_merged()                                  │
│               (k-way merge by timestamp)                         │
└─────────────────────────────────────────────────────────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐
   │FilteredView │  │FilteredView │  │FilteredView │
   │ filter: /.*/│  │filter: /ERR/│  │filter: /foo/│
   │matching_ids │  │matching_ids │  │matching_ids │
   │  (sorted)   │  │  (sorted)   │  │  (sorted)   │
   └─────────────┘  └─────────────┘  └─────────────┘
         │                │                │
         ▼                ▼                ▼
       Tab 1           Tab 2            Tab 3
      (all lines)    (errors only)   (foo matches)
```

---

*This document reflects the design discussion state. Implementation may reveal additional considerations.*
