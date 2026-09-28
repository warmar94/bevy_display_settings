//! The whole flow with keyboard controls and a text readout (any UI works the same way: send
//! requests, read the state and the facts).
//!
//! ```text
//! cargo run --example quick_start
//! ```
//!
//! Keys: 1 / 2 / 3 = Windowed / Borderless / Fullscreen, R = next windowed resolution,
//! V = vsync, H = shadows, B = next renderer, K = keep, N = revert, Enter = save,
//! Backspace = discard unsaved changes, F5 = restart now. The settings file is
//! `display_settings.ron` in the working directory.

use bevy::prelude::*;
use bevy_display_settings::*;

const FILE: &str = "display_settings.ron";

fn main() {
    // Before the app: load the settings, choose the renderer, describe the first window.
    let plugin = DisplaySettingsPlugin::from_file(FILE);
    let renderer = RendererChoice::from_process(plugin.settings.renderer, &RendererFlags::default());
    println!("{}", renderer.describe());
    let window = primary_window(&plugin.settings, "bevy_display_settings: quick_start");

    App::new()
        .add_plugins(DefaultPlugins.set(renderer.render_plugin()).set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(DisplaySettingsPlugin { renderer: Some(renderer), ..plugin })
        .init_resource::<Log>()
        .add_systems(Startup, setup)
        .add_systems(Update, controls.before(DisplaySettingsSystems::Requests))
        .add_systems(Update, (log_facts, readout).chain().after(DisplaySettingsSystems::Report))
        .run();
}

#[derive(Component)]
struct Readout;

/// The last few facts, for the readout.
#[derive(Resource, Default)]
struct Log(Vec<String>);

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    commands.spawn((Camera3d::default(), Transform::from_xyz(-4.0, 4.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y)));
    // The plugin sets `shadow_maps_enabled` on every directional light from the `shadows` setting.
    commands.spawn((DirectionalLight { illuminance: 8_000.0, ..default() }, Transform::from_xyz(3.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((Mesh3d(meshes.add(Plane3d::default().mesh().size(12.0, 12.0))), MeshMaterial3d(materials.add(Color::srgb(0.35, 0.45, 0.3)))));
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(1.5, 1.5, 1.5))),
        MeshMaterial3d(materials.add(Color::srgb(0.8, 0.5, 0.3))),
        Transform::from_xyz(0.0, 0.75, 0.0),
    ));
    commands.spawn((
        Text::new(""),
        TextFont { font_size: FontSize::Px(18.0), ..default() },
        Node { position_type: PositionType::Absolute, top: Val::Px(12.0), left: Val::Px(12.0), ..default() },
        Readout,
    ));
}

fn next<T: Copy + PartialEq>(list: &[T], current: T) -> T {
    let i = list.iter().position(|x| *x == current).map_or(0, |i| i + 1);
    list.get(i % list.len().max(1)).copied().unwrap_or(current)
}

