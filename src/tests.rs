//! Unit tests of the pure parts (no app, no window).

use crate::testing::{assert_old_spelling_loads, assert_round_trips};
use crate::*;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------- windowed fit

#[test]
fn a_windowed_request_as_big_as_the_monitor_is_clamped_inside_it() {
    let screen = Screen::new(1920, 1080);
    let (w, h) = windowed_fit((1920, 1080), Some(screen));
    assert_eq!((w, h), (1920 - WINDOW_BORDER_PX, 1080 - WINDOW_CHROME_PX));
    assert!(w < 1920 && h < 1080, "never the monitor's own size");
}

#[test]
fn the_window_chrome_scales_with_the_monitor() {
    let screen = Screen { width: 3840, height: 2160, scale: 1.5 };
    assert_eq!(windowed_limit(screen), (3840 - 24, 2160 - 120));
    // A broken scale factor falls back to 100 %.
    let odd = Screen { width: 1920, height: 1080, scale: f64::NAN };
    assert_eq!(windowed_limit(odd), (1904, 1000));
}

#[test]
fn a_small_request_is_kept_and_a_tiny_one_is_raised_to_the_minimum() {
    let screen = Some(Screen::new(2560, 1440));
    assert_eq!(windowed_fit((1280, 720), screen), (1280, 720));
    assert_eq!(windowed_fit((10, 10), screen), MIN_WINDOW);
}

#[test]
fn without_a_known_monitor_the_request_is_capped_to_a_safe_size() {
    assert_eq!(windowed_fit((3840, 2160), None), UNKNOWN_MONITOR_WINDOW);
    assert_eq!(windowed_fit((3840, 2160), Some(Screen::new(0, 0))), UNKNOWN_MONITOR_WINDOW);
    assert_eq!(windowed_fit((800, 600), None), (800, 600));
}

#[test]
fn a_tiny_monitor_never_produces_a_window_below_the_minimum() {
    assert_eq!(windowed_limit(Screen::new(320, 200)), MIN_WINDOW);
}

#[test]
fn the_resolution_list_is_deduplicated_sorted_and_never_the_monitor_size() {
    let modes = [(1920, 1080), (1280, 720), (1920, 1080), (640, 480), (320, 240), (1600, 900), (2560, 1440)];
    let list = resolution_options(modes, Some(Screen::new(1920, 1080)));
    assert_eq!(list, vec![(640, 480), (1280, 720), (1600, 900)]);
    assert!(resolution_options(modes, None).is_empty());
}

#[test]
fn the_resolution_applies_only_in_windowed_mode() {
    assert!(resolution_applies(DisplayMode::Windowed));
    assert!(!resolution_applies(DisplayMode::Borderless));
    assert!(!resolution_applies(DisplayMode::Fullscreen));
    let s = DisplaySettings { mode: DisplayMode::Borderless, resolution: (800, 600), ..Default::default() };
    assert_eq!(WindowTarget::new(&s, Some(Screen::new(1920, 1080))).size, None);
    let s = DisplaySettings { mode: DisplayMode::Windowed, resolution: (1920, 1080), ..Default::default() };
    assert_eq!(WindowTarget::new(&s, Some(Screen::new(1920, 1080))).size, Some((1904, 1000)));
}

#[test]
fn display_modes_map_to_window_modes_and_back() {
    for mode in DisplayMode::ALL {
        assert_eq!(display_mode_of(window_mode(mode)), mode);
    }
    assert_eq!(present_mode(true), bevy_window::PresentMode::AutoVsync);
    assert_eq!(present_mode(false), bevy_window::PresentMode::AutoNoVsync);
}

#[test]
fn the_first_window_takes_its_size_only_when_windowed() {
    let s = DisplaySettings { mode: DisplayMode::Windowed, resolution: (3840, 2160), vsync: false, ..Default::default() };
    let w = primary_window(&s, "Test");
    assert_eq!(w.title, "Test");
    assert_eq!((w.resolution.physical_width(), w.resolution.physical_height()), UNKNOWN_MONITOR_WINDOW);
    assert_eq!(w.present_mode, bevy_window::PresentMode::AutoNoVsync);
    let b = primary_window(&DisplaySettings::default(), "Test");
    assert_eq!(display_mode_of(b.mode), DisplayMode::Borderless);
    assert_eq!(b.resolution, bevy_window::Window::default().resolution);
}

// ---------------------------------------------------------------- renderer

