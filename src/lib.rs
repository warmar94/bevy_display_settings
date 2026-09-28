//! UI-agnostic display and renderer settings for Bevy.
//!
//! The crate owns the logic every game's graphics menu needs and none of the look: the game draws
//! its menu with any UI, sends requests and reads state and facts.
//!
//! - **Before the app is built**: [`RendererChoice`] picks the renderer backend (launch flag >
//!   `WGPU_BACKEND` > saved setting) and gives the `RenderPlugin`; [`primary_window`] gives the
//!   first window.
//! - **At runtime**: [`DisplaySettingsPlugin`] keeps [`DisplaySettings`] as a resource and applies
//!   display mode, windowed resolution (always through [`windowed_fit`]), vsync and shadows live.
//!   A mode or resolution change starts a keep-or-revert countdown; a renderer change raises
//!   [`RestartRequired`] and [`RequestRestart`] saves, relaunches and exits.
//! - **Files**: [`load_ron_or_default`] / [`save_ron_atomic`] never panic and never leave a half
//!   file; [`testing`] pins old spellings so a rename cannot silently reset a player's settings.
//!
//! Requests (you write): [`ApplyDisplaySettings`], [`KeepDisplaySettings`],
//! [`RevertDisplaySettings`], [`DiscardDisplayChanges`], [`SaveDisplaySettings`],
//! [`RequestRestart`]. Facts (you read): [`DisplaySettingsApplied`], [`DisplayConfirmPending`],
//! [`DisplayKept`], [`DisplayReverted`], [`DisplaySettingsSaved`], [`DisplaySaveFailed`],
//! [`RestartRequired`], [`RestartFailed`], [`DisplayDiagnostics`]. State (you read):
//! [`DisplaySettings`], [`SavedDisplaySettings`], [`DisplayConfirm`], [`RendererState`],
//! [`DisplayInfo`].
//!
//! Schedules: everything runs in `Update`, in [`DisplaySettingsSystems::Requests`], then
//! [`DisplaySettingsSystems::Apply`], then [`DisplaySettingsSystems::Report`].
#![warn(missing_docs)]

mod file;
mod messages;
mod renderer;
mod settings;
mod systems;
pub mod testing;
#[cfg(test)]
mod tests;
mod window;

/// Every Rust example in the README compiles (checked by `cargo test`).
#[cfg(all(doctest, feature = "render", feature = "shadows"))]
#[doc = include_str!("../README.md")]
struct ReadmeDoctests;

use std::path::PathBuf;
use std::time::Duration;

use bevy_app::{App, AppExit, Plugin, Update};
use bevy_ecs::schedule::{IntoScheduleConfigs, SystemSet};

pub use file::{load_ron_or_default, parse_ron_or_default, path_next_to_exe, save_ron_atomic, temp_path, to_ron_pretty, LoadStatus, Loaded};
pub use messages::{
    ApplyDisplaySettings, DiscardDisplayChanges, DisplayConfirmPending, DisplayDiagnostics, DisplayKept, DisplayReverted, DisplaySaveFailed,
    DisplaySettingsApplied, DisplaySettingsSaved, KeepDisplaySettings, MonitorReport, RequestRestart, RestartFailed, RestartRequired, RevertDisplaySettings,
    RevertReason, SaveDisplaySettings,
};
pub use renderer::{relaunch, relaunch_args, Platform, Relauncher, RendererChoice, RendererFlags, RendererSource, RendererState, WGPU_BACKEND_ENV};
pub use settings::{DisplayMode, DisplaySettings, RendererBackend, MAX_WINDOW};
pub use systems::{DisplayConfirm, DisplaySettingsConfig, SavedDisplaySettings};
pub use window::{
    display_mode_of, present_mode, primary_window, resolution_applies, resolution_options, window_mode, windowed_fit, windowed_limit, DisplayInfo, MonitorInfo,
    Screen, WindowTarget, MIN_WINDOW, UNKNOWN_MONITOR_WINDOW, WINDOW_BORDER_PX, WINDOW_CHROME_PX,
};

/// The default keep-or-revert countdown: 10 seconds.
pub const DEFAULT_CONFIRM_TIMEOUT: Duration = Duration::from_secs(10);
/// The default number of frames before [`DisplayDiagnostics`]: 10 (the window and the monitors
/// do not exist at `Startup`, and a resize takes the OS a few frames).
pub const DEFAULT_DIAGNOSTICS_FRAMES: u32 = 10;

/// The plugin's systems, all in `Update`, in this order.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplaySettingsSystems {
    /// Read the requests, write the facts, run the countdown.
    Requests,
    /// Refresh [`DisplayInfo`], push the settings to the window and the lights.
    Apply,
    /// Write [`DisplayDiagnostics`] when due.
    Report,
}

