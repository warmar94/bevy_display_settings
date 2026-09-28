//! Pure window helpers: the windowed-fit clamp, the resolution list, mode mapping.

use bevy_ecs::resource::Resource;
use bevy_window::{Monitor, MonitorSelection, PresentMode, VideoModeSelection, Window, WindowMode, WindowResolution};

use crate::settings::{DisplayMode, DisplaySettings};

/// Physical pixels kept free below a windowed client area for the title bar, the frame and the
/// taskbar, at 100 % scaling (scaled by the monitor's scale factor).
pub const WINDOW_CHROME_PX: u32 = 80;
/// Physical pixels kept free beside a windowed client area (left + right frame) at 100 % scaling.
pub const WINDOW_BORDER_PX: u32 = 16;
/// The smallest windowed size ever requested.
pub const MIN_WINDOW: (u32, u32) = (640, 360);
/// The largest windowed size requested while no monitor is known (before the windowing backend
/// has reported one). Fits every common screen; re-fitted once the monitor is known.
pub const UNKNOWN_MONITOR_WINDOW: (u32, u32) = (1280, 720);

/// A monitor as the fitting rules need it: physical size and scale factor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    /// Physical width in pixels.
    pub width: u32,
    /// Physical height in pixels.
    pub height: u32,
    /// The monitor's scale factor (1.0 = 100 %).
    pub scale: f64,
}

impl Screen {
    /// A 100 %-scaled screen.
    pub fn new(width: u32, height: u32) -> Self {
        Self { width, height, scale: 1.0 }
    }

    fn valid(&self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// The scale used for the window chrome: finite, 1..=4.
    fn chrome_scale(&self) -> f64 {
        if self.scale.is_finite() && self.scale >= 1.0 {
            self.scale.min(4.0)
        } else {
            1.0
        }
    }
}

/// The largest windowed client area that fits `screen`: the monitor minus the frame
/// ([`WINDOW_BORDER_PX`]) and the title bar + taskbar ([`WINDOW_CHROME_PX`]), both scaled by the
/// monitor's scale factor, and at least [`MIN_WINDOW`]. **Never the monitor's own size.**
pub fn windowed_limit(screen: Screen) -> (u32, u32) {
    let k = screen.chrome_scale();
    let bw = (f64::from(WINDOW_BORDER_PX) * k).ceil() as u32;
    let bh = (f64::from(WINDOW_CHROME_PX) * k).ceil() as u32;
    (screen.width.saturating_sub(bw).max(MIN_WINDOW.0), screen.height.saturating_sub(bh).max(MIN_WINDOW.1))
}

/// **The size a windowed window may actually ask for.**
///
/// Asking for a windowed client area as large as the monitor does not fail: on Windows the window
/// silently gets a smaller surface while `Window::resolution` keeps the requested size. The
/// renderer then sizes some attachments from one and some from the other, wgpu rejects the pass
/// ("Attachments have differing sizes") and the app exits. This clamps the request into
/// [`MIN_WINDOW`]`..=`[`windowed_limit`] and never rounds it up.
///
/// `screen` is `None` while no monitor is known; [`UNKNOWN_MONITOR_WINDOW`] caps it then.
pub fn windowed_fit(requested: (u32, u32), screen: Option<Screen>) -> (u32, u32) {
    let limit = match screen.filter(Screen::valid) {
        Some(s) => windowed_limit(s),
        None => UNKNOWN_MONITOR_WINDOW,
    };
    (requested.0.min(limit.0).max(MIN_WINDOW.0), requested.1.min(limit.1).max(MIN_WINDOW.1))
}

/// The windowed resolutions worth offering: every size among the monitor's video `modes`,
/// deduplicated (modes repeat per refresh rate and bit depth), without the ones that would not fit
/// as a window ([`windowed_limit`], so never the monitor's own size) or are below [`MIN_WINDOW`],
/// sorted small to large. Empty when no monitor is known.
pub fn resolution_options(modes: impl IntoIterator<Item = (u32, u32)>, screen: Option<Screen>) -> Vec<(u32, u32)> {
    let Some(s) = screen.filter(Screen::valid) else { return Vec::new() };
    let (lw, lh) = windowed_limit(s);
    let mut out: Vec<(u32, u32)> = modes.into_iter().filter(|&(w, h)| w >= MIN_WINDOW.0 && h >= MIN_WINDOW.1 && w <= lw && h <= lh).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Does the resolution setting apply in `mode`? Only in `Windowed`: a borderless window's surface
/// is the monitor's and exclusive fullscreen keeps the current video mode; writing a size in
/// either resizes the render targets under an unchanged surface, which wgpu rejects.
pub fn resolution_applies(mode: DisplayMode) -> bool {
    mode == DisplayMode::Windowed
}

/// The Bevy [`WindowMode`] for a [`DisplayMode`] (on the window's current monitor). The plugin
/// writes it only when the window's mode is a different [`DisplayMode`], so a game that put the
/// window on a specific monitor (`MonitorSelection::Index`, ...) keeps that choice.
pub fn window_mode(mode: DisplayMode) -> WindowMode {
    match mode {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        DisplayMode::Fullscreen => WindowMode::Fullscreen(MonitorSelection::Current, VideoModeSelection::Current),
    }
}

/// The [`DisplayMode`] a Bevy [`WindowMode`] corresponds to.
pub fn display_mode_of(mode: WindowMode) -> DisplayMode {
    match mode {
        WindowMode::Windowed => DisplayMode::Windowed,
        WindowMode::BorderlessFullscreen(_) => DisplayMode::Borderless,
        WindowMode::Fullscreen(..) => DisplayMode::Fullscreen,
    }
}

/// Vsync as a present mode: `AutoVsync` or `AutoNoVsync`.
pub fn present_mode(vsync: bool) -> PresentMode {
    if vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    }
}

/// The primary window as `settings` describe it, for `WindowPlugin { primary_window: .. }`.
/// Only `Windowed` gets a size, capped at [`UNKNOWN_MONITOR_WINDOW`] because the monitor is not
/// known yet; the plugin re-fits it once the monitor is reported. A fullscreen mode starts on the
/// primary monitor (a window being created has no "current" monitor yet).
pub fn primary_window(settings: &DisplaySettings, title: impl Into<String>) -> Window {
    let mode = match settings.mode {
        DisplayMode::Windowed => WindowMode::Windowed,
        DisplayMode::Borderless => WindowMode::BorderlessFullscreen(MonitorSelection::Primary),
        DisplayMode::Fullscreen => WindowMode::Fullscreen(MonitorSelection::Primary, VideoModeSelection::Current),
    };
    let mut window = Window { title: title.into(), mode, present_mode: present_mode(settings.vsync), ..Default::default() };
    if resolution_applies(settings.mode) {
        let (w, h) = windowed_fit(settings.resolution, None);
        window.resolution = WindowResolution::new(w, h);
    }
    window
}

/// What the window should look like for some settings on some screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowTarget {
    /// The display mode.
    pub mode: DisplayMode,
    /// The fitted windowed size in physical pixels; `None` outside `Windowed`.
    pub size: Option<(u32, u32)>,
    /// Vsync.
    pub vsync: bool,
}

