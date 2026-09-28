//! The plugin in a headless app: a hand-spawned window and monitor, manual time, strict ambiguity
//! detection on `Update`. No windowing backend, no GPU.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bevy::ecs::message::Message;
use bevy::ecs::schedule::{LogLevel, ScheduleBuildSettings};
use bevy::math::{IVec2, UVec2};
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::window::{Monitor, PresentMode, PrimaryMonitor, PrimaryWindow, VideoMode};
use bevy_display_settings::*;

const FRAME: Duration = Duration::from_millis(100);

#[derive(Resource)]
struct Seen<T: Message>(Vec<T>);

fn collect<T: Message + Clone>(mut reader: MessageReader<T>, mut seen: ResMut<Seen<T>>) {
    seen.0.extend(reader.read().cloned());
}

fn watch<T: Message + Clone>(app: &mut App) {
    app.insert_resource(Seen::<T>(Vec::new())).add_systems(PostUpdate, collect::<T>);
}

fn seen<T: Message + Clone>(app: &mut App) -> Vec<T> {
    std::mem::take(&mut app.world_mut().resource_mut::<Seen<T>>().0)
}

fn strict(schedule: &mut Schedule) {
    schedule.set_build_settings(ScheduleBuildSettings { ambiguity_detection: LogLevel::Error, ..default() });
}

/// A 1920x1080 @ 60 Hz primary monitor with a few video modes.
fn monitor() -> (Monitor, PrimaryMonitor) {
    let mode = |w, h| VideoMode { physical_size: UVec2::new(w, h), bit_depth: 32, refresh_rate_millihertz: 60_000 };
    (
        Monitor {
            name: Some("Test Monitor".into()),
            physical_height: 1080,
            physical_width: 1920,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 1.0,
            video_modes: vec![mode(1920, 1080), mode(1600, 900), mode(1280, 720), mode(1280, 720)],
        },
        PrimaryMonitor,
    )
}

