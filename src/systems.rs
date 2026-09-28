//! The plugin's resources and systems.

use std::path::PathBuf;
use std::time::Duration;

use bevy_app::AppExit;
use bevy_ecs::change_detection::DetectChanges;
use bevy_ecs::entity::Entity;
use bevy_ecs::lifecycle::RemovedComponents;
use bevy_ecs::message::{MessageReader, MessageWriter};
use bevy_ecs::query::{Changed, Has, Or, With};
use bevy_ecs::resource::Resource;
use bevy_ecs::system::{Query, Res, ResMut, SystemParam};
use bevy_time::{Real, Time};
use bevy_window::{Monitor, OnMonitor, PrimaryMonitor, PrimaryWindow, Window};

use crate::messages::*;
use crate::renderer::{Relauncher, RendererState};
use crate::settings::DisplaySettings;
use crate::window::{display_mode_of, present_mode, resolution_options, window_mode, DisplayInfo, MonitorInfo, WindowTarget};

// ---------------------------------------------------------------- resources

/// The plugin's configuration, copied from [`DisplaySettingsPlugin`](crate::DisplaySettingsPlugin).
/// Read-only.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct DisplaySettingsConfig {
    /// The settings file, if the plugin persists the settings itself.
    pub file: Option<PathBuf>,
    /// How long the keep-or-revert countdown runs. Zero turns the safety net off.
    pub confirm_timeout: Duration,
    /// Frames between startup (or an applied change) and [`DisplayDiagnostics`].
    pub diagnostics_after_frames: u32,
    /// Log [`DisplayDiagnostics::line`] at `info` level.
    pub log_diagnostics: bool,
    /// Apply the `shadows` setting to directional lights (feature `shadows`).
    pub manage_shadows: bool,
}

/// The last saved settings: the baseline [`DiscardDisplayChanges`] returns to. Read-only; when the
/// plugin has no file, persist this yourself after [`DisplaySettingsSaved`].
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct SavedDisplaySettings(pub DisplaySettings);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pending {
    /// The confirmed settings from before the change.
    previous: DisplaySettings,
    remaining: Duration,
    last_secs: u32,
    /// (Re)started this frame: do not tick yet.
    fresh: bool,
    /// A save was requested during the countdown: save again on keep.
    save_on_keep: bool,
}

/// The keep-or-revert countdown. Read-only for the game.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct DisplayConfirm {
    pending: Option<Pending>,
}

impl DisplayConfirm {
    /// Is a change waiting for [`KeepDisplaySettings`]?
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Time left before the automatic revert.
    pub fn remaining(&self) -> Option<Duration> {
        self.pending.as_ref().map(|p| p.remaining)
    }

    /// Whole seconds left (rounded up).
    pub fn secs_left(&self) -> Option<u32> {
        self.remaining().map(whole_secs)
    }

    /// The confirmed settings a revert would go back to.
    pub fn previous(&self) -> Option<&DisplaySettings> {
        self.pending.as_ref().map(|p| &p.previous)
    }
}

fn whole_secs(d: Duration) -> u32 {
    d.as_secs_f64().ceil().min(f64::from(u32::MAX)) as u32
}

/// What was last pushed to the window (so a player resizing the window is not snapped back when
/// an unrelated setting changes).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct AppliedWindow(pub(crate) Option<WindowTarget>);

/// Frames until the next [`DisplayDiagnostics`] (`None` = nothing scheduled).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct DiagnosticsCountdown(pub(crate) Option<u32>);

// ---------------------------------------------------------------- requests

#[derive(SystemParam)]
pub(crate) struct Requests<'w, 's> {
    apply: MessageReader<'w, 's, ApplyDisplaySettings>,
    keep: MessageReader<'w, 's, KeepDisplaySettings>,
    revert: MessageReader<'w, 's, RevertDisplaySettings>,
    discard: MessageReader<'w, 's, DiscardDisplayChanges>,
    save: MessageReader<'w, 's, SaveDisplaySettings>,
    restart: MessageReader<'w, 's, RequestRestart>,
}