#[test]
fn a_launch_flag_beats_the_environment_which_beats_the_settings() {
    let f = RendererFlags::default();
    let p = Platform::Windows;
    let c = RendererChoice::resolve(RendererBackend::Dx12, &args(&["--vulkan"]), Some("dx12"), &f, p);
    assert_eq!((c.requested, c.forced, c.source), (RendererBackend::Vulkan, Some(RendererBackend::Vulkan), RendererSource::LaunchFlag));
    let c = RendererChoice::resolve(RendererBackend::Dx12, &[], Some("vulkan"), &f, p);
    assert_eq!((c.requested, c.forced, c.source), (RendererBackend::Auto, None, RendererSource::Environment));
    let c = RendererChoice::resolve(RendererBackend::Dx12, &args(&["--other"]), None, &f, p);
    assert_eq!((c.requested, c.forced, c.source), (RendererBackend::Dx12, Some(RendererBackend::Dx12), RendererSource::Settings));
}

#[test]
fn the_last_flag_wins_and_an_empty_environment_variable_is_ignored() {
    let f = RendererFlags::default();
    let c = RendererChoice::resolve(RendererBackend::Auto, &args(&["--vulkan", "--dx12"]), None, &f, Platform::Windows);
    assert_eq!(c.requested, RendererBackend::Dx12);
    let c = RendererChoice::resolve(RendererBackend::Vulkan, &[], Some("  "), &f, Platform::Windows);
    assert_eq!(c.source, RendererSource::Settings);
}

#[test]
fn launch_flags_are_configurable_and_can_be_turned_off() {
    let f = RendererFlags { vulkan: vec!["-vk".into()], dx12: vec!["-d3d".into(), "/dx12".into()] };
    let c = RendererChoice::resolve(RendererBackend::Auto, &args(&["/dx12"]), None, &f, Platform::Windows);
    assert_eq!((c.requested, c.source), (RendererBackend::Dx12, RendererSource::LaunchFlag));
    let c = RendererChoice::resolve(RendererBackend::Auto, &args(&["--vulkan"]), None, &f, Platform::Windows);
    assert_eq!(c.source, RendererSource::Settings);
    let c = RendererChoice::resolve(RendererBackend::Auto, &args(&["--vulkan"]), None, &RendererFlags::none(), Platform::Windows);
    assert_eq!(c.source, RendererSource::Settings);
}

#[test]
fn a_backend_is_forced_only_where_the_platform_has_it() {
    use Platform::*;
    use RendererBackend::*;
    let table = [(Windows, [None, Some(Vulkan), Some(Dx12)]), (Linux, [None, Some(Vulkan), None]), (MacOs, [None, None, None]), (Other, [None, None, None])];
    for (platform, expected) in table {
        for (backend, want) in RendererBackend::ALL.into_iter().zip(expected) {
            assert_eq!(backend.forced_on(platform), want, "{backend:?} on {platform:?}");
        }
        // Everything offered on a platform is something it can run (or Auto).
        for b in RendererBackend::offered_on(platform) {
            assert!(*b == Auto || b.forced_on(platform) == Some(*b));
        }
    }
    let c = RendererChoice::resolve(Dx12, &[], None, &RendererFlags::default(), Linux);
    assert_eq!(c.forced, None);
    assert!(c.describe().contains("not available"), "{}", c.describe());
}

#[test]
fn the_renderer_line_names_the_backend_and_the_source() {
    let c = RendererChoice::resolve(RendererBackend::Vulkan, &args(&["--vulkan"]), None, &RendererFlags::default(), Platform::Windows);
    assert_eq!(c.describe(), ">>> RENDERER: Vulkan (from launch flag; applies at startup only)");
    assert!(c.describe().is_ascii());
}

#[cfg(feature = "render")]
#[test]
fn the_wgpu_settings_force_only_a_chosen_backend() {
    use bevy_render::settings::Backends;
    let f = RendererFlags::default();
    let vk = RendererChoice::resolve(RendererBackend::Vulkan, &[], None, &f, Platform::Windows);
    assert_eq!(vk.wgpu_settings().backends, Some(Backends::VULKAN));
    let dx = RendererChoice::resolve(RendererBackend::Dx12, &[], None, &f, Platform::Windows);
    assert_eq!(dx.wgpu_settings().backends, Some(Backends::DX12));
    let auto = RendererChoice::resolve(RendererBackend::Auto, &[], None, &f, Platform::Windows);
    let mut s = bevy_render::settings::WgpuSettings { backends: Some(Backends::GL), ..Default::default() };
    auto.apply_to(&mut s);
    assert_eq!(s.backends, Some(Backends::GL), "Auto leaves the caller's choice alone");
}