/// A headless app: the plugin (a fake relauncher counting calls), a window and a monitor.
fn app_with(plugin: DisplaySettingsPlugin) -> (App, Arc<AtomicUsize>) {
    let mut app = App::new();
    let settings = plugin.settings.clone();
    app.add_plugins((MinimalPlugins, plugin));
    let launches = Arc::new(AtomicUsize::new(0));
    let counter = launches.clone();
    app.insert_resource(Relauncher::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.edit_schedule(Update, strict);
    watch::<DisplaySettingsApplied>(&mut app);
    watch::<DisplayConfirmPending>(&mut app);
    watch::<DisplayKept>(&mut app);
    watch::<DisplayReverted>(&mut app);
    watch::<DisplaySettingsSaved>(&mut app);
    watch::<DisplaySaveFailed>(&mut app);
    watch::<RestartRequired>(&mut app);
    watch::<RestartFailed>(&mut app);
    watch::<DisplayDiagnostics>(&mut app);
    watch::<AppExit>(&mut app);
    app.world_mut().spawn((primary_window(&settings, "test"), PrimaryWindow));
    app.world_mut().spawn(monitor());
    app.update();
    (app, launches)
}

fn app() -> App {
    app_with(DisplaySettingsPlugin { log_diagnostics: false, ..default() }).0
}

fn send<M: Message>(app: &mut App, message: M) {
    app.world_mut().write_message(message);
    app.update();
}

fn settings(app: &App) -> DisplaySettings {
    app.world().resource::<DisplaySettings>().clone()
}

fn window(app: &mut App) -> Window {
    let mut q = app.world_mut().query_filtered::<&Window, With<PrimaryWindow>>();
    q.single(app.world()).expect("one primary window").clone()
}

fn physical(w: &Window) -> (u32, u32) {
    (w.resolution.physical_width(), w.resolution.physical_height())
}

fn with(f: impl FnOnce(&mut DisplaySettings)) -> DisplaySettings {
    let mut s = DisplaySettings::default();
    f(&mut s);
    s
}

fn tmp_file(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("bevy_display_settings_tests");
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

// ---------------------------------------------------------------- display info + window

#[test]
fn the_monitor_is_reported_with_its_windowed_resolutions() {
    let app = app();
    let info = app.world().resource::<DisplayInfo>();
    assert_eq!(info.monitors.len(), 1);
    let m = info.current_monitor().expect("current monitor");
    assert_eq!((m.name.as_str(), m.screen.width, m.refresh_hz()), ("Test Monitor", 1920, Some(60.0)));
    assert_eq!(info.windowed_resolutions, vec![(1280, 720), (1600, 900)], "deduplicated, without the monitor's own size");
}

#[test]
fn a_windowed_size_as_big_as_the_monitor_is_fitted_on_the_real_window() {
    let mut app = app();
    send(
        &mut app,
        ApplyDisplaySettings(with(|s| {
            s.mode = DisplayMode::Windowed;
            s.resolution = (1920, 1080);
        })),
    );
    let w = window(&mut app);
    assert_eq!(display_mode_of(w.mode), DisplayMode::Windowed);
    assert_eq!(physical(&w), (1904, 1000));
    // The setting keeps what the player asked for; only the request to the window is fitted.
    assert_eq!(settings(&app).resolution, (1920, 1080));
}

#[test]
fn the_resolution_is_ignored_outside_windowed() {
    let mut app = app();
    let before = physical(&window(&mut app));
    send(&mut app, ApplyDisplaySettings(with(|s| s.resolution = (800, 600))));
    assert_eq!(physical(&window(&mut app)), before, "a borderless window is never resized");
    assert!(!app.world().resource::<DisplayConfirm>().is_pending(), "nothing on screen changed");
    let applied = seen::<DisplaySettingsApplied>(&mut app);
    assert_eq!(applied.len(), 1);
    assert!(!applied[0].needs_confirm);
}

#[test]
fn vsync_applies_at_once_without_a_countdown() {
    let mut app = app();
    assert_eq!(window(&mut app).present_mode, PresentMode::AutoVsync);
    send(&mut app, ApplyDisplaySettings(with(|s| s.vsync = false)));
    assert_eq!(window(&mut app).present_mode, PresentMode::AutoNoVsync);
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
}

#[cfg(feature = "shadows")]
#[test]
fn shadows_apply_to_every_light_at_once_and_to_new_lights() {
    use bevy::light::DirectionalLight;
    let mut app = app();
    let light = app.world_mut().spawn(DirectionalLight::default()).id();
    app.update();
    assert!(app.world().get::<DirectionalLight>(light).unwrap().shadow_maps_enabled, "a new light gets the setting");
    send(&mut app, ApplyDisplaySettings(with(|s| s.shadows = false)));
    assert!(!app.world().get::<DirectionalLight>(light).unwrap().shadow_maps_enabled);
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
    let late = app.world_mut().spawn(DirectionalLight { shadow_maps_enabled: true, ..default() }).id();
    app.update();
    assert!(!app.world().get::<DirectionalLight>(late).unwrap().shadow_maps_enabled);
}

#[cfg(feature = "shadows")]
#[test]
fn shadows_can_be_left_to_the_game() {
    use bevy::light::DirectionalLight;
    let (mut app, _) = app_with(DisplaySettingsPlugin { manage_shadows: false, log_diagnostics: false, ..default() });
    let light = app.world_mut().spawn(DirectionalLight::default()).id();
    app.update();
    assert!(!app.world().get::<DirectionalLight>(light).unwrap().shadow_maps_enabled);
}

// ---------------------------------------------------------------- the countdown

fn to_windowed(app: &mut App) {
    send(
        app,
        ApplyDisplaySettings(with(|s| {
            s.mode = DisplayMode::Windowed;
            s.resolution = (1280, 720);
        })),
    );
}

#[test]
fn a_mode_change_waits_for_keep() {
    let mut app = app();
    to_windowed(&mut app);
    assert_eq!(seen::<DisplayConfirmPending>(&mut app), vec![DisplayConfirmPending { secs_left: 10 }]);
    let confirm = app.world().resource::<DisplayConfirm>();
    assert_eq!(confirm.secs_left(), Some(10));
    assert_eq!(confirm.previous().map(|p| p.mode), Some(DisplayMode::Borderless));
    send(&mut app, KeepDisplaySettings);
    assert_eq!(seen::<DisplayKept>(&mut app).len(), 1);
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
    for _ in 0..150 {
        app.update();
    }
    assert!(seen::<DisplayReverted>(&mut app).is_empty(), "a kept change never reverts");
    assert_eq!(settings(&app).mode, DisplayMode::Windowed);
}

#[test]
fn revert_goes_back_to_the_settings_before_the_change() {
    let mut app = app();
    to_windowed(&mut app);
    // A second change during the countdown keeps the ORIGINAL confirmed settings to go back to.
    send(&mut app, ApplyDisplaySettings(with(|s| s.mode = DisplayMode::Fullscreen)));
    send(&mut app, RevertDisplaySettings);
    let reverted = seen::<DisplayReverted>(&mut app);
    assert_eq!(reverted.len(), 1);
    assert_eq!(reverted[0].reason, RevertReason::Requested);
    assert_eq!(settings(&app), DisplaySettings::default());
    assert_eq!(display_mode_of(window(&mut app).mode), DisplayMode::Borderless);
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
}

#[test]
fn no_answer_reverts_after_the_timeout_counting_down_each_second() {
    let mut app = app();
    to_windowed(&mut app);
    let mut frames = 0;
    while seen_reverted_len(&mut app) == 0 {
        app.update();
        frames += 1;
        assert!(frames < 200, "never reverted");
    }
    // 10 s of 100 ms frames.
    assert_eq!(frames, 100);
    let secs: Vec<u32> = seen::<DisplayConfirmPending>(&mut app).into_iter().map(|p| p.secs_left).collect();
    assert_eq!(secs, (1..=10).rev().collect::<Vec<_>>());
    assert_eq!(settings(&app).mode, DisplayMode::Borderless);
    assert_eq!(display_mode_of(window(&mut app).mode), DisplayMode::Borderless);
}

fn seen_reverted_len(app: &mut App) -> usize {
    let r = app.world().resource::<Seen<DisplayReverted>>();
    if let Some(last) = r.0.last() {
        assert_eq!(last.reason, RevertReason::Timeout);
    }
    r.0.len()
}

#[test]
fn the_countdown_length_is_configurable_and_zero_turns_it_off() {
    let (mut app, _) = app_with(DisplaySettingsPlugin { confirm_timeout: Duration::from_secs(3), log_diagnostics: false, ..default() });
    to_windowed(&mut app);
    assert_eq!(seen::<DisplayConfirmPending>(&mut app), vec![DisplayConfirmPending { secs_left: 3 }]);

    let (mut app, _) = app_with(DisplaySettingsPlugin { confirm_timeout: Duration::ZERO, log_diagnostics: false, ..default() });
    to_windowed(&mut app);
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
    assert!(seen::<DisplayConfirmPending>(&mut app).is_empty());
}

#[test]
fn going_back_to_the_confirmed_mode_ends_the_countdown() {
    let mut app = app();
    to_windowed(&mut app);
    send(&mut app, ApplyDisplaySettings(DisplaySettings::default()));
    assert!(!app.world().resource::<DisplayConfirm>().is_pending());
}

// ---------------------------------------------------------------- save / discard

#[test]
fn save_writes_the_file_atomically_and_discard_returns_to_it() {
    let path = tmp_file("save_discard.ron");
    let (mut app, _) = app_with(DisplaySettingsPlugin { file: Some(path.clone()), log_diagnostics: false, ..default() });
    send(&mut app, ApplyDisplaySettings(with(|s| s.vsync = false)));
    send(&mut app, SaveDisplaySettings);
    assert_eq!(seen::<DisplaySettingsSaved>(&mut app).len(), 1);
    assert!(!temp_path(&path).exists(), "no temporary file left behind");
    assert!(!DisplaySettings::load(&path).vsync);

    send(
        &mut app,
        ApplyDisplaySettings(with(|s| {
            s.vsync = false;
            s.shadows = false;
        })),
    );
    send(&mut app, DiscardDisplayChanges);
    let reverted = seen::<DisplayReverted>(&mut app);
    assert_eq!(reverted.len(), 1);
    assert_eq!(reverted[0].reason, RevertReason::Discarded);
    assert!(settings(&app).shadows && !settings(&app).vsync, "back to the saved settings");
}

#[test]
fn saving_during_the_countdown_writes_the_confirmed_mode_until_it_is_kept() {
    let path = tmp_file("save_pending.ron");
    let (mut app, _) = app_with(DisplaySettingsPlugin { file: Some(path.clone()), log_diagnostics: false, ..default() });
    to_windowed(&mut app);
    send(&mut app, SaveDisplaySettings);
    assert_eq!(DisplaySettings::load(&path).mode, DisplayMode::Borderless, "an unconfirmed mode is never on disk");
    send(&mut app, KeepDisplaySettings);
    assert_eq!(DisplaySettings::load(&path).mode, DisplayMode::Windowed, "saved again once kept");
    assert_eq!(seen::<DisplaySettingsSaved>(&mut app).len(), 2);
}

#[test]
fn a_failed_save_is_reported_and_changes_nothing() {
    // A directory where the file should be: the rename fails.
    let path = tmp_file("is_a_directory.ron");
    std::fs::create_dir_all(&path).expect("dir");
    let (mut app, _) = app_with(DisplaySettingsPlugin { file: Some(path.clone()), log_diagnostics: false, ..default() });
    send(&mut app, ApplyDisplaySettings(with(|s| s.vsync = false)));
    send(&mut app, SaveDisplaySettings);
    assert_eq!(seen::<DisplaySaveFailed>(&mut app).len(), 1);
    assert!(seen::<DisplaySettingsSaved>(&mut app).is_empty());
    assert_eq!(app.world().resource::<SavedDisplaySettings>().0, DisplaySettings::default());
    assert!(!temp_path(&path).exists());
}

#[test]
fn without_a_file_save_only_moves_the_baseline() {
    let mut app = app();
    send(&mut app, ApplyDisplaySettings(with(|s| s.shadows = false)));
    send(&mut app, SaveDisplaySettings);
    let saved = seen::<DisplaySettingsSaved>(&mut app);
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].path, None);
    assert!(!app.world().resource::<SavedDisplaySettings>().0.shadows);
}