/// Display and renderer settings. See the crate documentation.
///
/// ```no_run
/// use bevy::prelude::*;
/// use bevy_display_settings::*;
///
/// let plugin = DisplaySettingsPlugin::from_file("settings.ron");
/// let renderer = RendererChoice::from_process(plugin.settings.renderer, &RendererFlags::default());
/// println!("{}", renderer.describe());
/// App::new()
///     .add_plugins(
///         DefaultPlugins
///             .set(renderer.render_plugin())
///             .set(WindowPlugin { primary_window: Some(primary_window(&plugin.settings, "My game")), ..default() }),
///     )
///     .add_plugins(DisplaySettingsPlugin { renderer: Some(renderer), ..plugin })
///     .run();
/// ```
#[derive(Clone, Debug)]
pub struct DisplaySettingsPlugin {
    /// The settings to start with (usually loaded from the file). Sanitized on build.
    pub settings: DisplaySettings,
    /// The settings file [`SaveDisplaySettings`] and [`RequestRestart`] write. `None`: nothing is
    /// written; persist [`SavedDisplaySettings`] yourself on [`DisplaySettingsSaved`].
    pub file: Option<PathBuf>,
    /// The renderer decision made before the app was built. `None`: assume
    /// `settings.renderer` from the settings.
    pub renderer: Option<RendererChoice>,
    /// The launch flags stripped from a relaunch (the default [`Relauncher`] uses them).
    pub renderer_flags: RendererFlags,
    /// How long the keep-or-revert countdown runs (default 10 s). Zero turns the safety net off.
    pub confirm_timeout: Duration,
    /// Frames before [`DisplayDiagnostics`] after startup and after each applied change
    /// (default 10).
    pub diagnostics_after_frames: u32,
    /// Log [`DisplayDiagnostics::line`] at `info` level (default `true`).
    pub log_diagnostics: bool,
    /// Apply `shadows` to directional lights (default `true`; needs feature `shadows`).
    pub manage_shadows: bool,
}

impl Default for DisplaySettingsPlugin {
    fn default() -> Self {
        Self {
            settings: DisplaySettings::default(),
            file: None,
            renderer: None,
            renderer_flags: RendererFlags::default(),
            confirm_timeout: DEFAULT_CONFIRM_TIMEOUT,
            diagnostics_after_frames: DEFAULT_DIAGNOSTICS_FRAMES,
            log_diagnostics: true,
            manage_shadows: true,
        }
    }
}

impl DisplaySettingsPlugin {
    /// Load `path` ([`DisplaySettings::load`]: missing or broken gives defaults, never panics) and
    /// save back to it.
    pub fn from_file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        Self { settings: DisplaySettings::load(&path), file: Some(path), ..Default::default() }
    }
}

impl Plugin for DisplaySettingsPlugin {
    fn build(&self, app: &mut App) {
        let settings = self.settings.clone().sanitized();
        let choice = match &self.renderer {
            Some(c) => {
                tracing::info!("{}", c.describe());
                c.clone()
            }
            None => RendererChoice::resolve(settings.renderer, &[], None, &self.renderer_flags, Platform::current()),
        };
        if !app.world().contains_resource::<Relauncher>() {
            app.insert_resource(Relauncher::process(self.renderer_flags.clone()));
        }
        app.insert_resource(RendererState::new(choice, settings.renderer))
            .insert_resource(SavedDisplaySettings(settings.clone()))
            .insert_resource(settings)
            .insert_resource(DisplaySettingsConfig {
                file: self.file.clone(),
                confirm_timeout: self.confirm_timeout,
                diagnostics_after_frames: self.diagnostics_after_frames,
                log_diagnostics: self.log_diagnostics,
                manage_shadows: self.manage_shadows,
            })
            .init_resource::<DisplayConfirm>()
            .init_resource::<DisplayInfo>()
            .init_resource::<systems::AppliedWindow>()
            .insert_resource(systems::DiagnosticsCountdown(Some(self.diagnostics_after_frames)))
            .add_message::<AppExit>()
            .add_message::<ApplyDisplaySettings>()
            .add_message::<KeepDisplaySettings>()
            .add_message::<RevertDisplaySettings>()
            .add_message::<DiscardDisplayChanges>()
            .add_message::<SaveDisplaySettings>()
            .add_message::<RequestRestart>()
            .add_message::<DisplaySettingsApplied>()
            .add_message::<DisplayConfirmPending>()
            .add_message::<DisplayKept>()
            .add_message::<DisplayReverted>()
            .add_message::<DisplaySettingsSaved>()
            .add_message::<DisplaySaveFailed>()
            .add_message::<RestartRequired>()
            .add_message::<RestartFailed>()
            .add_message::<DisplayDiagnostics>()
            .configure_sets(Update, (DisplaySettingsSystems::Requests, DisplaySettingsSystems::Apply, DisplaySettingsSystems::Report).chain())
            .add_systems(Update, systems::handle_requests.in_set(DisplaySettingsSystems::Requests))
            .add_systems(Update, (systems::refresh_display_info, systems::apply_window).chain().in_set(DisplaySettingsSystems::Apply))
            .add_systems(Update, systems::report_diagnostics.in_set(DisplaySettingsSystems::Report));
        #[cfg(feature = "shadows")]
        app.add_systems(Update, systems::apply_shadows.after(systems::apply_window).in_set(DisplaySettingsSystems::Apply));
    }
}