#[derive(SystemParam)]
pub(crate) struct Facts<'w> {
    applied: MessageWriter<'w, DisplaySettingsApplied>,
    pending: MessageWriter<'w, DisplayConfirmPending>,
    kept: MessageWriter<'w, DisplayKept>,
    reverted: MessageWriter<'w, DisplayReverted>,
    saved: MessageWriter<'w, DisplaySettingsSaved>,
    save_failed: MessageWriter<'w, DisplaySaveFailed>,
    restart_required: MessageWriter<'w, RestartRequired>,
    restart_failed: MessageWriter<'w, RestartFailed>,
    exit: MessageWriter<'w, AppExit>,
}

#[derive(SystemParam)]
pub(crate) struct State<'w> {
    settings: ResMut<'w, DisplaySettings>,
    saved: ResMut<'w, SavedDisplaySettings>,
    confirm: ResMut<'w, DisplayConfirm>,
    renderer: ResMut<'w, RendererState>,
    config: Res<'w, DisplaySettingsConfig>,
    relauncher: Res<'w, Relauncher>,
}

impl State<'_> {
    fn set(&mut self, next: DisplaySettings, facts: &mut Facts) {
        if *self.settings != next {
            *self.settings = next;
        }
        let setting = self.settings.renderer;
        let want = (setting != self.renderer.choice().requested).then_some(setting);
        // Touch the resource only on a real change (change detection).
        if self.renderer.pending() != want && self.renderer.update(setting) {
            facts.restart_required.write(RestartRequired { renderer: setting });
        }
    }

    fn apply(&mut self, next: DisplaySettings, facts: &mut Facts) {
        let next = next.sanitized();
        let timeout = self.config.confirm_timeout;
        let baseline = self.confirm.previous().cloned().unwrap_or_else(|| self.settings.clone());
        let needs_confirm = !timeout.is_zero() && baseline.needs_confirm(&next) && self.settings.needs_confirm(&next);
        if needs_confirm {
            let secs = whole_secs(timeout);
            match self.confirm.pending.as_mut() {
                Some(p) => {
                    p.remaining = timeout;
                    p.last_secs = secs;
                    p.fresh = true;
                }
                None => {
                    self.confirm.pending = Some(Pending { previous: baseline, remaining: timeout, last_secs: secs, fresh: true, save_on_keep: false });
                }
            }
            facts.pending.write(DisplayConfirmPending { secs_left: secs });
        } else if self.confirm.is_pending() && !baseline.needs_confirm(&next) {
            // Back to the confirmed mode and size: nothing left to confirm.
            self.confirm.pending = None;
        }
        self.set(next.clone(), facts);
        facts.applied.write(DisplaySettingsApplied { settings: next, needs_confirm });
    }

    fn revert(&mut self, reason: RevertReason, facts: &mut Facts) {
        let Some(p) = self.confirm.pending.take() else { return };
        self.set(p.previous.clone(), facts);
        facts.reverted.write(DisplayReverted { reason, settings: p.previous });
    }

    fn keep(&mut self, facts: &mut Facts) {
        let Some(p) = self.confirm.pending.take() else { return };
        facts.kept.write(DisplayKept);
        if p.save_on_keep {
            let _ = self.save(facts);
        }
    }

    fn discard(&mut self, facts: &mut Facts) {
        self.confirm.pending = None;
        let saved = self.saved.0.clone();
        self.set(saved.clone(), facts);
        facts.reverted.write(DisplayReverted { reason: RevertReason::Discarded, settings: saved });
    }

    /// The settings to persist: the current ones, with the confirmed mode and resolution while a
    /// countdown runs.
    fn persistable(&self) -> DisplaySettings {
        let mut s = self.settings.clone();
        if let Some(prev) = self.confirm.previous() {
            s.mode = prev.mode;
            s.resolution = prev.resolution;
        }
        s
    }

    fn save(&mut self, facts: &mut Facts) -> Result<(), String> {
        let persist = self.persistable();
        if let Some(p) = self.confirm.pending.as_mut() {
            p.save_on_keep = true;
        }
        let path = self.config.file.clone();
        if let Some(path) = &path {
            if let Err(e) = persist.save(path) {
                tracing::warn!("bevy_display_settings: could not save {}: {e}", path.display());
                facts.save_failed.write(DisplaySaveFailed { path: path.clone(), error: e.to_string() });
                return Err(format!("could not save {}: {e}", path.display()));
            }
        }
        if self.saved.0 != persist {
            self.saved.0 = persist.clone();
        }
        facts.saved.write(DisplaySettingsSaved { path, settings: persist });
        Ok(())
    }

    fn restart(&mut self, facts: &mut Facts) {
        if let Err(error) = self.save(facts) {
            facts.restart_failed.write(RestartFailed { error });
            return;
        }
        match (self.relauncher.0)() {
            Ok(()) => {
                tracing::info!("bevy_display_settings: restarting");
                facts.exit.write(AppExit::Success);
            }
            Err(e) => {
                tracing::warn!("bevy_display_settings: could not restart: {e}");
                facts.restart_failed.write(RestartFailed { error: format!("could not start the new process: {e}") });
            }
        }
    }
}

