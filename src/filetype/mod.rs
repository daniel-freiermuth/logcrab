use egui::Ui;

pub mod calibration_window;
pub mod registry_macro;
pub mod generic;
pub mod logcat;
pub mod bugreport;
pub mod dlt;
pub mod pcap;
pub mod btsnoop;

pub use calibration_window::CalibrationWindow;

// ============================================================================
// CalibrationState — typed alias used by every FileState
// ============================================================================

/// Per-source calibration in progress: the line's raw timestamp (needed to compute the
/// final offset on confirm) paired with the open [`CalibrationWindow`].
///
/// Stored in `FileState` as `#[serde(skip)]`. Created by `egui_render_context_menu`;
/// driven each frame by `LogFileState::egui_render_file_state`.
pub type CalibrationState = (chrono::DateTime<chrono::Local>, CalibrationWindow);

/// Shared helper — drives an in-progress calibration window and returns the
/// offset to apply on confirm, or `None` if still open / cancelled.
///
/// Clears `calibration` on both confirm and cancel.
pub fn render_calibration(
    ui: &egui::Ui,
    calibration: &mut Option<CalibrationState>,
) -> Option<i64> {
    let (raw_time, result) = if let Some((raw, window)) = calibration.as_mut() {
        (*raw, window.render(ui))
    } else {
        return None;
    };
    match result {
        Ok(Some((target_time, _apply_to_all))) => {
            *calibration = None;
            Some(target_time.timestamp_millis() - raw_time.timestamp_millis())
        }
        Ok(None) => None,
        Err(()) => {
            *calibration = None;
            None
        }
    }
}

/// Trait that every per-source `FileState` type must implement.
///
/// Provides a frame-driven hook for UI state that lives inside the `FileState`
/// (e.g. an open calibration window).  The default implementation is a no-op so
/// that simple types like `()` satisfy the bound without any boilerplate.
pub trait LogFileState {
    /// Drive any open UI window stored in this state.
    ///
    /// Called once per source per frame from `SourceData::render_file_state`.
    /// Returns `true` when the user confirms a new calibration time (the offset has
    /// already been written into `self`); the caller then bumps the source version.
    /// Default: no-op returning `false`.
    fn egui_render_file_state(&mut self, _ui: &egui::Ui) -> bool {
        false
    }
}

/// Blanket impl so that `()` (used as `FileState` by the legacy Mixed source)
/// satisfies the `LogFileState` bound without any behaviour.
impl LogFileState for () {}

// ============================================================================
// EguiConfig — trait for config types that can render their own settings UI
// ============================================================================

/// Implemented by every `LineType::Config` type to render format-specific settings UI.
///
/// The default impl is a no-op so that config types without user-visible settings
/// (i.e. `()`) satisfy the bound automatically.
pub trait EguiConfig {
    /// Render the settings UI for this config value.
    ///
    /// Returns `true` if the value was mutated. The caller is responsible for
    /// rebuilding any derived state (e.g. timestamp sort indices) on `true`.
    fn egui_render(&mut self, _ui: &mut Ui) -> bool {
        false
    }
}

/// Blanket impl for `()` — no settings to show.
impl EguiConfig for () {}

/// Filetype trait infrastructure for logcrab
pub trait LineType: std::fmt::Debug + Send + Sync {
    /// Per-type global user-controlled settings shared across all sources of this type
    /// (e.g. DLT timestamp source). Shared via `Arc<RwLock<T::Config>>` — a single
    /// instance is held per session and mutating it affects all open files of this type live.
    type Config: std::fmt::Debug + Default + Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + EguiConfig;

    /// Per-source persistent state and transient UI state for this file
    /// (e.g. time offset, open calibration window). Persisted to the `.crab` file.
    /// Transient fields (open windows etc.) must be annotated `#[serde(skip)]`.
    type FileState: LogFileState + std::fmt::Debug + Default + Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de>;

    /// Migrate a v2 `.crab` file's `time_offset_ms` field into a `FileState`.
    ///
    /// Called when loading a `.crab` file with `version < 3`. All types must implement
    /// this explicitly — there is no default — to ensure no calibration data is silently lost.
    fn file_state_from_v2(time_offset_ms: i64) -> Self::FileState;

