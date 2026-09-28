//! A small, safe helper for a public settings file (RON): load that never panics, atomic save.

use std::io;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// How a settings file load went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadStatus {
    /// The file was read and parsed.
    Loaded,
    /// There is no file (a first run). The defaults were used, silently.
    Missing,
    /// The file exists but could not be read (permissions, not UTF-8, ...). Defaults were used.
    Unreadable(String),
    /// The file is not valid for this type. Defaults were used.
    Invalid(String),
}

impl LoadStatus {
    /// `true` for [`LoadStatus::Loaded`].
    pub fn is_loaded(&self) -> bool {
        matches!(self, Self::Loaded)
    }
}

/// A loaded value and how the load went.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded<T> {
    /// The value: from the file, or `T::default()`.
    pub value: T,
    /// What happened.
    pub status: LoadStatus,
}

/// Parse RON text into `T`; on an error, `T::default()` with [`LoadStatus::Invalid`] and a
/// warning. Never panics.
///
/// Give `T` `#[serde(default)]` so a missing field costs only that field, and `#[serde(alias)]`
/// on every renamed field or variant: a rename without an alias makes every existing file
/// [`Invalid`](LoadStatus::Invalid), which silently resets ALL of a player's settings.
pub fn parse_ron_or_default<T: DeserializeOwned + Default>(text: &str) -> Loaded<T> {
    match ron::from_str::<T>(text) {
        Ok(value) => Loaded { value, status: LoadStatus::Loaded },
        Err(e) => {
            tracing::warn!("bevy_display_settings: settings are not valid ({e}); using defaults");
            Loaded { value: T::default(), status: LoadStatus::Invalid(e.to_string()) }
        }
    }
}

/// Load a RON settings file into `T`. Missing file: `T::default()`, no warning. Unreadable or
/// invalid: `T::default()` and a warning. Never panics.
///
/// Read with plain `std::fs` on purpose: a player's settings file lives on disk next to the game
/// and is written by the game; it is never embedded in the executable.
pub fn load_ron_or_default<T: DeserializeOwned + Default>(path: impl AsRef<Path>) -> Loaded<T> {
    let path = path.as_ref();
    match std::fs::read_to_string(path) {
        Ok(text) => match ron::from_str::<T>(&text) {
            Ok(value) => Loaded { value, status: LoadStatus::Loaded },
            Err(e) => {
                tracing::warn!("bevy_display_settings: {} is not valid ({e}); using defaults", path.display());
                Loaded { value: T::default(), status: LoadStatus::Invalid(e.to_string()) }
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => Loaded { value: T::default(), status: LoadStatus::Missing },
        Err(e) => {
            tracing::warn!("bevy_display_settings: cannot read {} ({e}); using defaults", path.display());
            Loaded { value: T::default(), status: LoadStatus::Unreadable(e.to_string()) }
        }
    }
}

/// The pretty RON [`save_ron_atomic`] writes.
pub fn to_ron_pretty<T: Serialize>(value: &T) -> Result<String, ron::Error> {
    ron::ser::to_string_pretty(value, ron::ser::PrettyConfig::new().indentor("    ".to_string()))
}

/// The temporary file [`save_ron_atomic`] writes before renaming it over `path`: `<path>.tmp`.
pub fn temp_path(path: &Path) -> PathBuf {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    PathBuf::from(tmp)
}

/// Write `value` as pretty RON **atomically**: `<path>.tmp` first, then a rename over `path`, so a
/// crash mid-write never leaves a half-written file. Creates the parent directory. The temporary
/// file is removed if the rename fails. Never panics; the caller decides what to do with an error.
pub fn save_ron_atomic<T: Serialize>(path: impl AsRef<Path>, value: &T) -> io::Result<()> {
    let path = path.as_ref();
    let text = to_ron_pretty(value).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_path(path);
    std::fs::write(&tmp, text)?;
    // `rename` replaces an existing file on Windows too.
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// `file_name` in the directory of the running executable, the usual home of a player's settings
/// file in a shipped game. Falls back to `file_name` itself (relative to the working directory)
/// when the executable's path is unknown.
///
/// During development `cargo run` puts the executable in `target/<profile>/`; many games prefer
/// the working directory there. Pick whichever suits you; the plugin takes any path.
pub fn path_next_to_exe(file_name: impl AsRef<Path>) -> PathBuf {
    let file_name = file_name.as_ref();
    match std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) {
        Some(dir) => dir.join(file_name),
        None => file_name.to_path_buf(),
    }
}
