# Multi-Source Implementation Plan

Incremental steps to implement the multi-source architecture. Each step is a shippable commit that doesn't break existing functionality.

---

## Phase 1: Foundation (Pure Additions)

### Step 1: Add `LineId` struct
- Create new struct with `timestamp`, `source_id`, `line_number`
- Implement `Ord`, `Eq`, `Hash`, `Clone`, `Copy`
- Add constructor and helper methods
- **Zero impact on existing code** — just a new type sitting there

### Step 2: Add `source_id` field to `LogLine`
- Default to `0` for all existing lines
- Update parser to pass `source_id: 0` everywhere
- **Backwards compatible** — everything still works with the single-source assumption

### Step 3: Create `id: LineId` field on `LogLine`
- Construct from existing `timestamp`, hardcoded `source_id: 0`, and `line_number`
- **Backwards compatible** — duplicate data for now, nothing uses `LineId` yet

---

## Phase 2: Data Access Layer

### Step 4: Create `LogStore` wrapper around single source
- Wrap the existing `Vec<LogLine>` in a `LogStore` with one `SourceData`
- Add `version: u64` that's never bumped yet (always `1`)
- Provide `iter_all()` that just iterates the one source
- **Swap in one place** — `LogViewState` uses `LogStore` instead of direct `Vec`

### Step 5: Add `SourceInfo` metadata struct
- Track `path`, `name`, `status` per source
- Single source gets this automatically
- **No behavior change** — just richer metadata

### Step 6: Progressive file loading
- Load file in chunks (e.g., 10k lines at a time)
- After each chunk: call `store.append_lines()`, bump version, yield to UI
- User sees lines appearing progressively
- Same pattern as tailing — just stops at EOF
- **Immediate UX win** for large files

---

## Phase 3: Filter Migration

### Step 7: Change filter matching to use `LineId`
- `FilterState.filtered_indices: Vec<usize>` → `matching_ids: Vec<LineId>`
- All filter logic now returns/stores `LineId` instead of index
- **Key moment**: Access changes from `lines[idx]` to `store.get_line(id)`

### Step 8: Add `LogStore::get_line(LineId) -> &LogLine`
- With single source: `sources[0].lines[id.line_number as usize]`
- O(1) lookup by `LineId`
- This is the **bridge** that makes `LineId`-based access work

### Step 9: Add version-based filter cache invalidation
- `FilteredView.cached_for_version: u64`
- If `store.version != cached_for_version`, recompute
- **Still single source**, but cache invalidation is ready

---

## Phase 4: Bookmarks Migration

### Step 10: Change bookmarks to use `LineId`
- `HashMap<usize, Bookmark>` → `HashMap<LineId, Bookmark>`
- Bookmark lookup by `LineId` instead of line index
- **Shippable**: Bookmarks work exactly as before with single source

---

## Phase 5: Multi-Source (The Payoff)

### Step 11: Extend `LogStore` to hold multiple sources
- `sources: Vec<SourceData>` instead of single source
- `add_source()` method that assigns new `source_id` and bumps version
- Existing single-file open uses this with one source

### Step 12: CLI: Accept multiple file arguments
- Parse `logcrab file1.log file2.log`
- Each file becomes a source with unique `source_id`
- **Merged iteration** now kicks in via `iter_merged()`

### Step 13: Implement `iter_merged()` k-way merge
- Real merge across sources by timestamp
- Replace `iter_all()` usage with `iter_merged()` where needed
- **Everything now supports multiple files**

---

## Phase 6: UI Enhancements

### Step 14: Add source column to log view
- Show filename (or "stdin") in a column
- Color-code by source
- **Nice visual feedback** for multi-source

### Step 15: Add "Open Additional File" menu action
- Opens file dialog → adds to existing `LogStore`
- Bumps version → filters recompute
- **User can now add files dynamically**

---

## Phase 7: Stdin Support

### Step 16: Add `--stdin` CLI flag
- Creates a source with `source_id` and no path
- Reads stdin into memory, parses lines
- Works with the existing multi-source machinery

### Step 17: Mark stdin source as ephemeral for .crab
- Skip saving `.crab` for stdin
- Warn user about ephemeral bookmarks

---

## Phase 8: Tailing

### Step 18: Add `-f/--follow` flag parsing
- Parse the flag, store `SourceOptions { follow: true }`
- No behavior yet — just captures intent

### Step 19: Implement file polling for tailed sources
- `TailingSource` struct with position tracking
- Poll every 500ms, call `store.append_lines()`
- Version bump triggers filter refresh

---

## Phase 9: Per-File Sorting

### Step 20: Add `-s/--sort` flag
- For sources that need post-load sorting
- Sorts by `LineId` after initial load

---

## Phase 10: Persistence

### Step 21: Update `.crab` format to use `LineId`
- Bookmarks stored with `LineId` instead of line index
- Filters still text-based, no change

### Step 22: Implement spread-out save
- Each source gets its own `.crab` file
- Filters duplicated, bookmarks partitioned by `source_id`

### Step 23: Implement merge load
- Load `.crab` files for each source
- Merge filters (dedupe), collect all bookmarks

---

## Quick Start

Recommended first three steps (can be done in an afternoon):

| Step | Time | Risk |
|------|------|------|
| 1. `LineId` struct | 30 min | Zero |
| 2. `source_id: 0` on `LogLine` | 30 min | Trivial |
| 3. `id: LineId` on `LogLine` | 1 hour | None |

After these, the stable identifier is in place and consumers can be migrated incrementally.

---

## Dependency Graph

```
Step 1 ─┬─► Step 2 ─► Step 3 ─┬─► Step 4 ─► Step 5 ─► Step 6
        │                     │
        │                     └─► Step 7 ─► Step 8 ─► Step 9
        │                              │
        │                              └─► Step 10
        │
        └─► Step 11 ─► Step 12 ─► Step 13
                 │
                 ├─► Step 14 ─► Step 15
                 │
                 ├─► Step 16 ─► Step 17
                 │
                 ├─► Step 18 ─► Step 19
                 │
                 ├─► Step 20
                 │
                 └─► Step 21 ─► Step 22 ─► Step 23
```

---

*See ARCHITECTURE_MULTI_SOURCE.md for detailed design rationale.*
