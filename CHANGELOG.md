# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (before 1.0, a breaking
change or a Bevy bump raises the minor version).

## [0.1.0] - Unreleased

First release, for Bevy 0.19.0.

### Added

- `DisplaySettingsPlugin` (`settings`, `file`, `renderer`, `renderer_flags`, `confirm_timeout`,
  `diagnostics_after_frames`, `log_diagnostics`, `manage_shadows`; `from_file`), copied into the
  `DisplaySettingsConfig` resource; the public `DisplaySettingsSystems::{Requests, Apply, Report}`
  sets in `Update`.
- `DisplaySettings` (`renderer`, `mode`, `resolution`, `vsync`, `shadows`; `#[serde(default)]`,
  aliases for older spellings), `DisplayMode` (`Windowed`, `Borderless` default, `Fullscreen`),
  `RendererBackend` (`Auto` default, `Vulkan`, `Dx12`) with per-platform `offered_on` /
  `forced_on`.
- Renderer choice before the app is built: `RendererChoice` (launch flag > `WGPU_BACKEND` > saved
  setting, configurable `RendererFlags`, `Platform`, `describe()`, and with feature `render`
  `apply_to` / `wgpu_settings` / `render_plugin`); `RendererState` and `RestartRequired`;
  `RequestRestart` with `Relauncher`, `relaunch` and `relaunch_args`.
- Live display mode, windowed resolution and vsync on the primary window; the windowed-fit clamp
  (`windowed_fit`, `windowed_limit`) and the "resolution only in Windowed" rule
  (`resolution_applies`); `DisplayInfo` with every monitor and the windowed resolution list
  (`resolution_options`); `primary_window` for the first window.
- The keep-or-revert countdown (`DisplayConfirm`, `KeepDisplaySettings`, `RevertDisplaySettings`,
  `DisplayConfirmPending`, `DisplayKept`, `DisplayReverted` with `RevertReason`), running on real
  time; `DiscardDisplayChanges` back to `SavedDisplaySettings`.
- Saving: `SaveDisplaySettings`, `DisplaySettingsSaved`, `DisplaySaveFailed`; an unconfirmed mode
  is never written to disk.
- Shadows on / off for every `DirectionalLight` (feature `shadows`).
- `DisplayDiagnostics` with a one-line `>>> DISPLAY:` form, after startup and after each change.
- Settings-file helpers: `load_ron_or_default`, `parse_ron_or_default` (`Loaded`, `LoadStatus`),
  `save_ron_atomic`, `to_ron_pretty`, `temp_path`, `path_next_to_exe`; the `testing` module with
  `assert_old_spelling_loads` and `assert_round_trips`.
- Examples `quick_start` and `renderer_select`.