#[test]
fn the_plugin_loads_its_file_and_a_broken_one_gives_defaults() {
    let path = tmp_file("load.ron");
    std::fs::write(&path, "(display_mode: Exclusive, vsync: false)").expect("write");
    let plugin = DisplaySettingsPlugin::from_file(&path);
    assert_eq!(plugin.settings.mode, DisplayMode::Fullscreen);
    assert!(!plugin.settings.vsync);

    std::fs::write(&path, "(vsync: fals").expect("write");
    let loaded = load_ron_or_default::<DisplaySettings>(&path);
    assert!(matches!(loaded.status, LoadStatus::Invalid(_)));
    assert_eq!(loaded.value, DisplaySettings::default());

    let missing = load_ron_or_default::<DisplaySettings>(tmp_file("missing.ron"));
    assert_eq!(missing.status, LoadStatus::Missing);
}

#[test]
fn an_atomic_save_round_trips_and_replaces_an_existing_file() {
    let path = tmp_file("nested/dir/round_trip.ron");
    let a = DisplaySettings { renderer: RendererBackend::Vulkan, mode: DisplayMode::Windowed, resolution: (1600, 900), vsync: false, shadows: false };
    save_ron_atomic(&path, &a).expect("save");
    assert_eq!(load_ron_or_default::<DisplaySettings>(&path), Loaded { value: a.clone(), status: LoadStatus::Loaded });
    let b = DisplaySettings { vsync: true, ..a };
    b.save(&path).expect("save over");
    assert_eq!(DisplaySettings::load(&path), b);
    assert!(!temp_path(&path).exists());
}