#[allow(clippy::too_many_arguments)]
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Res<DisplaySettings>,
    info: Res<DisplayInfo>,
    mut apply: MessageWriter<ApplyDisplaySettings>,
    mut keep: MessageWriter<KeepDisplaySettings>,
    mut revert: MessageWriter<RevertDisplaySettings>,
    mut save: MessageWriter<SaveDisplaySettings>,
    mut discard: MessageWriter<DiscardDisplayChanges>,
    mut restart: MessageWriter<RequestRestart>,
) {
    let mut s = settings.clone();
    let mut changed = true;
    if keys.just_pressed(KeyCode::Digit1) {
        s.mode = DisplayMode::Windowed;
    } else if keys.just_pressed(KeyCode::Digit2) {
        s.mode = DisplayMode::Borderless;
    } else if keys.just_pressed(KeyCode::Digit3) {
        s.mode = DisplayMode::Fullscreen;
    } else if keys.just_pressed(KeyCode::KeyR) && !info.windowed_resolutions.is_empty() {
        s.resolution = next(&info.windowed_resolutions, s.resolution);
    } else if keys.just_pressed(KeyCode::KeyV) {
        s.vsync = !s.vsync;
    } else if keys.just_pressed(KeyCode::KeyH) {
        s.shadows = !s.shadows;
    } else if keys.just_pressed(KeyCode::KeyB) {
        s.renderer = next(RendererBackend::offered_on(Platform::current()), s.renderer);
    } else {
        changed = false;
    }
    if changed {
        apply.write(ApplyDisplaySettings(s));
    }
    if keys.just_pressed(KeyCode::KeyK) {
        keep.write(KeepDisplaySettings);
    }
    if keys.just_pressed(KeyCode::KeyN) {
        revert.write(RevertDisplaySettings);
    }
    if keys.just_pressed(KeyCode::Enter) {
        save.write(SaveDisplaySettings);
    }
    if keys.just_pressed(KeyCode::Backspace) {
        discard.write(DiscardDisplayChanges);
    }
    if keys.just_pressed(KeyCode::F5) {
        restart.write(RequestRestart);
    }
}

#[allow(clippy::too_many_arguments)]
fn log_facts(
    mut log: ResMut<Log>,
    mut kept: MessageReader<DisplayKept>,
    mut reverted: MessageReader<DisplayReverted>,
    mut saved: MessageReader<DisplaySettingsSaved>,
    mut save_failed: MessageReader<DisplaySaveFailed>,
    mut restart: MessageReader<RestartRequired>,
    mut restart_failed: MessageReader<RestartFailed>,
    mut diagnostics: MessageReader<DisplayDiagnostics>,
) {
    let mut lines = Vec::new();
    lines.extend(kept.read().map(|_| "kept".to_string()));
    lines.extend(reverted.read().map(|r| format!("reverted ({:?})", r.reason)));
    lines.extend(saved.read().map(|s| format!("saved to {:?}", s.path)));
    lines.extend(save_failed.read().map(|f| format!("save failed: {}", f.error)));
    lines.extend(restart.read().map(|r| format!("{} applies after a restart (F5)", r.renderer.label())));
    lines.extend(restart_failed.read().map(|f| format!("restart failed: {}", f.error)));
    lines.extend(diagnostics.read().map(|d| d.line()));
    for line in lines {
        println!("{line}");
        log.0.push(line);
    }
    let excess = log.0.len().saturating_sub(6);
    log.0.drain(..excess);
}

fn readout(
    settings: Res<DisplaySettings>,
    confirm: Res<DisplayConfirm>,
    renderer: Res<RendererState>,
    info: Res<DisplayInfo>,
    log: Res<Log>,
    mut text: Query<&mut Text, With<Readout>>,
) {
    let Ok(mut text) = text.single_mut() else { return };
    let monitor = info.current_monitor().map_or("unknown".to_string(), |m| format!("{} {}x{}", m.name, m.screen.width, m.screen.height));
    let mut s = format!(
        "mode      {}   (1 / 2 / 3)\nwindowed  {}x{}   (R; used only in Windowed)\nvsync     {}   (V)\nshadows   {}   (H)\nrenderer  {}   (B)\nmonitor   {monitor}\n\n",
        settings.mode.label(),
        settings.resolution.0,
        settings.resolution.1,
        settings.vsync,
        settings.shadows,
        settings.renderer.label(),
    );
    if let Some(secs) = confirm.secs_left() {
        s.push_str(&format!("KEEP THESE SETTINGS? Reverting in {secs} s.  K = keep, N = revert\n"));
    }
    if let Some(r) = renderer.pending() {
        s.push_str(&format!("RESTART REQUIRED for {}.  F5 = restart now\n", r.label()));
    }
    s.push_str("Enter = save, Backspace = discard unsaved changes\n\n");
    for line in &log.0 {
        s.push_str(line);
        s.push('\n');
    }
    if text.0 != s {
        text.0 = s;
    }
}
