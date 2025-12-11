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
| Stdin activation | CLI flag |

---

## Open Questions (Deferred)

### Session Persistence

Current: Single file → `filename.crab` sidecar

Problem: With multiple sources, where do bookmarks/filters go?

Options discussed:
- **Option A**: Separate session file concept
- **Option B**: Always session-based
- **Option C**: Implicit session, explicit "Save Session As..."
- **Option D**: First file wins (`first.log.crab` stores everything)

**Decision**: Deferred. Need to think through UX more carefully.

### Bookmarks

- Need to change from `line_index: usize` to `LineId`
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