// ---------------------------------------------------------------- renderer + restart

#[test]
fn a_renderer_change_requires_a_restart_once() {
    let mut app = app();
    assert!(!app.world().resource::<RendererState>().restart_required());
    send(&mut app, ApplyDisplaySettings(with(|s| s.renderer = RendererBackend::Vulkan)));
    assert_eq!(seen::<RestartRequired>(&mut app), vec![RestartRequired { renderer: RendererBackend::Vulkan }]);
    assert_eq!(app.world().resource::<RendererState>().pending(), Some(RendererBackend::Vulkan));
    assert!(!app.world().resource::<DisplayConfirm>().is_pending(), "a renderer change is not a live change");
    // The same again: nothing new.
    send(
        &mut app,
        ApplyDisplaySettings(with(|s| {
            s.renderer = RendererBackend::Vulkan;
            s.vsync = false;
        })),
    );
    assert!(seen::<RestartRequired>(&mut app).is_empty());
    // Back to what runs: no restart needed.
    send(&mut app, ApplyDisplaySettings(DisplaySettings::default()));
    assert!(!app.world().resource::<RendererState>().restart_required());
    assert!(seen::<RestartRequired>(&mut app).is_empty());
}

#[test]
fn the_running_renderer_comes_from_the_startup_choice() {
    let choice = RendererChoice::resolve(RendererBackend::Auto, &["--dx12".to_string()], None, &RendererFlags::default(), Platform::Windows);
    let (app, _) = app_with(DisplaySettingsPlugin { renderer: Some(choice), log_diagnostics: false, ..default() });
    let state = app.world().resource::<RendererState>();
    assert_eq!(state.choice().source, RendererSource::LaunchFlag);
    assert_eq!(state.pending(), Some(RendererBackend::Auto), "the saved setting differs from the forced one");
}

