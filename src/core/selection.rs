// LogCrab - GPL-3.0-or-later
// This file is part of LogCrab.
//
// Copyright (C) 2025 Daniel Freiermuth
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

use chrono::{DateTime, Local};
use crate::parser::line::LogLine;

/// Tracks the currently selected log line across different views
#[derive(Debug, Clone)]
pub struct Selection {
    line_index: Option<usize>,
    timestamp: Option<DateTime<Local>>,
}

impl Selection {
    pub fn new() -> Self {
        Selection {
            line_index: None,
            timestamp: None,
        }
    }
    
    /// Select a specific line
    pub fn select(&mut self, line_index: usize, timestamp: Option<DateTime<Local>>) {
        self.line_index = Some(line_index);
        self.timestamp = timestamp;
    }
    
    /// Clear selection
    pub fn clear(&mut self) {
        self.line_index = None;
        self.timestamp = None;
    }
    
    /// Get the selected line index
    pub fn line_index(&self) -> Option<usize> {
        self.line_index
    }
    
    /// Get the selected timestamp
    pub fn timestamp(&self) -> Option<DateTime<Local>> {
        self.timestamp
    }
    
    /// Find the position of the selected line in a filtered list
    /// Returns the index within the filtered_indices array
    pub fn find_in_filtered(&self, filtered_indices: &[usize]) -> Option<usize> {
        if let Some(selected_idx) = self.line_index {
            filtered_indices.iter().position(|&idx| idx == selected_idx)
        } else {
            None
        }
    }
    
    /// Find the closest line by timestamp in a filtered list
    pub fn find_closest_by_timestamp(
        &self,
        lines: &[LogLine],
        filtered_indices: &[usize],
    ) -> Option<usize> {
        if filtered_indices.is_empty() {
            return None;
        }
        
        let target_ts = self.timestamp?;
        
        let mut closest_idx = 0;
        let mut min_diff = i64::MAX;
        
        for (filtered_idx, &line_idx) in filtered_indices.iter().enumerate() {
            if let Some(line_ts) = lines[line_idx].timestamp {
                let diff = (line_ts.timestamp() - target_ts.timestamp()).abs();
                if diff < min_diff {
                    min_diff = diff;
                    closest_idx = filtered_idx;
                }
            }
        }
        
        Some(closest_idx)
    }
}

impl Default for Selection {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    
    #[test]
    fn test_selection_basic() {
        let mut sel = Selection::new();
        assert_eq!(sel.line_index(), None);
        
        let ts = Local.with_ymd_and_hms(2025, 11, 22, 10, 30, 0).unwrap();
        sel.select(42, Some(ts));
        
        assert_eq!(sel.line_index(), Some(42));
        assert_eq!(sel.timestamp(), Some(ts));
        
        sel.clear();
        assert_eq!(sel.line_index(), None);
    }
    
    #[test]
    fn test_find_in_filtered() {
        let mut sel = Selection::new();
        sel.select(5, None);
        
        let filtered = vec![1, 3, 5, 7, 9];
        assert_eq!(sel.find_in_filtered(&filtered), Some(2));
        
        let filtered2 = vec![1, 3, 7, 9];
        assert_eq!(sel.find_in_filtered(&filtered2), None);
    }
}