/// Handle the request messages, in the order discard, revert, keep, apply (the last one of a
/// frame stands), save, restart; then tick the countdown.
pub(crate) fn handle_requests(mut requests: Requests, mut facts: Facts, mut state: State, time: Option<Res<Time<Real>>>) {
    if requests.discard.read().count() > 0 {
        state.discard(&mut facts);
    }
    if requests.revert.read().count() > 0 {
        state.revert(RevertReason::Requested, &mut facts);
    }
    if requests.keep.read().count() > 0 {
        state.keep(&mut facts);
    }
    if let Some(ApplyDisplaySettings(next)) = requests.apply.read().last().cloned() {
        state.apply(next, &mut facts);
    }
    if requests.save.read().count() > 0 {
        let _ = state.save(&mut facts);
    }
    if requests.restart.read().count() > 0 {
        state.restart(&mut facts);
    }

    // The countdown, on real time (it keeps running while the game's virtual time is paused).
    let delta = time.map_or(Duration::ZERO, |t| t.delta());
    let Some(p) = state.confirm.pending.as_mut() else { return };
    if p.fresh {
        p.fresh = false;
        return;
    }
    p.remaining = p.remaining.saturating_sub(delta);
    if p.remaining.is_zero() {
        state.revert(RevertReason::Timeout, &mut facts);
        return;
    }
    let secs = whole_secs(p.remaining);
    if secs != p.last_secs {
        p.last_secs = secs;
        facts.pending.write(DisplayConfirmPending { secs_left: secs });
    }
}

// ---------------------------------------------------------------- apply

/// A monitor was added or changed, or became the primary one.
type MonitorChanged = Or<(Changed<Monitor>, Changed<PrimaryMonitor>)>;

/// Rebuild [`DisplayInfo`] when a monitor or the primary window's monitor changed.
pub(crate) fn refresh_display_info(
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    changed: Query<(), MonitorChanged>,
    mut removed: RemovedComponents<Monitor>,
    windows: Query<Option<&OnMonitor>, With<PrimaryWindow>>,
    window_moved: Query<(), (With<PrimaryWindow>, Changed<OnMonitor>)>,
    mut info: ResMut<DisplayInfo>,
) {
    let any_removed = removed.read().count() > 0;
    if changed.is_empty() && !any_removed && window_moved.is_empty() {
        return;
    }
    let mut list: Vec<(Entity, MonitorInfo)> = monitors.iter().map(|(e, m, primary)| (e, MonitorInfo::from_monitor(m, primary))).collect();
    list.sort_by(|a, b| b.1.primary.cmp(&a.1.primary).then_with(|| a.1.name.cmp(&b.1.name)).then_with(|| a.0.cmp(&b.0)));
    let on = windows.single().ok().flatten().map(|o| o.0);
    let current = on.and_then(|e| list.iter().position(|(me, _)| *me == e)).or_else(|| list.iter().position(|(_, m)| m.primary)).or(if list.is_empty() {
        None
    } else {
        Some(0)
    });
    let monitors: Vec<MonitorInfo> = list.into_iter().map(|(_, m)| m).collect();
    let windowed_resolutions = current.and_then(|i| monitors.get(i)).map(|m| resolution_options(m.modes.iter().copied(), Some(m.screen))).unwrap_or_default();
    let next = DisplayInfo { monitors, current, windowed_resolutions };
    if *info != next {
        *info = next;
    }
}

