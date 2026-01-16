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

//! Filter data types and computation logic.
//!
//! This module provides the data structures for regex-based filtering
//! with async computation support via `AsyncCache`.

use crate::core::async_cache::AsyncCache;
use crate::core::log_store::StoreID;
use crate::core::LogStore;
use fancy_regex::Regex;
use std::sync::Arc;

/// The result of a filter computation: a list of matching line IDs.
#[derive(Clone, Debug, Default)]
pub struct FilterData {
    pub filtered_indices: Vec<StoreID>,
}

impl FilterData {
    /// Compute filtered indices by matching a regex against all lines in the store.
    pub fn compute(store: &Arc<LogStore>, regex: &Regex) -> Self {
        profiling::scope!("FilterData::compute");

        let filtered_indices = store.get_matching_ids(|line| {
            regex.is_match(&line.message).unwrap_or(false)
                || regex.is_match(&line.raw).unwrap_or(false)
        });

        Self { filtered_indices }
    }
}

/// Cache key for filter validity.
///
/// A filter cache is valid when:
/// - The search text matches
/// - The case sensitivity setting matches
/// - The store version hasn't changed
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterKey {
    pub search_text: String,
    pub case_sensitive: bool,
    pub store_version: u64,
}

impl FilterKey {
    pub fn new(search_text: String, case_sensitive: bool, store_version: u64) -> Self {
        Self {
            search_text,
            case_sensitive,
            store_version,
        }
    }
}

/// Type alias for the filter cache.
pub type FilterCache = AsyncCache<usize, FilterKey, FilterData>;
