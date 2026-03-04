use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use crate::filetype::{CalibrationState, LogFileState, render_calibration};

/// Interior-mutable file state for the four simple time-offset-based formats.
///
/// `time_offset_ms` uses `AtomicI64` so that rayon worker threads can read it
/// concurrently in `timestamp()` without locking.  `calibration` holds the
/// currently-open calibration window (UI-thread only) behind a `Mutex`; it is
/// always uncontended in practice.
pub struct SimpleFileState {
    pub time_offset_ms: AtomicI64,
    pub calibration: Mutex<Option<CalibrationState>>,
}

impl SimpleFileState {
    /// Read the current time offset in milliseconds.
    #[inline]
    pub fn time_offset_ms(&self) -> i64 {
        self.time_offset_ms.load(Ordering::Relaxed)
    }

    /// Set the time offset in milliseconds.
    #[inline]
    pub fn set_time_offset_ms(&self, v: i64) {
        self.time_offset_ms.store(v, Ordering::Relaxed);
    }
}

impl Default for SimpleFileState {
    fn default() -> Self {
        Self {
            time_offset_ms: AtomicI64::new(0),
            calibration: Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for SimpleFileState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimpleFileState")
            .field("time_offset_ms", &self.time_offset_ms())
            .finish_non_exhaustive()
    }
}

impl Clone for SimpleFileState {
    fn clone(&self) -> Self {
        Self {
            time_offset_ms: AtomicI64::new(self.time_offset_ms()),
            calibration: Mutex::new(None), // calibration is transient
        }
    }
}

impl serde::Serialize for SimpleFileState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = s.serialize_struct("SimpleFileState", 1)?;
        state.serialize_field("time_offset_ms", &self.time_offset_ms())?;
        state.end()
    }
}

impl<'de> serde::Deserialize<'de> for SimpleFileState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Helper {
            #[serde(default)]
            time_offset_ms: i64,
        }
        let h = Helper::deserialize(d)?;
        Ok(Self {
            time_offset_ms: AtomicI64::new(h.time_offset_ms),
            calibration: Mutex::new(None),
        })
    }
}

impl LogFileState for SimpleFileState {
    fn egui_render_file_state(&self, ui: &egui::Ui) -> bool {
        let mut cal = self.calibration.lock().expect("calibration lock poisoned");
        if let Some(offset_ms) = render_calibration(ui, &mut cal) {
            self.set_time_offset_ms(offset_ms);
            true
        } else {
            false
        }
    }
}
