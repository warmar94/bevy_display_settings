# bevy_display_settings

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![CI](https://github.com/warmar94/bevy_display_settings/actions/workflows/ci.yml/badge.svg)](https://github.com/warmar94/bevy_display_settings/actions/workflows/ci.yml)
[![Bevy 0.20.0](https://img.shields.io/badge/Bevy-0.20.0-informational)](https://bevyengine.org)

Safe, **UI-agnostic display and renderer settings for [Bevy](https://bevyengine.org)**: the logic
behind a game's graphics menu, without the menu.

- Choose the renderer backend (**DX12 / Vulkan / Auto**) *before* the app is built, with launch
  flags and `WGPU_BACKEND` as rescue paths, and restart the game to change it.
- **Windowed / Borderless / Fullscreen**, the monitor's resolution list, **vsync** and
  **shadows**, applied live.
- A **windowed-size clamp** that prevents a real crash, a **"keep these settings?" countdown**
  that reverts on its own, a one-line **display diagnostic**, and a **settings file** that never
  panics, never leaves a half-written file and keeps loading after you rename things.

Your game draws the menu with whatever UI it uses. It sends requests (`ApplyDisplaySettings`,
`KeepDisplaySettings`, `SaveDisplaySettings`, ...) and reads state (`DisplaySettings`,
`DisplayConfirm`, `DisplayInfo`, `RendererState`) and facts (`DisplayConfirmPending`,
`DisplayReverted`, `RestartRequired`, ...). The crate draws nothing.

## Contents

- [Highlights](#highlights)
- [Quick start](#quick-start)
- [How to use it](#how-to-use-it)
  - [1. Before the app: renderer and first window](#1-before-the-app-renderer-and-first-window)
  - [2. Add the plugin](#2-add-the-plugin)
  - [3. Build your menu](#3-build-your-menu)
  - [4. Keep or revert](#4-keep-or-revert)
  - [5. Save, discard, restart](#5-save-discard-restart)
  - [6. Your own settings file](#6-your-own-settings-file)
  - [7. Renaming a setting safely](#7-renaming-a-setting-safely)
  - [8. Diagnostics](#8-diagnostics)
  - [9. System order](#9-system-order)
  - [10. Testing your game](#10-testing-your-game)
- [How it works](#how-it-works)
- [API reference](#api-reference)
- [Cargo features](#cargo-features)
- [Compatibility](#compatibility)
- [Examples](#examples)
- [Limitations and FAQ](#limitations-and-faq)
- [License](#license)
- [Contributing](#contributing)

## Highlights

- **UI-agnostic**: requests in, state and facts out. Works with Bevy UI, egui, a console, or a
  gamepad-only menu.
- **Renderer choice before startup**: `RendererChoice` decides from a launch flag (`--vulkan`,
  `--dx12`, configurable), then `WGPU_BACKEND`, then the saved setting, never forces a backend
  the platform does not have, and gives you the `RenderPlugin`. A change at runtime raises
  `RestartRequired`; `RequestRestart` saves, starts a new copy of the game with the same
  arguments and exits.
- **The windowed-fit clamp**: a windowed client area as large as the monitor is silently shrunk by
  the OS while Bevy keeps the requested size; wgpu then rejects the frame ("Attachments have
  differing sizes") and the app exits. Every windowed size goes through `windowed_fit`, which
  keeps room for the title bar, frame and taskbar.
- **The resolution applies only in Windowed**: borderless and exclusive fullscreen take the
  monitor's size; writing a resolution there is the same crash.
- **Safety net**: a mode or resolution change starts a countdown (10 s by default). Keep it, revert
  it, or do nothing and it reverts.
- **Borderless by default**: the mode that works on every system. Exclusive fullscreen is the only
  mode that can run above the monitor's refresh rate; the docs say so.
- **Diagnostics**: `>>> DISPLAY: mode Borderless, window 1920x1080 ... | monitors: "..." 1920x1080
  @60.0 Hz (primary)`, logged and sent as a message a few frames after startup and after every
  change.
- **Settings file**: `#[serde(default)]`, missing or broken file = defaults + a warning, atomic save
  (temp file + rename), Cancel returns to the saved values, and a test helper that proves an old
  file still loads.
- **Headless-tested**, with strict ambiguity detection; no window or GPU needed in tests.

## Quick start

```toml
[dependencies]
bevy = "0.20.0"
bevy_display_settings = { git = "https://github.com/warmar94/bevy_display_settings", tag = "v0.2.0" }
# or, once published: bevy_display_settings = "0.2"
```

```rust,no_run
use bevy::prelude::*;
use bevy_display_settings::*;

fn main() {
    // 1. Before the app: load the settings, pick the renderer, describe the first window.
    let plugin = DisplaySettingsPlugin::from_file("settings.ron");
    let renderer = RendererChoice::from_process(plugin.settings.renderer, &RendererFlags::default());
    println!("{}", renderer.describe()); // >>> RENDERER: Auto (wgpu decides) (from settings; ...)
    let window = primary_window(&plugin.settings, "My Game");

    // 2. The app.
    App::new()
        .add_plugins(DefaultPlugins.set(renderer.render_plugin()).set(WindowPlugin { primary_window: Some(window), ..default() }))
        .add_plugins(DisplaySettingsPlugin { renderer: Some(renderer), ..plugin })
        .add_systems(Update, go_windowed.before(DisplaySettingsSystems::Requests))
        .run();
}

// 3. Your menu sends requests.
fn go_windowed(keys: Res<ButtonInput<KeyCode>>, settings: Res<DisplaySettings>, mut apply: MessageWriter<ApplyDisplaySettings>) {
    if keys.just_pressed(KeyCode::F11) {
        let mut next = settings.clone();
        next.mode = DisplayMode::Windowed;
        apply.write(ApplyDisplaySettings(next)); // starts the 10 s keep-or-revert countdown
    }
}
```

`cargo run --example quick_start` shows the whole flow with keyboard controls and a text readout.

## How to use it

Everything below uses `use bevy::prelude::*; use bevy_display_settings::*;`.

### 1. Before the app: renderer and first window

The renderer backend is chosen when the renderer is created, inside `DefaultPlugins`, so it has
to be decided before the app is built. Load the settings first, then decide:

```rust,no_run
use bevy::prelude::*;
use bevy_display_settings::*;

let settings = DisplaySettings::load("settings.ron"); // missing or broken: defaults, never panics
let renderer = RendererChoice::from_process(settings.renderer, &RendererFlags::default());
println!("{}", renderer.describe()); // no log subscriber exists yet: print it

let mut app = App::new();
app.add_plugins(
    DefaultPlugins
        .set(renderer.render_plugin())
        .set(WindowPlugin { primary_window: Some(primary_window(&settings, "My Game")), ..default() }),
);
```

Precedence, strongest first:

| source | example | `RendererChoice::source` |
|---|---|---|
| a launch flag (the last one given wins) | `mygame --vulkan` | `LaunchFlag` |
| the `WGPU_BACKEND` environment variable (non-empty) | `WGPU_BACKEND=dx12 mygame` | `Environment` (wgpu reads it itself) |
| the saved setting | `renderer: Vulkan` in the file | `Settings` |

The flags and the variable are rescue paths: a backend that will not start cannot be changed from
a menu you cannot reach. The flags are configurable (`RendererFlags { vulkan, dx12 }`,
`RendererFlags::none()`).

What is actually forced depends on the platform; a backend the platform does not have is never
forced, so it can never stop the game starting:

| | `Auto` | `Vulkan` | `Dx12` |
|---|---|---|---|
| Windows | wgpu decides | Vulkan | DirectX 12 |
| Linux | wgpu decides | Vulkan | wgpu decides |
| macOS / other | wgpu decides | wgpu decides | wgpu decides |

`RendererBackend::offered_on(Platform::current())` lists the choices worth showing in a menu.
Already customising `WgpuSettings`? Call `renderer.apply_to(&mut your_wgpu_settings)` instead of
`render_plugin()`.

`primary_window` gives the first window: the mode, vsync, and in Windowed a size capped to
`UNKNOWN_MONITOR_WINDOW` (1280x720), because no monitor is known yet. The plugin re-fits it to the
setting as soon as the monitor is reported.

### 2. Add the plugin

```rust
use bevy::prelude::*;
use bevy_display_settings::*;
use std::time::Duration;

let mut app = App::new();
app.add_plugins((
    MinimalPlugins,
    DisplaySettingsPlugin {
        file: Some("settings.ron".into()),
        confirm_timeout: Duration::from_secs(15),
        ..default()
    },
));
```

| field | default | meaning |
|---|---|---|
| `settings` | `DisplaySettings::default()` | the settings to start with (`from_file` loads them) |
| `file` | `None` | where `SaveDisplaySettings` and `RequestRestart` write. `None`: nothing is written, you persist `SavedDisplaySettings` yourself |
| `renderer` | `None` | the `RendererChoice` made before the app. `None`: assumed from `settings.renderer` |
| `renderer_flags` | `--vulkan`, `--dx12` | stripped from the arguments of a relaunch |
| `confirm_timeout` | 10 s | the keep-or-revert countdown. `Duration::ZERO` turns the safety net off |
| `diagnostics_after_frames` | 10 | frames before `DisplayDiagnostics`, after startup and after each change |
| `log_diagnostics` | `true` | log the diagnostics line at `info` |
| `manage_shadows` | `true` | apply `shadows` to directional lights (feature `shadows`) |

`DisplaySettingsPlugin::from_file(path)` = load `path` into `settings` and save back to it.

The settings:

| field | default | applies |
|---|---|---|
| `renderer: RendererBackend` | `Auto` | at the next start (`RestartRequired`) |
| `mode: DisplayMode` | `Borderless` | live, with the countdown |
| `resolution: (u32, u32)` | 1280x720 | live in `Windowed` only (with the countdown), physical pixels, fitted to the monitor |
| `vsync: bool` | `true` | live (`AutoVsync` / `AutoNoVsync`) |
| `shadows: bool` | `true` | live, on every `DirectionalLight` (feature `shadows`) |

Display modes: `Windowed` (a normal window; the only mode that uses `resolution`), `Borderless`
(the default: a borderless window over the current monitor), `Fullscreen` (exclusive, at the
monitor's current video mode).

### 3. Build your menu

Read the live state, send a whole `DisplaySettings` back:

```rust
use bevy::prelude::*;
use bevy_display_settings::*;

/// A menu row: "Resolution < 1600x900 >".
fn next_resolution(settings: Res<DisplaySettings>, info: Res<DisplayInfo>, mut apply: MessageWriter<ApplyDisplaySettings>) {
    let list = &info.windowed_resolutions; // fits as a window; never the monitor's own size
    let i = list.iter().position(|r| *r == settings.resolution).map_or(0, |i| (i + 1) % list.len().max(1));
    if let Some(res) = list.get(i) {
        apply.write(ApplyDisplaySettings(DisplaySettings { resolution: *res, ..settings.clone() }));
    }
}
```

- `DisplayInfo`: `monitors` (name, size, scale, refresh rate, primary, video modes), `current`
  (the monitor the window is on), `windowed_resolutions` (deduplicated, sorted, only sizes that
  fit as a window). Empty until the windowing backend reports a monitor (a few frames in).
- Grey out the resolution row outside `Windowed` (`resolution_applies(mode)`); a resolution
  change there is stored but changes nothing on screen.
- Offer `RendererBackend::offered_on(Platform::current())` and show "applies after a restart"
  while `RendererState::restart_required()`.
- Labels: `DisplayMode::label()`, `RendererBackend::label()` (ASCII).
- `DisplaySettings` is the live state. Treat it as read-only: changing it directly applies without
  the countdown and without the renderer check.

### 4. Keep or revert

A change of mode, or of the windowed resolution, starts the countdown. Vsync, shadows and the
renderer never do.

```rust
use bevy::prelude::*;
use bevy_display_settings::*;

fn confirm_dialog(
    confirm: Res<DisplayConfirm>,
    keys: Res<ButtonInput<KeyCode>>,
    mut keep: MessageWriter<KeepDisplaySettings>,
    mut revert: MessageWriter<RevertDisplaySettings>,
) {
    let Some(secs) = confirm.secs_left() else { return }; // no dialog
    // Draw it your way: "Keep these settings? Reverting in {secs} s"  [Keep] [Revert]
    info!("Keep these settings? Reverting in {secs} s");
    if keys.just_pressed(KeyCode::Enter) {
        keep.write(KeepDisplaySettings);
    } else if keys.just_pressed(KeyCode::Escape) {
        revert.write(RevertDisplaySettings);
    }
}
```

- `DisplayConfirmPending { secs_left }` is written when the countdown starts and each time the
  whole number of seconds changes; `DisplayConfirm` has the same information at any time
  (`is_pending`, `secs_left`, `remaining`, `previous`).
- `KeepDisplaySettings` -> `DisplayKept`. `RevertDisplaySettings`, or no answer ->
  `DisplayReverted { reason: Requested | Timeout, settings }`: the settings from **before** the
  change are live again. Several changes during one countdown restart it and still revert to the
  last confirmed settings; applying the confirmed mode and size again ends it.
- The countdown runs on real time, so it keeps going while your game's virtual time is paused.

### 5. Save, discard, restart

| request | effect | facts |
|---|---|---|
| `SaveDisplaySettings` | write the file (when the plugin has one) and make the settings the baseline | `DisplaySettingsSaved { path, settings }` or `DisplaySaveFailed { path, error }` |
| `DiscardDisplayChanges` | "Cancel": apply the saved settings again, no countdown | `DisplayReverted { reason: Discarded, .. }` |
| `RequestRestart` | save, start a new copy of the game, exit | `AppExit::Success`, or `RestartFailed { error }` and the game keeps running |

- During a countdown, a save writes the **confirmed** mode and resolution, so a crash never boots
  into an unconfirmed mode. When the change is kept, the save is repeated.
- A renderer change writes `RestartRequired { renderer }` (once per new target) and shows in
  `RendererState` (`restart_required()`, `pending()`, `choice()`). Show "Restart now / Later";
  "Later" needs nothing: the next start picks up the saved setting.
- `RequestRestart` uses the `Relauncher` resource: by default `relaunch(&flags)`, which starts
  `current_exe()` with this run's arguments minus the renderer flags, without `WGPU_BACKEND`, in
  the same working directory. Replace it (`Relauncher::new(|| ...)`) for a launcher or in tests.
- With `file: None`, save your own file on `DisplaySettingsSaved` **before** sending
  `RequestRestart` (or in the same frame, before `DisplaySettingsSystems::Requests`).

### 6. Your own settings file

Most games keep display settings inside a bigger settings file. The file helpers are generic:

```rust,no_run
use bevy_display_settings::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
struct GameSettings {
    display: DisplaySettings,
    master_volume: f32,
}

let loaded = load_ron_or_default::<GameSettings>("settings.ron");
if let LoadStatus::Invalid(error) = &loaded.status {
    eprintln!("settings reset to defaults: {error}");
}
let settings = loaded.value;
// ... later, e.g. on DisplaySettingsSaved:
if let Err(e) = save_ron_atomic("settings.ron", &settings) {
    eprintln!("could not save: {e}");
}
```

- `load_ron_or_default` / `parse_ron_or_default`: `Loaded { value, status }` with
  `LoadStatus::{Loaded, Missing, Unreadable, Invalid}`. Missing is silent (a first run); the others
  log a warning. Never panics.
- `save_ron_atomic`: pretty RON to `<path>.tmp`, then a rename over `path` (creates the folder;
  removes the temp file on failure).
- `path_next_to_exe("settings.ron")`: the usual home of a player's file in a shipped game.
- A player's settings file is written by the game and lives on disk. Never embed it in the
  executable.
- Use the plugin with `file: None` and `settings: loaded.value.display.clone()`, and write your
  file when `DisplaySettingsSaved` arrives (it carries the saved settings).

### 7. Renaming a setting safely

A settings file outlives every version of the game. Rename a field or an enum variant without
`#[serde(alias = "old")]` and every existing file stops parsing: the loader falls back to the
defaults and the player loses **all** their settings, with only a warning in a log. Keep the old
spelling as an alias and pin it with a test:

```rust
use bevy_display_settings::testing::{assert_old_spelling_loads, assert_round_trips};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, PartialEq, Debug)]
#[serde(default)]
struct Audio {
    #[serde(alias = "volume")] // the old name
    master_volume: f32,
}

// In a #[test]:
let old: Audio = assert_old_spelling_loads("(volume: 0.5)"); // panics with the error if it no longer parses
assert_eq!(old.master_volume, 0.5);
assert_round_trips(&Audio { master_volume: 0.25 });
```

`DisplaySettings` itself reads `backend` as `renderer`, `display_mode` as `mode`, `Exclusive` as
`Fullscreen` and `DX12` as `Dx12`.

### 8. Diagnostics

`diagnostics_after_frames` frames after startup and after each applied change the plugin writes a
`DisplayDiagnostics` message and logs its `line()`:

```text
>>> DISPLAY: mode Borderless, window 1920x1080 logical / 1920x1080 physical (scale 1.00), present_mode AutoNoVsync | monitors: "DELL U2415" 1920x1080 @60.0 Hz (primary)
```

It is the line to ask a player for when the frame rate looks capped or the window looks wrong.
What to know when reading it:

- **Exclusive fullscreen is the only mode that can exceed the monitor's refresh rate.** Borderless
  on Windows is presented by the desktop compositor, which caps it at the refresh rate even with
  vsync off (`AutoNoVsync`).
- A windowed size smaller than the setting is the clamp at work, not a bug.

`RendererChoice::describe()` is the matching `>>> RENDERER:` line.

### 9. System order

| set | schedule | what runs |
|---|---|---|
| `DisplaySettingsSystems::Requests` | `Update` | read the requests (discard, revert, keep, apply, save, restart, in that order; the last apply of a frame stands), write the facts, run the countdown |
| `DisplaySettingsSystems::Apply` | `Update`, after `Requests` | refresh `DisplayInfo`, push mode / size / present mode to the primary window, shadows to lights |
| `DisplaySettingsSystems::Report` | `Update`, after `Apply` | `DisplayDiagnostics` when due |

| your system | order it |
|---|---|
| writes a request and wants it handled this frame | `.before(DisplaySettingsSystems::Requests)` |
| reads the facts or the state | `.after(DisplaySettingsSystems::Requests)` (or `.after(..::Report)` for diagnostics) |
| also writes `Window` in `Update` | order it against `DisplaySettingsSystems::Apply` |

The window is written only when something differs, and the mode only when it is a different
display mode, so a game that put the window on a specific monitor keeps that choice.

### 10. Testing your game

Everything works headless: spawn a `Window` with `PrimaryWindow` and a `Monitor` yourself, drive
time with `TimeUpdateStrategy::ManualDuration`, and replace the `Relauncher`:

```rust
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use bevy::window::PrimaryWindow;
use bevy_display_settings::*;
use std::time::Duration;

let mut app = App::new();
app.add_plugins((MinimalPlugins, DisplaySettingsPlugin::default()));
app.insert_resource(Relauncher::new(|| Ok(()))); // never spawns a process
app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(100)));
app.world_mut().spawn((primary_window(&DisplaySettings::default(), "test"), PrimaryWindow));
app.update();

let windowed = DisplaySettings { mode: DisplayMode::Windowed, ..default() };
app.world_mut().write_message(ApplyDisplaySettings(windowed));
for _ in 0..=100 {
    app.update(); // 10 s later, nobody answered ...
}
assert_eq!(app.world().resource::<DisplaySettings>().mode, DisplayMode::Borderless); // ... reverted
```

## How it works

- **Requests** (`Update`): a new setting is sanitized (resolution clamped to
  `MIN_WINDOW..=MAX_WINDOW`) and compared with the confirmed settings; a mode or windowed size
  change opens the countdown, holding the confirmed settings to go back to. The renderer setting
  is compared with what this run requested at startup.
- **Apply**: `DisplayInfo` is rebuilt when a monitor or the window's monitor changes. The window is
  written when the settings, the monitors or the window change: mode first, then the fitted size
  (`windowed_fit` with the current monitor, Windowed only), then the present mode, each only if it
  differs. A size the player dragged smaller stays; one larger than the fit is clamped again.
  Shadows go to every directional light on a change, and to each new light.
- **Countdown**: real time, whole-second facts, revert on zero.
- **Save**: `save_ron_atomic`; on success the saved settings become the baseline.
- **Restart**: save, `Relauncher`, `AppExit::Success`; any failure stops before exiting.
- **State**: nothing is replicated (display settings are per machine). The file is the player's.

## API reference

### Plugin, configuration, sets

| item | what |
|---|---|
| `DisplaySettingsPlugin { settings, file, renderer, renderer_flags, confirm_timeout, diagnostics_after_frames, log_diagnostics, manage_shadows }` | the plugin; `::from_file(path)` |
| `DisplaySettingsConfig` | resource: a copy of the configuration (read-only) |
| `DisplaySettingsSystems` | `Requests`, `Apply`, `Report` (all `Update`, chained) |
| `DEFAULT_CONFIRM_TIMEOUT` | 10 s |
| `DEFAULT_DIAGNOSTICS_FRAMES` | 10 |

### Settings and state

| item | what |
|---|---|
| `DisplaySettings { renderer, mode, resolution, vsync, shadows }` | resource: the live settings; `sanitized()`, `needs_confirm(&next)`, `from_ron(text)`, `load(path)`, `save(path)` |
| `DisplayMode` | `Windowed`, `Borderless` (default), `Fullscreen`; `ALL`, `label()` |
| `RendererBackend` | `Auto` (default), `Vulkan`, `Dx12`; `ALL`, `label()`, `offered_on(platform)`, `forced_on(platform)` |
| `SavedDisplaySettings(DisplaySettings)` | resource: the last saved settings (the discard baseline) |
| `DisplayConfirm` | resource: `is_pending()`, `secs_left()`, `remaining()`, `previous()` |
| `RendererState` | resource: `choice()`, `pending()`, `restart_required()` |
| `DisplayInfo { monitors, current, windowed_resolutions }` | resource: `current_monitor()`, `current_screen()` |
| `MonitorInfo { name, screen, refresh_millihertz, primary, modes }` | one monitor; `from_monitor(&Monitor, primary)`, `refresh_hz()` |
| `MAX_WINDOW` | 16384x16384, the largest accepted windowed resolution |

### Request messages (you write)

| message | effect |
|---|---|
| `ApplyDisplaySettings(DisplaySettings)` | apply; a mode / windowed size change starts the countdown |
| `KeepDisplaySettings` | keep the change the countdown is asking about |
| `RevertDisplaySettings` | go back to the settings before it |
| `DiscardDisplayChanges` | apply the saved settings again |
| `SaveDisplaySettings` | save and make the baseline |
| `RequestRestart` | save, relaunch, exit |

### Fact messages (you read)

| message | when |
|---|---|
| `DisplaySettingsApplied { settings, needs_confirm }` | an `ApplyDisplaySettings` was applied |
| `DisplayConfirmPending { secs_left }` | the countdown started, or its whole seconds changed |
| `DisplayKept` | the change was kept |
| `DisplayReverted { reason: RevertReason, settings }` | `Requested`, `Timeout` or `Discarded` (`#[non_exhaustive]`) |
| `DisplaySettingsSaved { path, settings }` | saved |
| `DisplaySaveFailed { path, error }` | the file could not be written |
| `RestartRequired { renderer }` | the renderer setting now needs a restart |
| `RestartFailed { error }` | a restart could not save or start the new process |
| `DisplayDiagnostics { mode, logical_size, physical_size, scale_factor, present_mode, monitors }` | after startup and each change; `line()`; `MonitorReport { name, size, refresh_millihertz, primary }` |

### Renderer (before the app)

| item | what |
|---|---|
| `RendererChoice { requested, forced, source, platform }` | `resolve(saved, args, env, flags, platform)`, `from_process(saved, flags)`, `describe()`, `apply_to(&mut WgpuSettings)`, `wgpu_settings()`, `render_plugin()` (last three: feature `render`) |
| `RendererSource` | `LaunchFlag`, `Environment`, `Settings`; `label()` |
| `RendererFlags { vulkan, dx12 }` | default `--vulkan` / `--dx12`; `none()`, `backend_of(arg)`, `is_flag(arg)` |
| `Platform` | `Windows`, `Linux`, `MacOs`, `Other`; `current()` |
| `WGPU_BACKEND_ENV` | `"WGPU_BACKEND"` |
| `relaunch(&flags)`, `relaunch_args(&args, &flags)` | start a new copy of the game; the arguments it passes |
| `Relauncher` | resource: what `RequestRestart` calls; `new(closure)`, `process(flags)` |

### Window helpers (pure)

| item | what |
|---|---|
| `windowed_fit(requested, Option<Screen>)` | the windowed size that is safe to request |
| `windowed_limit(Screen)` | the largest windowed size on a screen |
| `resolution_options(modes, Option<Screen>)` | the windowed sizes to offer |
| `resolution_applies(mode)` | `true` only for `Windowed` |
| `window_mode(mode)`, `display_mode_of(window_mode)`, `present_mode(vsync)` | conversions |
| `primary_window(&settings, title)` | the first `Window` |
| `WindowTarget::new(&settings, Option<Screen>)` | what the plugin pushes to the window |
| `Screen { width, height, scale }` | a monitor for the fitting rules; `Screen::new(w, h)` |
| `WINDOW_CHROME_PX` (80), `WINDOW_BORDER_PX` (16), `MIN_WINDOW` (640x360), `UNKNOWN_MONITOR_WINDOW` (1280x720) | the constants |

### Files and testing

| item | what |
|---|---|
| `load_ron_or_default(path)`, `parse_ron_or_default(text)` | `Loaded { value, status: LoadStatus }`; never panic |
| `save_ron_atomic(path, &value)`, `to_ron_pretty(&value)`, `temp_path(path)` | atomic save and its pieces |
| `path_next_to_exe(name)` | a path beside the executable |
| `testing::assert_old_spelling_loads(text)` | strict parse or a panic with the error |
| `testing::assert_round_trips(&value)` | save format -> parse -> equal |

## Cargo features

| feature | default | adds |
|---|---|---|
| `render` | on | `RendererChoice::{apply_to, wgpu_settings, render_plugin}` (depends on `bevy_render`) |
| `shadows` | on | applying `shadows` to `DirectionalLight` (depends on `bevy_light`) |

With `--no-default-features` the crate needs only `bevy_app`, `bevy_ecs`, `bevy_time` and
`bevy_window`: the renderer is still chosen (apply `RendererChoice::forced` yourself) and
`shadows` is stored and saved but not applied.

## Compatibility

| bevy_display_settings | Bevy | Rust |
|---|---|---|
| 0.2 | 0.20.0 | 1.97.1+ |
| 0.1 | 0.19.0 | 1.95+ |

All Bevy crates are pinned to exactly `=0.20.0` with no default features, so the crate adds no
Bevy feature your game did not ask for and your game keeps one Bevy.

## Examples

| example | what it shows |
|---|---|
| `cargo run --example quick_start` | a 3D scene with a text readout: every request by keyboard (1/2/3 mode, R resolution, V vsync, H shadows, B renderer, K keep, N revert, Enter save, Backspace discard, F5 restart) |
| `cargo run --example renderer_select [-- --vulkan \| --dx12]` | the pre-app renderer choice, then the adapter wgpu actually opened; exits after 2 s |

Both read and write `display_settings.ron` in the working directory.

## Limitations and FAQ

**Why can the renderer not change without a restart?** Bevy creates the GPU device once, inside
`RenderPlugin`. The crate stores the choice, tells you a restart is needed and does the restart
for you.

**Why is my windowed window smaller than the resolution I picked?** It would not fit: the title
bar, frame and taskbar take room, and a window as large as the monitor crashes the renderer on
Windows. Pick Borderless for "the whole screen".

**My frame rate is capped at the refresh rate with vsync off.** Borderless on Windows goes
through the compositor. Use Fullscreen (exclusive).

**Which monitor does fullscreen use?** The one the window is on (`MonitorSelection::Current`);
the first window starts on the primary monitor.

**Does it do anti-aliasing, render scale, frame limits, shadow quality?** No, on purpose: those
depend on your renderer setup. Keep them in your own settings next to `DisplaySettings`
(section 6) and apply them yourself.

**Multiplayer?** Display settings are per machine: nothing is replicated.

**Does it touch anything else?** The primary window's mode, size and present mode, and
`shadow_maps_enabled` on directional lights (turn that off with `manage_shadows: false`).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or
  <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

## Contributing

Issues and pull requests are welcome. Before opening a pull request, please run:

```text
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --all-features -- -D warnings
cargo clippy --all-targets --no-default-features -- -D warnings
cargo test
cargo test --all-features
cargo build --no-default-features
cargo doc --no-deps --all-features
```

Tests must not need a window or a GPU. Keep the README examples compiling (they are checked by
`cargo test`) and add a line to `CHANGELOG.md`.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in
the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without
any additional terms or conditions.