/// Push the settings to the primary window: mode first, then the fitted windowed size, then the
/// present mode. Runs when the settings, the monitors or the window change; writes only what
/// differs.
pub(crate) fn apply_window(
    settings: Res<DisplaySettings>,
    info: Res<DisplayInfo>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    new_window: Query<(), (With<PrimaryWindow>, Changed<PrimaryWindow>)>,
    mut applied: ResMut<AppliedWindow>,
    mut diagnostics: ResMut<DiagnosticsCountdown>,
    config: Res<DisplaySettingsConfig>,
) {
    if !settings.is_changed() && !info.is_changed() && new_window.is_empty() {
        return;
    }
    let Ok(mut window) = windows.single_mut() else { return };
    let want = WindowTarget::new(&settings, info.current_screen());
    let prev = applied.0;
    if prev.map(|p| p.mode) != Some(want.mode) && display_mode_of(window.mode) != want.mode {
        window.mode = window_mode(want.mode);
    }
    if let Some((w, h)) = want.size {
        let (cw, ch) = (window.resolution.physical_width(), window.resolution.physical_height());
        let asked = prev.and_then(|p| p.size) != Some((w, h)) || cw > w || ch > h;
        if asked && (cw, ch) != (w, h) {
            window.resolution.set_physical_resolution(w, h);
        }
    }
    let present = present_mode(want.vsync);
    if window.present_mode != present {
        window.present_mode = present;
    }
    if prev.is_some() && prev != Some(want) {
        diagnostics.0 = Some(config.diagnostics_after_frames);
    }
    if prev != Some(want) {
        applied.0 = Some(want);
    }
}

/// Apply `shadows` to every directional light when the setting changes, and to each new light.
#[cfg(feature = "shadows")]
pub(crate) fn apply_shadows(settings: Res<DisplaySettings>, config: Res<DisplaySettingsConfig>, mut lights: Query<&mut bevy_light::DirectionalLight>) {
    if !config.manage_shadows {
        return;
    }
    let all = settings.is_changed();
    for mut light in &mut lights {
        if (all || light.is_added()) && light.shadow_maps_enabled != settings.shadows {
            light.shadow_maps_enabled = settings.shadows;
        }
    }
}

// ---------------------------------------------------------------- report

/// Write (and log) [`DisplayDiagnostics`] once the countdown reaches zero.
pub(crate) fn report_diagnostics(
    mut countdown: ResMut<DiagnosticsCountdown>,
    windows: Query<&Window, With<PrimaryWindow>>,
    info: Res<DisplayInfo>,
    config: Res<DisplaySettingsConfig>,
    mut out: MessageWriter<DisplayDiagnostics>,
) {
    let Some(n) = countdown.0 else { return };
    if n > 0 {
        countdown.0 = Some(n - 1);
        return;
    }
    countdown.0 = None;
    let Ok(w) = windows.single() else { return };
    let report = DisplayDiagnostics {
        mode: display_mode_of(w.mode),
        logical_size: (w.resolution.width(), w.resolution.height()),
        physical_size: (w.resolution.physical_width(), w.resolution.physical_height()),
        scale_factor: w.resolution.scale_factor(),
        present_mode: w.present_mode,
        monitors: info
            .monitors
            .iter()
            .map(|m| MonitorReport {
                name: m.name.clone(),
                size: (m.screen.width, m.screen.height),
                refresh_millihertz: m.refresh_millihertz,
                primary: m.primary,
            })
            .collect(),
    };
    if config.log_diagnostics {
        tracing::info!("{}", report.line());
    }
    out.write(report);
}
