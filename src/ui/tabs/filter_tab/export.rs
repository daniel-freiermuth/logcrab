use crate::core::log_store::LogStore;
use crate::parser::logline_types::LogLineCore;
use crate::ui::tabs::filter_tab::filter_state::FilterState;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

/// Export filtered results to a file (timestamp and message columns)
pub fn export_filtered_results(
    filter: &FilterState,
    store: &LogStore,
    path: &Path,
) -> Result<(), String> {
    let filtered_indices = filter.search.get_filtered_indices_cached();
    let file = File::create(path).map_err(|e| format!("Failed to create file: {e}"))?;
    let mut writer = BufWriter::new(file);

    for id in filtered_indices.iter() {
        if let Some(line) = store.get_by_id(id) {
            // Use calibrated timestamp for export
            let ts = store.get_adjusted_timestamp(id, &line).to_rfc3339();
            let msg = line.message();
            writeln!(writer, "{ts}\t{msg}").map_err(|e| format!("Write error: {e}"))?;
        }
    }
    Ok(())
}
