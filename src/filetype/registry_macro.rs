//! Registry macro for logcrab file types.
//!
//! # Usage
//!
//! ```ignore
//! crate::register_filetypes! {
//!     binary {
//!         dlt:     Dlt:     DltFileType:     DltLogLine,
//!         btsnoop: Btsnoop: BtsnoopFileType: BtsnoopLogLine,
//!         pcap:    Pcap:    PcapFileType:    PcapLogLine,
//!     }
//!     text {
//!         logcat:  Logcat:  LogcatFileType:  LogcatLogLine,
//!         generic: Generic: GenericFileType: GenericLogLine,
//!     }
//! }
//! ```
//!
//! Each entry is `slug: Arm: FileType: LogLine`:
//! - `slug` — `snake_case` field name in [`GlobalFileConfig`] and the config key used when
//!   propagating settings to open sources.
//! - `Arm` — `PascalCase` variant name in [`DataSourceVariant`].
//! - `FileType` — the struct implementing [`BinaryFileType`] or [`TextFileType`].
//! - `LogLine` — the concrete log-line type stored in [`SourceData`].
//!
//! **Binary types** must implement [`BinaryFileType`]; detected by magic bytes.
//! **Text types** must implement [`TextFileType`]; detection order matters (first match
//! wins) and should be handled by the caller. `Generic` (always-true) must be last.
//!
//! The macro generates:
//! - [`GlobalFileConfig`] – one serializable `Config` field per type.
//! - [`DataSourceVariant`] – enum with one arm per registered type.
//! - All [`DataSourceVariant`] dispatch methods.
//! - [`AsTypedSource<T>`] and `From<Arc<SourceData<T>>>` impls for each arm.
//! - [`all_file_extensions()`] – deduplicated list of all file extensions.
//! - [`try_open_binary()`] – reads the file header, matches magic bytes, and opens the source.
//! - [`open_text_source()`] – runs `looks_like()` on a sample and opens the source.
//! - Compile-time assertions: each binary type has ≥1 magic pattern and no two patterns
//!   across all binary types are byte-prefix of one another.

