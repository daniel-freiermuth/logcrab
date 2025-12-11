# Optimization Roadmap for Large Files

*Future optimization paths for handling GB-scale log files*

## Scale Reference

| File Size | Lines (approx) | `LineId` storage | Raw text |
|-----------|----------------|------------------|----------|
| 100 MB    | 500K - 1M      | 9 - 18 MB        | 100 MB   |
| 1 GB      | 5M - 10M       | 90 - 180 MB      | 1 GB     |
| 10 GB     | 50M - 100M     | 0.9 - 1.8 GB     | 10 GB    |

---

## Current Design: Bottlenecks at Scale

| Component | Current | Pain at 10M lines |
|-----------|---------|-------------------|
| `LogLine.raw` | `String` per line | 2GB+ heap allocations |
| `FilteredView.matching_ids` | `Vec<LineId>` | 180MB per view |
| Filter recompute | O(n) regex scan | Seconds per filter change |
| `iter_merged()` | K-way merge every call | Repeated work |

---

## Optimization 1: Memory-Mapped File Backing

**Problem**: Loading entire file into RAM.

**Solution**: Memory-map the file, store offsets instead of strings.

```rust
// Before
pub struct LogLine {
    pub id: LineId,
    pub raw: String,              // Heap allocated
    pub message: String,          // Another allocation
    pub score: Option<f64>,
}

// After
pub struct LogLine {
    pub id: LineId,
    pub raw_range: Range<u32>,    // Offset into mmap
    pub message_offset: u16,      // Offset within raw where message starts
    pub score: Option<f64>,
}

impl LogStore {
    pub fn get_raw(&self, source_id: u16, line: &LogLine) -> &str {
        let mmap = &self.sources[source_id].mmap;
        &mmap[line.raw_range.start as usize..line.raw_range.end as usize]
    }
}
```

**Benefit**: 
- File stays on disk, OS pages in on demand
- `LogLine` shrinks from ~100+ bytes to ~32 bytes
- Can handle files larger than RAM

**Compatibility**: ✓ `LogStore` API unchanged, internals swapped.

---

## Optimization 2: Block Indexing

**Problem**: Seeking to a timestamp requires scanning.

**Solution**: Divide file into blocks with timestamp index.

```rust
struct BlockIndex {
    blocks: Vec<Block>,
}

struct Block {
    line_range: Range<u32>,           // Lines in this block
    byte_range: Range<u64>,           // Bytes in file
    timestamp_range: (DateTime, DateTime),
}

impl BlockIndex {
    /// Find blocks that might contain this timestamp range
    fn blocks_for_range(&self, start: DateTime, end: DateTime) -> impl Iterator<Item = &Block> {
        self.blocks.iter().filter(move |b| {
            b.timestamp_range.1 >= start && b.timestamp_range.0 <= end
        })
    }
}
```

**Benefit**:
- Skip entire blocks when filtering by time range
- Fast seeking for "jump to timestamp"
- Enables lazy loading of blocks

**Compatibility**: ✓ Internal optimization, API unchanged.

---

## Optimization 3: Lazy/Windowed Filter Results

**Problem**: Materializing all matches (180MB for 10M lines).

**Solution**: Only compute matches for visible window + buffer.

```rust
// Before: Eager, all in RAM
struct FilteredView {
    matching_ids: Vec<LineId>,       // ALL matches
}

// After: Lazy, windowed
struct FilteredView {
    filter: Regex,
    
    // Only materialize what we need
    total_matches: Option<usize>,    // Lazily computed
    cached_ranges: BTreeMap<usize, Vec<LineId>>,  // position → chunk of IDs
}

impl FilteredView {
    /// Get matches for positions [start, end)
    fn get_window(&mut self, store: &LogStore, start: usize, end: usize) -> &[LineId] {
        // Check cache, compute if missing
    }
    
    /// Total match count (requires full scan once)
    fn len(&mut self, store: &LogStore) -> usize {
        if self.total_matches.is_none() {
            self.total_matches = Some(self.count_matches(store));
        }
        self.total_matches.unwrap()
    }
}
```

