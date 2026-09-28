//! Test helpers for a public settings file: prove an old file still loads.
//!
//! A player's settings file outlives every version of the game. Renaming a field or an enum
//! variant without `#[serde(alias = "old_name")]` does not fail loudly: the whole file stops
//! parsing, the loader falls back to the defaults, and every setting the player chose is gone.
//! Pin each rename with a test:
//!
//! ```
//! use bevy_display_settings::testing::assert_old_spelling_loads;
//! use bevy_display_settings::{DisplayMode, DisplaySettings};
//!
//! // `display_mode` and `Exclusive` are older spellings of `mode` and `Fullscreen`.
//! let s: DisplaySettings = assert_old_spelling_loads("(display_mode: Exclusive, vsync: false)");
//! assert_eq!(s.mode, DisplayMode::Fullscreen);
//! assert!(!s.vsync);
//! ```

use std::fmt::Debug;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// Parse `old_file` **strictly** (no fallback to defaults) and return the value, so the caller can
/// assert the old values landed on the new fields.
///
/// # Panics
///
/// With the parse error and the text, when `old_file` does not parse: exactly the case in which a
/// real loader would silently reset the player's settings.
#[track_caller]
pub fn assert_old_spelling_loads<T: DeserializeOwned>(old_file: &str) -> T {
    match ron::from_str::<T>(old_file) {
        Ok(v) => v,
        Err(e) => panic!("an older settings file no longer loads (a rename without #[serde(alias)]?): {e}\n--- file ---\n{old_file}"),
    }
}

/// Serialize `value` the way [`save_ron_atomic`](crate::save_ron_atomic) does, parse it back and
/// return the result.
///
/// # Panics
///
/// When the value does not serialize, the text does not parse, or the result differs.
#[track_caller]
pub fn assert_round_trips<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> T {
    let text = match crate::to_ron_pretty(value) {
        Ok(t) => t,
        Err(e) => panic!("the value does not serialize: {e}"),
    };
    let back: T = match ron::from_str(&text) {
        Ok(v) => v,
        Err(e) => panic!("the saved text does not parse back: {e}\n--- file ---\n{text}"),
    };
    assert_eq!(&back, value, "a save/load round trip changed the value\n--- file ---\n{text}");
    back
}