/// Compile-time helpers for magic byte prefix invariant checking.
pub mod const_checks {
    /// Returns `true` if `needle` is a byte-prefix of `haystack` (including equal length).
    pub const fn is_prefix(needle: &[u8], haystack: &[u8]) -> bool {
        if needle.len() > haystack.len() {
            return false;
        }
        let mut i = 0;
        while i < needle.len() {
            if needle[i] != haystack[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// Returns `true` if any pattern in `a` is a prefix of any pattern in `b`,
    /// or vice-versa. Used to enforce unambiguous magic-byte detection.
    pub const fn slices_have_prefix_conflict(a: &[&[u8]], b: &[&[u8]]) -> bool {
        let mut i = 0;
        while i < a.len() {
            let mut j = 0;
            while j < b.len() {
                if is_prefix(a[i], b[j]) || is_prefix(b[j], a[i]) {
                    return true;
                }
                j += 1;
            }
            i += 1;
        }
        false
    }

    /// Returns `true` if any two patterns *within the same slice* are prefix-related.
    /// Catches degenerate `MAGIC_BYTES` like `&[b"DLT", b"DLT\x01"]`.
    pub const fn self_has_prefix_conflict(patterns: &[&[u8]]) -> bool {
        let mut i = 0;
        while i < patterns.len() {
            let mut j = i + 1;
            while j < patterns.len() {
                if is_prefix(patterns[i], patterns[j]) || is_prefix(patterns[j], patterns[i]) {
                    return true;
                }
                j += 1;
            }
            i += 1;
        }
        false
    }
}

/// Core registry macro. See module-level documentation for usage.
///
/// The macro is `#[macro_export]`-ed so it is available at the crate root as
/// `logcrab::register_filetypes!` but the canonical import path is
/// `crate::filetype::registry_macro::register_filetypes`.
#[macro_export]
macro_rules! register_filetypes {
    (
        binary {
            $( $b_slug:ident : $b_arm:ident : $b_ftype:ty : $b_logline:ty ),* $(,)?
        }
        text {
            $( $t_slug:ident : $t_arm:ident : $t_ftype:ty : $t_logline:ty ),* $(,)?
        }
    ) => {
        // ── Compile-time invariants ──────────────────────────────────────────────

        // Each binary type must have at least one magic pattern.
        $(
            const _: () = assert!(
                !<$b_ftype as $crate::filetype::BinaryFileType>::MAGIC_BYTES.is_empty(),
                concat!(stringify!($b_ftype), ": MAGIC_BYTES must not be empty"),
            );
        )*

        // Within each binary type, no two patterns may be prefix-related.
        $(
            const _: () = assert!(
                !$crate::filetype::registry_macro::const_checks::self_has_prefix_conflict(
                    <$b_ftype as $crate::filetype::BinaryFileType>::MAGIC_BYTES,
                ),
                concat!(stringify!($b_ftype), ": MAGIC_BYTES contains patterns that are byte-prefixes of each other"),
            );
        )*

        // Across binary types, no two patterns from different types may be prefix-related.
        $crate::__check_cross_type_magic_prefix!(
            $( $b_ftype : $b_logline ),*
        );

        // ── HasSlug impls ────────────────────────────────────────────────────────

        $(
            impl $crate::filetype::HasSlug for $b_ftype {
                const SLUG: &'static str = stringify!($b_slug);
            }
        )*
        $(
            impl $crate::filetype::HasSlug for $t_ftype {
                const SLUG: &'static str = stringify!($t_slug);
            }
        )*

        // ── GlobalFileConfig ─────────────────────────────────────────────────────

        /// Persistent per-type file format configuration.
        ///
        /// One plain `T::Config` value per registered file type. Stored in the
        /// global config and serialized to disk. When a file is loaded, the relevant
        /// field's value is cloned into the source's own `Arc<RwLock<T::Config>>`.
        /// Call [`LogStore::rebuild_all_time_indices`] after mutating any field to
        /// propagate the change to all open sources of that type.
        #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
        pub struct GlobalFileConfig {
            $(
                #[serde(default)]
                pub $b_slug: <$b_logline as $crate::filetype::LineType>::Config,
            )*
            $(
                #[serde(default)]
                pub $t_slug: <$t_logline as $crate::filetype::LineType>::Config,
            )*
        }

        impl GlobalFileConfig {
            /// Render per-type config settings UI.
            ///
            /// Calls [`$crate::filetype::EguiConfig::egui_render`] on each config field
            /// in registration order. Returns `true` if any value changed.
            /// The caller is responsible for calling
            /// [`LogStore::rebuild_all_time_indices`] when this returns `true`.
            pub fn render(&mut self, ui: &mut egui::Ui) -> bool {
                use $crate::filetype::EguiConfig as _;
                let mut changed = false;
                $( changed |= self.$b_slug.egui_render(ui); )*
                $( changed |= self.$t_slug.egui_render(ui); )*
                changed
            }
        }

        // ── Extension / magic helpers ────────────────────────────────────────────

        /// Returns all file extensions across every registered type, sorted and deduplicated.
        ///
        /// Intended for the file-open dialog filter. Detection itself uses magic bytes
        /// (binary types) or `looks_like()` (text types), not extensions.
        pub fn all_file_extensions() -> Vec<&'static str> {
            let mut exts: Vec<&'static str> = Vec::new();
            $(
                exts.extend_from_slice(
                    <$b_ftype as $crate::filetype::InputFileType>::FILE_EXTENSIONS
                );
            )*
            $(
                exts.extend_from_slice(
                    <$t_ftype as $crate::filetype::InputFileType>::FILE_EXTENSIONS
                );
            )*
            exts.sort_unstable();
            exts.dedup();
            exts
        }

        pub fn try_open_binary(
            path: &::std::path::Path,
            toast: &$crate::ui::ProgressToastHandle,
            warnings: &$crate::ui::ToastSender,
            file_config: &GlobalFileConfig,
        ) -> ::std::option::Option<(DataSourceVariant, Vec<$crate::core::SavedFilter>, Vec<$crate::core::SavedHighlight>)> {
            use ::std::io::Read as _;
            let mut file = ::std::fs::File::open(path).ok()?;
            let mut header = [0u8; 16];
            let n = file.read(&mut header).ok().filter(|&n| n >= 4)?;
            let header = &header[..n];
            $(
                if <$b_ftype as $crate::filetype::BinaryFileType>::MAGIC_BYTES
                    .iter()
                    .any(|p| header.starts_with(p))
                {
                    let config_val = file_config.$b_slug.clone();
                    let arc_config = ::std::sync::Arc::new(::std::sync::RwLock::new(config_val.clone()));
                    let (source, filters, highlights) = $crate::core::log_file::LogFileLoader::load_typed(
                        path.to_path_buf(),
                        toast,
                        warnings,
                        arc_config,
                        move |p, fs| <$b_ftype as $crate::filetype::InputFileType>::open(p, config_val, fs),
                    );
                    return Some((source.into(), filters, highlights));
                }
            )*
            // Header didn't match any registered binary type — caller should try text detection.
            None
        }

        /// Returns `None` when the file cannot be opened for sampling.
        pub fn open_text_source(
            path: &::std::path::Path,
            toast: &$crate::ui::ProgressToastHandle,
            warnings: &$crate::ui::ToastSender,
            file_config: &GlobalFileConfig,
        ) -> ::std::option::Option<(DataSourceVariant, Vec<$crate::core::SavedFilter>, Vec<$crate::core::SavedHighlight>)> {
            use ::std::io::Read as _;
            const MAX_SAMPLE_BYTES: usize = 100 * 1024;
            let mut sample = ::std::vec::Vec::with_capacity(MAX_SAMPLE_BYTES);
            match ::std::fs::File::open(path) {
                Ok(f) => { let _ = f.take(MAX_SAMPLE_BYTES as u64).read_to_end(&mut sample); }
                Err(e) => {
                    tracing::error!("Cannot open file for format detection: {e}");
                    warnings.send(format!("Cannot open file: {e}"));
                    return None;
                }
            }
            $(
                if <$t_ftype as $crate::filetype::TextFileType>::looks_like(
                    &mut ::std::io::Cursor::new(&sample),
                ) {
                    tracing::info!("Opening {} with detected format {}", path.display(), stringify!($t_ftype));
                    let config_val = file_config.$t_slug.clone();
                    let arc_config = ::std::sync::Arc::new(::std::sync::RwLock::new(config_val.clone()));
                    let (source, filters, highlights) = $crate::core::log_file::LogFileLoader::load_typed(
                        path.to_path_buf(),
                        toast,
                        warnings,
                        arc_config,
                        move |p, fs| <$t_ftype as $crate::filetype::InputFileType>::open(p, config_val, fs),
                    );
                    return Some((source.into(), filters, highlights));
                }
            )*
            // Should never be reached if the last text type is a catch-all.
            tracing::error!("open_text_source: no text type matched — is the catch-all registered last?");
            None
        }

        // ── DataSourceVariant ────────────────────────────────────────────────────

        #[derive(Debug, Clone)]
        pub enum DataSourceVariant {
            $( $b_arm(::std::sync::Arc<SourceData<$b_ftype>>), )*
            $( $t_arm(::std::sync::Arc<SourceData<$t_ftype>>), )*
        }

        impl DataSourceVariant {
            pub fn source_id(&self) -> u64 {
                match self {
                    $( Self::$b_arm(s) => s.source_id(), )*
                    $( Self::$t_arm(s) => s.source_id(), )*
                }
            }

            pub fn file_path(&self) -> &::std::path::Path {
                match self {
                    $( Self::$b_arm(s) => s.file_path(), )*
                    $( Self::$t_arm(s) => s.file_path(), )*
                }
            }

            pub fn version(&self) -> u64 {
                match self {
                    $( Self::$b_arm(s) => s.version(), )*
                    $( Self::$t_arm(s) => s.version(), )*
                }
            }

            pub fn len(&self) -> usize {
                match self {
                    $( Self::$b_arm(s) => s.len(), )*
                    $( Self::$t_arm(s) => s.len(), )*
                }
            }

            pub fn has_bookmark(&self, line_index: usize) -> bool {
                match self {
                    $( Self::$b_arm(s) => s.has_bookmark(line_index), )*
                    $( Self::$t_arm(s) => s.has_bookmark(line_index), )*
                }
            }

            pub fn get_bookmark(&self, line_index: usize) -> Option<Bookmark> {
                match self {
                    $( Self::$b_arm(s) => s.get_bookmark(line_index), )*
                    $( Self::$t_arm(s) => s.get_bookmark(line_index), )*
                }
            }

            pub fn get_bookmarks(&self) -> Vec<Bookmark> {
                match self {
                    $( Self::$b_arm(s) => s.get_bookmarks(), )*
                    $( Self::$t_arm(s) => s.get_bookmarks(), )*
                }
            }

            pub fn set_bookmark(&self, line_index: usize, name: String) {
                match self {
                    $( Self::$b_arm(s) => s.set_bookmark(line_index, name), )*
                    $( Self::$t_arm(s) => s.set_bookmark(line_index, name), )*
                }
            }

            pub fn remove_bookmark(&self, line_index: usize) -> Option<Bookmark> {
                match self {
                    $( Self::$b_arm(s) => s.remove_bookmark(line_index), )*
                    $( Self::$t_arm(s) => s.remove_bookmark(line_index), )*
                }
            }

            pub fn save_crab_file(
                &self,
                filters: &[$crate::core::SavedFilter],
                highlights: &[$crate::core::SavedHighlight],
            ) {
                match self {
                    $( Self::$b_arm(s) => s.save_crab_file(filters, highlights), )*
                    $( Self::$t_arm(s) => s.save_crab_file(filters, highlights), )*
                }
            }

            /// Drive any open calibration windows for this source (one per frame).
            pub fn render_file_state(&self, ui: &egui::Ui) -> bool {
                match self {
                    $( Self::$b_arm(s) => s.render_file_state(ui), )*
                    $( Self::$t_arm(s) => s.render_file_state(ui), )*
                }
            }

            /// Write the relevant field from `file_config` into this source's config
            /// arc, then rebuild the timestamp-sorted index and bump the version.
            pub fn apply_file_config_and_rebuild(&self, file_config: &GlobalFileConfig) {
                match self {
                    $( Self::$b_arm(s) => {
                        *s.config.write().expect("config lock poisoned") =
                            file_config.$b_slug.clone();
                        s.rebuild_time_index();
                    } )*
                    $( Self::$t_arm(s) => {
                        *s.config.write().expect("config lock poisoned") =
                            file_config.$t_slug.clone();
                        s.rebuild_time_index();
                    } )*
                }
            }

            /// Render format-specific context menu items for the line at `line_index`.
            pub fn render_context_menu(&self, line_index: usize, ui: &mut egui::Ui) {
                match self {
                    $( Self::$b_arm(s) => s.render_line_context_menu(line_index, ui), )*
                    $( Self::$t_arm(s) => s.render_line_context_menu(line_index, ui), )*
                }
            }

            /// Get the fully-calibrated timestamp for the line at `line_index`.
            ///
            /// Locks `config` and `file_state` and calls `LineType::timestamp()`, so
            /// both config-driven source selection (e.g. DLT ECU/session clock) and the
            /// per-source calibration offset are applied. Returns `None` if the line
            /// index is out of bounds.
            pub fn adjusted_timestamp(&self, line_index: usize) -> Option<::chrono::DateTime<::chrono::Local>> {
                match self {
                    $( Self::$b_arm(s) => {
                        let lines = s.lines.read().expect("lines lock poisoned");
                        let config = s.config.read().expect("config lock poisoned");
                        let file_state = &*s.file_state;
                        lines.get(line_index).map(|l| l.timestamp(&*config, file_state))
                    } )*
                    $( Self::$t_arm(s) => {
                        let lines = s.lines.read().expect("lines lock poisoned");
                        let config = s.config.read().expect("config lock poisoned");
                        let file_state = &*s.file_state;
                        lines.get(line_index).map(|l| l.timestamp(&*config, file_state))
                    } )*
                }
            }

            /// Look up a single line by its index, returned as a fully-computed [`LogLine`] DTO.
            ///
            /// Acquires source locks once and pre-computes all display fields including
            /// the adjusted timestamp.  Returns `None` when `id` is out of range.
            pub fn get_log_line(&self, id: usize) -> Option<$crate::core::log_store::LogLine> {
                match self {
                    $( Self::$b_arm(s) => s.get_as_log_line(id), )*
                    $( Self::$t_arm(s) => s.get_as_log_line(id), )*
                }
            }

            /// Filter lines by display-message and raw text in timestamp order.
            ///
            /// Predicate receives `(display_message, raw)` — the display message includes
            /// any active per-source overlays (e.g. SOME/IP SD decoding for PCAP).
            pub fn filter_sorted_by_search<F>(&self, predicate: &F) -> Vec<usize>
            where
                F: Fn(&str, &str) -> bool + Sync,
            {
                match self {
                    $( Self::$b_arm(s) => s.filter_sorted_by_search(predicate), )*
                    $( Self::$t_arm(s) => s.filter_sorted_by_search(predicate), )*
                }
            }
        }

        // `From<Arc<SourceData<FT>>>` for each arm
        $(
            impl From<::std::sync::Arc<SourceData<$b_ftype>>> for DataSourceVariant {
                fn from(s: ::std::sync::Arc<SourceData<$b_ftype>>) -> Self {
                    Self::$b_arm(s)
                }
            }
        )*
        $(
            impl From<::std::sync::Arc<SourceData<$t_ftype>>> for DataSourceVariant {
                fn from(s: ::std::sync::Arc<SourceData<$t_ftype>>) -> Self {
                    Self::$t_arm(s)
                }
            }
        )*
    };
}

/// Internal helper macro: generate compile-time prefix-conflict assertions for every
/// *ordered pair* `(A, B)` with `A ≠ B` among the binary types.
///
/// Uses a tt-muncher to consume the type list and emit one `const _: ()` assert
/// per unordered pair.
#[doc(hidden)]
#[macro_export]
macro_rules! __check_cross_type_magic_prefix {
    // Base case: zero or one type — no pairs possible.
    () => {};
    ( $_head_ftype:ty : $_head_logline:ty ) => {};

    // Recursive case: check head against every tail element, then recurse on tail.
    (
        $head_ftype:ty : $head_logline:ty,
        $( $tail_ftype:ty : $tail_logline:ty ),+ $(,)?
    ) => {
        // head vs. each tail
        $(
            const _: () = assert!(
                !$crate::filetype::registry_macro::const_checks::slices_have_prefix_conflict(
                    <$head_ftype as $crate::filetype::BinaryFileType>::MAGIC_BYTES,
                    <$tail_ftype as $crate::filetype::BinaryFileType>::MAGIC_BYTES,
                ),
                concat!(
                    stringify!($head_ftype),
                    " and ",
                    stringify!($tail_ftype),
                    ": MAGIC_BYTES patterns are byte-prefix of each other (ambiguous detection)",
                ),
            );
        )+

        // recurse on tail
        $crate::__check_cross_type_magic_prefix!( $( $tail_ftype : $tail_logline ),+ );
    };
}