**Benefit**:
- Initial display is instant (only compute visible rows)
- Memory bounded by window size, not file size

**Compatibility**: ✓ API change minimal (len becomes method with store param).

---

## Optimization 4: Parallel Filtering

**Problem**: Single-threaded regex over 10M lines is slow.

**Solution**: Parallelize with rayon.

```rust
use rayon::prelude::*;

fn recompute_parallel(&mut self, store: &LogStore) {
    // Filter each source in parallel
    let mut results: Vec<Vec<LineId>> = store.sources
        .par_iter()
        .map(|source| {
            source.lines
                .iter()
                .filter(|line| self.matches(line))
                .map(|line| line.id)
                .collect()
        })
        .collect();
    
    // Merge results (already sorted within each source)
    self.matching_ids = k_way_merge(results);
}
```

**Benefit**:
- Near-linear speedup with cores
- Sources are natural parallelism boundary

**Compatibility**: ✓ Internal change only.

---

## Optimization 5: Incremental Filter Updates

**Problem**: Any change triggers full recompute.

**Solution**: Track what's already filtered, only process new lines.

```rust
struct FilteredView {
    matching_ids: Vec<LineId>,
    
    // Track progress per source
    last_processed: HashMap<u16, u32>,  // source_id → last line_number processed
    last_filter_hash: u64,              // Detect filter changes
}

impl FilteredView {
    fn update(&mut self, store: &LogStore) {
        let current_hash = hash(&self.filter);
        
        if current_hash != self.last_filter_hash {
            // Filter changed → full recompute
            self.full_recompute(store);
            self.last_filter_hash = current_hash;
        } else {
            // Filter same → incremental
            self.incremental_update(store);
        }
    }
    
    fn incremental_update(&mut self, store: &LogStore) {
        for source in &store.sources {
            let last = *self.last_processed.get(&source.info.id).unwrap_or(&0);
            let new_lines = &source.lines[last as usize..];
            
            for line in new_lines {
                if self.matches(line) {
                    // For tailing: usually appends at end
                    self.matching_ids.push(line.id);
                }
            }
            
            self.last_processed.insert(source.info.id, source.lines.len() as u32);
        }
        
        // Re-sort if timestamps aren't monotonic
        // (or use insertion sort if mostly sorted)
    }
}
```

**Benefit**:
- Tailing is O(new lines) not O(all lines)
- Filter typing remains responsive

**Compatibility**: ✓ Internal change only.

---

## Optimization 6: Compressed In-Memory Storage

**Problem**: 10GB file doesn't fit in RAM even with mmap.

**Solution**: Compress blocks, decompress on demand.

```rust
struct CompressedSource {
    blocks: Vec<CompressedBlock>,
    index: BlockIndex,
}

struct CompressedBlock {
    compressed_data: Vec<u8>,        // zstd compressed
    line_count: u32,
}

impl CompressedSource {
    fn decompress_block(&self, block_idx: usize) -> Vec<LogLine> {
        let block = &self.blocks[block_idx];
        let data = zstd::decompress(&block.compressed_data);
        parse_lines(&data)
    }
}
```

**Benefit**:
- 5-10x compression ratio on log text
- 10GB file → 1-2GB in memory

**Compatibility**: ✓ Behind `LogStore` abstraction.

---

## Optimization 7: Cursor-Based Timestamp Navigation

**Problem**: Even lazy windowed filtering requires O(n) scan to skip to position N.

**Solution**: Since there's no global line index (user navigates by timestamp), use binary search + bidirectional cursors.

**Key insight**: User jumps to a timestamp, not a line number. All sources are sorted. We can binary search each source to find the starting point, then iterate outward.

