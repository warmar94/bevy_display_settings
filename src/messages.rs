//! Request messages (the game writes) and fact messages (the plugin writes).

use std::path::PathBuf;

use bevy_ecs::message::Message;
use bevy_window::PresentMode;

use crate::settings::{DisplayMode, DisplaySettings, RendererBackend};

// ---------------------------------------------------------------- requests

/// Apply these settings. Sanitized first. A mode change, or a resolution change in `Windowed`,
/// starts the keep-or-revert countdown ([`DisplayConfirmPending`]); vsync and shadows apply at
/// once; a renderer change makes [`RestartRequired`]. Nothing is saved.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct ApplyDisplaySettings(pub DisplaySettings);

/// Keep the settings the countdown is asking about ("Keep changes"). No-op without a countdown.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeepDisplaySettings;

/// Go back to the settings from before the unconfirmed change ("Revert"). No-op without a
/// countdown.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RevertDisplaySettings;

/// Throw away every change since the last save ("Cancel"): the saved settings are applied again,
/// with no countdown (they are the known-good ones).
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiscardDisplayChanges;

/// Save the current settings (to the plugin's file, when it has one) and make them the new
/// baseline for [`DiscardDisplayChanges`]. During a countdown the unconfirmed mode and resolution
/// are **not** written: the confirmed ones are, and the save is repeated when the change is kept.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SaveDisplaySettings;

/// The player confirmed "restart now": save (like [`SaveDisplaySettings`]), start a new copy of
/// the game through the [`Relauncher`](crate::Relauncher), then exit with `AppExit::Success`. On a
/// failure the game keeps running and [`RestartFailed`] is written.
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestRestart;

// ---------------------------------------------------------------- facts

/// Settings were applied (by [`ApplyDisplaySettings`], a revert or a discard).
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettingsApplied {
    /// The settings now live.
    pub settings: DisplaySettings,
    /// Did this start (or restart) the keep-or-revert countdown?
    pub needs_confirm: bool,
}

/// The keep-or-revert countdown is running: written when it starts and each time the whole
/// number of seconds left changes. Show "Keep these settings? Reverting in {secs_left} s".
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayConfirmPending {
    /// Whole seconds left (rounded up).
    pub secs_left: u32,
}

/// The countdown ended with [`KeepDisplaySettings`].
#[derive(Message, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisplayKept;

/// Why settings were reverted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RevertReason {
    /// [`RevertDisplaySettings`].
    Requested,
    /// Nobody answered the countdown in time.
    Timeout,
    /// [`DiscardDisplayChanges`]: back to the saved settings.
    Discarded,
}

/// Settings were reverted; `settings` are live again.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct DisplayReverted {
    /// Why.
    pub reason: RevertReason,
    /// The settings now live.
    pub settings: DisplaySettings,
}

/// Settings were saved (and are the new baseline). `path` is `None` when the plugin has no file:
/// persist [`SavedDisplaySettings`](crate::SavedDisplaySettings) yourself then.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettingsSaved {
    /// The file written, if any.
    pub path: Option<PathBuf>,
    /// What was saved.
    pub settings: DisplaySettings,
}

/// Writing the settings file failed. Nothing else changed.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct DisplaySaveFailed {
    /// The file.
    pub path: PathBuf,
    /// The I/O error, as text.
    pub error: String,
}

/// The renderer setting now differs from what this run started with: it applies at the next
/// start. Written when that becomes true (or the target changes). Offer "Restart now" /
/// "Later"; "Restart now" = [`RequestRestart`]. The current state is
/// [`RendererState`](crate::RendererState).
#[derive(Message, Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestartRequired {
    /// The backend the next start will use.
    pub renderer: RendererBackend,
}

/// [`RequestRestart`] could not save or could not start the new process. The game keeps running.
#[derive(Message, Clone, Debug, PartialEq, Eq)]
pub struct RestartFailed {
    /// What went wrong, as text.
    pub error: String,
}

/// One monitor in [`DisplayDiagnostics`].
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorReport {
    /// The monitor's name.
    pub name: String,
    /// Physical size.
    pub size: (u32, u32),
    /// Refresh rate in millihertz, when known.
    pub refresh_millihertz: Option<u32>,
    /// Is it the primary monitor?
    pub primary: bool,
}

/// A snapshot of the display, written (and logged, unless turned off) a few frames after
/// startup and after each applied change: the thing to ask a player for when the frame rate
/// looks capped or the window looks wrong.
///
/// Exclusive fullscreen is the only mode that can exceed the monitor's refresh rate; borderless
/// on Windows is presented by the compositor and capped at it even with vsync off.
#[derive(Message, Clone, Debug, PartialEq)]
pub struct DisplayDiagnostics {
    /// The window's mode.
    pub mode: DisplayMode,
    /// Logical window size.
    pub logical_size: (f32, f32),
    /// Physical window size.
    pub physical_size: (u32, u32),
    /// The window's scale factor.
    pub scale_factor: f32,
    /// The window's present mode.
    pub present_mode: PresentMode,
    /// Every monitor.
    pub monitors: Vec<MonitorReport>,
}

impl DisplayDiagnostics {
    /// The one-line ASCII form, e.g. `>>> DISPLAY: mode Borderless, window 1920x1080 logical /
    /// 1920x1080 physical (scale 1.00), present_mode AutoVsync | monitors: "DELL" 1920x1080
    /// @60.0 Hz (primary)`.
    pub fn line(&self) -> String {
        let mons = if self.monitors.is_empty() {
            "none".to_string()
        } else {
            self.monitors
                .iter()
                .map(|m| {
                    let hz = match m.refresh_millihertz {
                        Some(mhz) => format!("@{:.1} Hz", f64::from(mhz) / 1000.0),
                        None => "@unknown Hz".to_string(),
                    };
                    format!("\"{}\" {}x{} {hz}{}", m.name, m.size.0, m.size.1, if m.primary { " (primary)" } else { "" })
                })
                .collect::<Vec<_>>()
                .join("; ")
        };
        format!(
            ">>> DISPLAY: mode {}, window {:.0}x{:.0} logical / {}x{} physical (scale {:.2}), present_mode {:?} | monitors: {mons}",
            self.mode.label(),
            self.logical_size.0,
            self.logical_size.1,
            self.physical_size.0,
            self.physical_size.1,
            self.scale_factor,
            self.present_mode
        )
    }
}
