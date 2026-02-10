// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2026 Daniel Freiermuth
//
// LogCrab is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// LogCrab is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with LogCrab.  If not, see <https://www.gnu.org/licenses/>.

//! A map that maintains FIFO processing order while allowing updates to coalesce.

use std::collections::{HashMap, VecDeque};

/// A map that maintains FIFO processing order while allowing updates to coalesce.
///
/// This data structure combines a queue (for FIFO ordering) with a map (for O(1) updates).
/// When a key is inserted for the first time, it's added to the back of the queue.
/// When a key is updated, the map value changes but the queue position remains unchanged.
/// This ensures fair processing order regardless of key values.
///
/// # Example
///
/// ```ignore
/// let mut queue_map = QueueMap::new();
/// queue_map.insert(10, "highlight");
/// queue_map.insert(0, "filter");
/// queue_map.insert(0, "filter_updated"); // Coalesces with previous
///
/// // Processes in FIFO order: (10, "highlight") then (0, "filter_updated")
/// assert_eq!(queue_map.pop_front(), Some((10, "highlight")));
/// assert_eq!(queue_map.pop_front(), Some((0, "filter_updated")));
/// ```
pub struct QueueMap<K, V> {
    /// Map for O(1) lookups and updates
    map: HashMap<K, V>,
    /// Queue tracking insertion order of keys
    queue: VecDeque<K>,
}

impl<K: std::hash::Hash + Eq + Copy, V> QueueMap<K, V> {
    /// Create a new empty `QueueMap`.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            queue: VecDeque::new(),
        }
    }

    /// Insert or update a value. Returns true if this is a new key.
    ///
    /// If the key already exists, only the value is updated; the queue position
    /// remains unchanged to preserve FIFO ordering.
    pub fn insert(&mut self, key: K, value: V) -> bool {
        let is_new = !self.map.contains_key(&key);
        if is_new {
            self.queue.push_back(key);
        }
        self.map.insert(key, value);
        is_new
    }

    /// Remove and return the next item in FIFO order.
    ///
    /// Returns `None` if the queue is empty.
    pub fn pop_front(&mut self) -> Option<(K, V)> {
        let key = self.queue.pop_front()?;
        let value = self.map.remove(&key)?;
        Some((key, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_ordering() {
        let mut qm = QueueMap::new();
        qm.insert(10, "second");
        qm.insert(0, "first");

        // Should process in insertion order (10, then 0), not key order
        assert_eq!(qm.pop_front(), Some((10, "second")));
        assert_eq!(qm.pop_front(), Some((0, "first")));
        assert_eq!(qm.pop_front(), None);
    }

    #[test]
    fn test_coalescing() {
        let mut qm = QueueMap::new();
        assert!(qm.insert(5, "first"));
        assert!(qm.insert(10, "other"));
        assert!(!qm.insert(5, "updated")); // Not new

        // Key 5 should still be first in queue, with updated value
        assert_eq!(qm.pop_front(), Some((5, "updated")));
        assert_eq!(qm.pop_front(), Some((10, "other")));
    }

    #[test]
    fn test_rapid_updates() {
        let mut qm = QueueMap::new();
        qm.insert(0, "v1");
        qm.insert(0, "v2");
        qm.insert(0, "v3");

        // Only the last value should be present
        assert_eq!(qm.pop_front(), Some((0, "v3")));
        assert_eq!(qm.pop_front(), None);
    }
}