#[test]
fn a_relaunch_drops_the_renderer_flags_and_keeps_everything_else() {
    let a = args(&["--dev", "--vulkan", "save1", "--dx12"]);
    assert_eq!(relaunch_args(&a, &RendererFlags::default()), args(&["--dev", "save1"]));
    assert_eq!(relaunch_args(&a, &RendererFlags::none()), a);
}

// ---------------------------------------------------------------- settings

#[test]
fn the_defaults_are_the_safe_ones() {
    let d = DisplaySettings::default();
    assert_eq!(d.mode, DisplayMode::Borderless);
    assert_eq!(d.renderer, RendererBackend::Auto);
    assert!(d.vsync && d.shadows);
}

#[test]
fn only_a_mode_or_windowed_size_change_needs_confirmation() {
    let base = DisplaySettings::default();
    let mut v = base.clone();
    v.vsync = false;
    v.shadows = false;
    v.renderer = RendererBackend::Vulkan;
    assert!(!base.needs_confirm(&v));
    let mut r = base.clone();
    r.resolution = (800, 600);
    assert!(!base.needs_confirm(&r), "borderless ignores the resolution");
    let mut m = base.clone();
    m.mode = DisplayMode::Windowed;
    assert!(base.needs_confirm(&m));
    let mut mr = m.clone();
    mr.resolution = (1024, 768);
    assert!(m.needs_confirm(&mr));
}

#[test]
fn sanitizing_clamps_the_resolution() {
    let s = DisplaySettings { resolution: (1, 99999), ..Default::default() }.sanitized();
    assert_eq!(s.resolution, (MIN_WINDOW.0, MAX_WINDOW.1));
}

#[test]
fn a_file_missing_fields_keeps_the_rest_and_a_broken_file_gives_defaults() {
    let s = DisplaySettings::from_ron("(vsync: false)");
    assert!(!s.vsync);
    assert_eq!(s.mode, DisplayMode::Borderless);
    let loaded = parse_ron_or_default::<DisplaySettings>("(vsync: maybe");
    assert!(matches!(loaded.status, LoadStatus::Invalid(_)));
    assert_eq!(loaded.value, DisplaySettings::default());
    assert_eq!(DisplaySettings::from_ron("not ron at all {{"), DisplaySettings::default());
}

#[test]
fn old_spellings_still_load() {
    let s: DisplaySettings = assert_old_spelling_loads("(backend: DX12, display_mode: Exclusive, resolution: (1024, 768))");
    assert_eq!(s.renderer, RendererBackend::Dx12);
    assert_eq!(s.mode, DisplayMode::Fullscreen);
    assert_eq!(s.resolution, (1024, 768));
    let named: DisplaySettings = assert_old_spelling_loads("DisplaySettings(renderer: Vulkan, unknown_future_field: 3)");
    assert_eq!(named.renderer, RendererBackend::Vulkan);
}

#[test]
#[should_panic(expected = "no longer loads")]
fn the_old_spelling_helper_fails_loudly_on_a_rename_without_alias() {
    let _: DisplaySettings = assert_old_spelling_loads("(mode: Exclusiv)");
}

#[test]
fn settings_round_trip_through_ron() {
    let s = DisplaySettings { renderer: RendererBackend::Dx12, mode: DisplayMode::Windowed, resolution: (1600, 900), vsync: false, shadows: false };
    assert_round_trips(&s);
}

// ---------------------------------------------------------------- diagnostics

#[test]
fn the_diagnostics_line_is_one_readable_ascii_line() {
    let d = DisplayDiagnostics {
        mode: DisplayMode::Borderless,
        logical_size: (1280.0, 720.0),
        physical_size: (1920, 1080),
        scale_factor: 1.5,
        present_mode: bevy_window::PresentMode::AutoNoVsync,
        monitors: vec![
            MonitorReport { name: "Main".into(), size: (1920, 1080), refresh_millihertz: Some(59_950), primary: true },
            MonitorReport { name: "Side".into(), size: (1280, 1024), refresh_millihertz: None, primary: false },
        ],
    };
    assert_eq!(
        d.line(),
        ">>> DISPLAY: mode Borderless, window 1280x720 logical / 1920x1080 physical (scale 1.50), present_mode AutoNoVsync | monitors: \"Main\" 1920x1080 @60.0 Hz (primary); \"Side\" 1280x1024 @unknown Hz"
    );
    assert!(!d.line().contains('\n') && d.line().is_ascii());
    let none = DisplayDiagnostics { monitors: Vec::new(), ..d };
    assert!(none.line().ends_with("monitors: none"));
}

#[test]
fn every_label_is_ascii() {
    for m in DisplayMode::ALL {
        assert!(m.label().is_ascii());
    }
    for b in RendererBackend::ALL {
        assert!(b.label().is_ascii());
    }
}