impl WindowTarget {
    /// The target for `settings` on `screen` (pure).
    pub fn new(settings: &DisplaySettings, screen: Option<Screen>) -> Self {
        Self { mode: settings.mode, size: resolution_applies(settings.mode).then(|| windowed_fit(settings.resolution, screen)), vsync: settings.vsync }
    }
}

/// One monitor, as reported by the windowing backend.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorInfo {
    /// The monitor's name (`"unnamed"` when it has none).
    pub name: String,
    /// Size and scale.
    pub screen: Screen,
    /// Refresh rate in millihertz, when known.
    pub refresh_millihertz: Option<u32>,
    /// Is this the primary monitor?
    pub primary: bool,
    /// The sizes of its video modes (may repeat).
    pub modes: Vec<(u32, u32)>,
}

impl MonitorInfo {
    /// From a Bevy [`Monitor`] component.
    pub fn from_monitor(m: &Monitor, primary: bool) -> Self {
        Self {
            name: m.name.clone().unwrap_or_else(|| "unnamed".to_string()),
            screen: Screen { width: m.physical_width, height: m.physical_height, scale: m.scale_factor },
            refresh_millihertz: m.refresh_rate_millihertz,
            primary,
            modes: m.video_modes.iter().map(|v| (v.physical_size.x, v.physical_size.y)).collect(),
        }
    }

    /// The refresh rate in hertz, when known.
    pub fn refresh_hz(&self) -> Option<f64> {
        self.refresh_millihertz.map(|m| f64::from(m) / 1000.0)
    }
}

/// The monitors and the resolution list, kept up to date by the plugin. Read-only for the game.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct DisplayInfo {
    /// Every monitor, primary first, then by name.
    pub monitors: Vec<MonitorInfo>,
    /// Index into `monitors` of the monitor the primary window is on (else the primary monitor,
    /// else the first). `None` until a monitor has been reported.
    pub current: Option<usize>,
    /// [`resolution_options`] for the current monitor: the windowed sizes to offer.
    pub windowed_resolutions: Vec<(u32, u32)>,
}

impl DisplayInfo {
    /// The monitor the primary window is on (see [`DisplayInfo::current`]).
    pub fn current_monitor(&self) -> Option<&MonitorInfo> {
        self.current.and_then(|i| self.monitors.get(i))
    }

    /// The current monitor as a [`Screen`].
    pub fn current_screen(&self) -> Option<Screen> {
        self.current_monitor().map(|m| m.screen)
    }
}
