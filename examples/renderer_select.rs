//! The renderer choice, made before the app is built.
//!
//! ```text
//! cargo run --example renderer_select                # the saved setting (Auto on first run)
//! cargo run --example renderer_select -- --vulkan    # a launch flag wins for this run
//! WGPU_BACKEND=dx12 cargo run --example renderer_select
//! ```
//!
//! Prints what was chosen and why, opens a window for two seconds and exits. The choice is made
//! from the saved setting, `--vulkan` / `--dx12` and `WGPU_BACKEND`, in that order of weakness.

use std::time::Duration;

use bevy::prelude::*;
use bevy_display_settings::*;

fn main() {
    let settings = DisplaySettings::load("display_settings.ron");
    let choice = RendererChoice::from_process(settings.renderer, &RendererFlags::default());
    println!("{}", choice.describe());
    println!("requested {:?}, forced {:?}, source {:?}, platform {:?}", choice.requested, choice.forced, choice.source, choice.platform);
    println!("offered on this platform: {:?}", RendererBackend::offered_on(Platform::current()));

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(choice.render_plugin())
                .set(WindowPlugin { primary_window: Some(primary_window(&settings, "bevy_display_settings: renderer_select")), ..default() }),
        )
        .add_plugins(DisplaySettingsPlugin { settings, renderer: Some(choice), ..default() })
        .add_systems(Startup, |mut commands: Commands| {
            commands.spawn(Camera2d);
        })
        .add_systems(Update, quit_after_two_seconds)
        .run();
}

fn quit_after_two_seconds(
    time: Res<Time>,
    mut exit: MessageWriter<AppExit>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut printed: Local<bool>,
) {
    if let (Some(adapter), false) = (adapter, *printed) {
        println!("running on {:?} ({})", adapter.backend, adapter.name);
        *printed = true;
    }
    if time.elapsed() > Duration::from_secs(2) {
        exit.write(AppExit::Success);
    }
}