#[test]
fn restart_saves_first_then_relaunches_then_exits() {
    let path = tmp_file("restart.ron");
    let (mut app, launches) = app_with(DisplaySettingsPlugin { file: Some(path.clone()), log_diagnostics: false, ..default() });
    send(&mut app, ApplyDisplaySettings(with(|s| s.renderer = RendererBackend::Vulkan)));
    send(&mut app, RequestRestart);
    assert_eq!(DisplaySettings::load(&path).renderer, RendererBackend::Vulkan);
    assert_eq!(launches.load(Ordering::SeqCst), 1);
    assert_eq!(seen::<AppExit>(&mut app), vec![AppExit::Success]);
}

#[test]
fn a_failed_relaunch_keeps_the_game_running() {
    let mut app = app();
    app.insert_resource(Relauncher::new(|| Err(std::io::Error::other("no exe"))));
    send(&mut app, RequestRestart);
    assert_eq!(seen::<RestartFailed>(&mut app).len(), 1);
    assert!(seen::<AppExit>(&mut app).is_empty());
}

#[test]
fn a_failed_save_stops_the_restart_before_it_relaunches() {
    let path = tmp_file("restart_dir.ron");
    std::fs::create_dir_all(&path).expect("dir");
    let (mut app, launches) = app_with(DisplaySettingsPlugin { file: Some(path), log_diagnostics: false, ..default() });
    send(&mut app, RequestRestart);
    assert_eq!(seen::<RestartFailed>(&mut app).len(), 1);
    assert_eq!(launches.load(Ordering::SeqCst), 0);
    assert!(seen::<AppExit>(&mut app).is_empty());
}

// ---------------------------------------------------------------- diagnostics

#[test]
fn diagnostics_come_once_after_startup_and_again_after_a_change() {
    let (mut app, _) = app_with(DisplaySettingsPlugin { diagnostics_after_frames: 3, log_diagnostics: false, ..default() });
    for _ in 0..10 {
        app.update();
    }
    let first = seen::<DisplayDiagnostics>(&mut app);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].mode, DisplayMode::Borderless);
    assert_eq!(first[0].monitors[0].refresh_millihertz, Some(60_000));
    assert!(first[0].line().starts_with(">>> DISPLAY: mode Borderless"));

    send(&mut app, ApplyDisplaySettings(with(|s| s.vsync = false)));
    for _ in 0..10 {
        app.update();
    }
    let second = seen::<DisplayDiagnostics>(&mut app);
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].present_mode, PresentMode::AutoNoVsync);
}

#[test]
fn the_plugin_runs_without_a_window() {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, DisplaySettingsPlugin { log_diagnostics: false, ..default() }));
    app.edit_schedule(Update, strict);
    app.world_mut().write_message(ApplyDisplaySettings(with(|s| s.mode = DisplayMode::Windowed)));
    for _ in 0..20 {
        app.update();
    }
    assert_eq!(app.world().resource::<DisplaySettings>().mode, DisplayMode::Windowed);
}