    /// Get the calibrated timestamp of this log line.
    ///
    /// Must apply `file_state.time_offset_ms` (if the `FileState` type carries one)
    /// on top of the source-selected base timestamp. Used for display, sort, and
    /// filter operations. Call `raw_timestamp()` instead when computing a new offset.
    fn timestamp(
        &self,
        config: &Self::Config,
        file_state: &Self::FileState,
    ) -> chrono::DateTime<chrono::Local>;

    /// Get the formatted message (may be constructed lazily).
    ///
    /// Returns the raw log message without any display decorations.
    /// Used for filtering, anomaly detection, and template key computation.
    fn message(&self) -> String;

    /// Get the message as it should be displayed in the UI.
    ///
    /// Prepends a `[±HH:MM:SS.mmm]` offset prefix when a non-zero time offset
    /// is active for this source, so every view (log table, bookmarks, etc.)
    /// shows consistent decorated output without duplicating the logic.
    ///
    /// Each linetype implements this explicitly so that type-specific display
    /// decisions (e.g. DLT embeds calibration differently) can diverge freely.
    fn display_message(&self, file_state: &Self::FileState) -> String;

    /// Get the raw line as it appeared in the source (may be constructed lazily)
    fn raw(&self) -> String;

    /// Get the original line number in the source file
    fn line_number(&self) -> usize;

    /// Get the anomaly score
    fn anomaly_score(&self) -> f64;

    /// Set the anomaly score
    fn set_anomaly_score(&mut self, score: f64);

    /// Render format-specific context menu items for a single log line.
    ///
    /// Called inside an egui context menu. Implementations write into
    /// `file_state.calibration` to open the calibration window for that source.
    /// Generic items (bookmark, copy) are handled by the table, not here.
    fn egui_render_context_menu(
        &self,
        ui: &mut Ui,
        config: &Self::Config,
        file_state: &mut Self::FileState,
    );
}

/// Provides a unique slug for a file type, used as the JSON key for `file_state`
/// in `.crab` files. Implemented automatically by the `register_filetypes!` macro
/// via `stringify!($slug)` — no hand-written impl is required in any filetype file.
pub trait HasSlug {
    const SLUG: &'static str;
}

pub trait InputFileType: HasSlug {
    /// Log line type for this filetype
    type LineType: LineType;

    /// File extensions supported by this format (hint for file dialog only — not used
    /// for detection; multiple types may share extensions).
    const FILE_EXTENSIONS: &'static [&'static str];

    /// Open the file for pull-based reading, consuming the type-specific config value.
    ///
    /// Called by the registry to open any registered file type without knowing its
    /// concrete type. The `config` argument carries format-specific settings taken
    /// from [`GlobalFileConfig`] (e.g. DLT timestamp source). Types whose `Config`
    /// is `()` may ignore the argument.
    fn open(
        path: &::std::path::Path,
        config: <Self::LineType as LineType>::Config,
    ) -> Result<Self, String>
    where
        Self: Sized;

    /// Read the next `lines_to_read` lines/messages/packets from the file.
    ///
    /// Raw parsing only — no progress reporting, no chunk management.
    /// `ChunkedLoader` drives this method with adaptive chunk sizing and progress.
    /// Returns fewer than `lines_to_read` items (including zero) to signal EOF.
    fn read(&mut self, lines_to_read: usize) -> Result<Vec<Self::LineType>, String>;

    /// Bytes consumed from the source file so far.
    ///
    /// Used by `ChunkedLoader` to compute loading progress. Must increase
    /// monotonically as `read()` is called. May be an estimate.
    fn bytes_consumed(&self) -> u64;
}

pub trait BinaryFileType: InputFileType {
    /// One or more byte sequences that identify this format at the start of a file.
    ///
    /// # Invariants (enforced at build time)
    /// - At least one pattern must be provided.
    /// - No pattern may be a byte-prefix of any other pattern across all registered
    ///   binary file types (guarantees unambiguous detection).
    const MAGIC_BYTES: &'static [&'static [u8]];
}

pub trait TextFileType: InputFileType {
    /// Returns `true` if the file content looks like this format.
    ///
    /// Called in registration order; first match wins. `Generic` must be last
    /// (its `looks_like` always returns `true`). `Bugreport` must precede `Logcat`.
    fn looks_like(file: &mut dyn std::io::Read) -> bool;
}