```rust
struct FilteredView {
    filter: Option<Regex>,
    
    // Cursor position per source
    cursors: Vec<SourceCursor>,
    
    // Visible window (bidirectional from anchor)
    backward_buffer: VecDeque<LineId>,  // Lines before anchor
    forward_buffer: VecDeque<LineId>,   // Lines after anchor
    anchor_timestamp: DateTime<Local>,
}

struct SourceCursor {
    source_id: u16,
    forward_idx: usize,   // Next line to check going forward
    backward_idx: usize,  // Next line to check going backward
}

impl FilteredView {
    fn jump_to_timestamp(&mut self, store: &LogStore, ts: DateTime<Local>) {
        self.anchor_timestamp = ts;
        self.backward_buffer.clear();
        self.forward_buffer.clear();
        
        // Binary search each source — O(k log n) total
        for (i, source) in store.sources.iter().enumerate() {
            let pos = source.lines.partition_point(|l| l.id.timestamp < ts);
            self.cursors[i] = SourceCursor {
                source_id: i as u16,
                forward_idx: pos,
                backward_idx: pos.saturating_sub(1),
            };
        }
    }
    
    fn fill_visible(&mut self, store: &LogStore, lines_needed: usize) {
        let half = lines_needed / 2;
        
        // Fill backward (k-way merge in reverse)
        while self.backward_buffer.len() < half {
            if let Some(line_id) = self.next_backward(store) {
                if self.matches(store.get(line_id)) {
                    self.backward_buffer.push_front(line_id);
                }
            } else {
                break;
            }
        }
        
        // Fill forward (k-way merge)
        while self.forward_buffer.len() < half {
            if let Some(line_id) = self.next_forward(store) {
                if self.matches(store.get(line_id)) {
                    self.forward_buffer.push_back(line_id);
                }
            } else {
                break;
            }
        }
    }
    
    fn next_forward(&mut self, store: &LogStore) -> Option<LineId> {
        // K-way merge: pick cursor with smallest next timestamp
    }
    
    fn next_backward(&mut self, store: &LogStore) -> Option<LineId> {
        // Reverse k-way merge: pick cursor with largest prev timestamp
    }
}
```

**Complexity**:

| Operation | Before (Eager) | After (Cursor) |
|-----------|----------------|----------------|
| Jump to timestamp | O(n) scan | O(k log n) binary search |
| Render 50 lines | O(1) lookup | O(50 × log k) merge |
| Scroll | O(1) | O(delta × log k) |
| Memory | O(matches) | O(visible) |

**Scrollbar**: Shows timestamp range instead of line count. Position is based on current timestamp relative to min/max timestamps.

**Benefit**:
- Opening a 10GB file and jumping to the middle is instant
- Memory usage is O(visible), not O(file size)
- Filter changes only recompute visible window

**Compatibility**: ✓ Internal change. Callers still get `LineId`s for visible lines.

---

## Design Principles (Locked In)

These choices in the current design enable future optimization:

| Principle | Why It Matters |
|-----------|----------------|
| `LineId` is small (18 bytes) and `Copy` | Can have millions without heap pressure |
| Sources stored separately | Natural parallelism, partial loading, cursor per source |
| All access through `LogStore` | Can swap internals freely |
| Filter results are `LineId`, not `LogLine` | Memory efficient, cache-friendly |
| Version-based invalidation | Simple, enables incremental updates |
| Navigation by timestamp, not line number | Enables cursor-based O(log n) jumping |
| Sources sorted internally | Enables binary search + k-way merge |

---

## Priority Order

When to implement each optimization:

| Trigger | Optimization |
|---------|--------------|
| Files > 500MB feel slow | Parallel filtering |
| Files > 1GB won't open | Memory-mapped backing |
| Tailing feels laggy | Incremental filter updates |
| Jump-to-timestamp is slow | Cursor-based navigation |
| Scrollbar jumping is slow | Block indexing |
| Files > 10GB needed | Compressed storage |
| Many filter tabs open | Lazy/windowed results |

---

## What NOT to Do

- Don't optimize prematurely — measure first
- Don't add complexity until the simple version is too slow
- Don't break the `LogStore` abstraction — all optimizations go behind it


## What more
- Parse timestamps parallel (not needed if file loaded icrementally?)
- Filter parallel (map_par)
- 

---

*This document captures optimization paths. Implementation should be driven by measured bottlenecks, not speculation.*

