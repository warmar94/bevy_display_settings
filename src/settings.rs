//! The settings themselves: [`DisplaySettings`], [`DisplayMode`], [`RendererBackend`].

use std::path::Path;

use bevy_ecs::resource::Resource;
use serde::{Deserialize, Serialize};

use crate::file::{load_ron_or_default, parse_ron_or_default, save_ron_atomic};
use crate::renderer::Platform;
use crate::window::MIN_WINDOW;

/// The largest windowed resolution accepted from a settings file (anything above is clamped).
pub const MAX_WINDOW: (u32, u32) = (16384, 16384);

/// The graphics API the renderer is created with. Chosen **before** the app is built (see
/// [`RendererChoice`](crate::RendererChoice)); changing it takes a restart.
///
/// | platform | `Auto` | `Vulkan` | `Dx12` |
/// |---|---|---|---|
/// | Windows | wgpu decides (usually DX12) | Vulkan | DirectX 12 |
/// | Linux | wgpu decides (Vulkan) | Vulkan | not available: wgpu decides |
/// | macOS / other | wgpu decides (Metal) | not available: wgpu decides | not available: wgpu decides |
///
/// A choice the platform does not have is never forced, so it can never stop the app starting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RendererBackend {
    /// Let wgpu decide. Also leaves the `WGPU_BACKEND` environment variable working.
    #[default]
    Auto,
    /// Vulkan (Windows, Linux).
    #[serde(alias = "VULKAN")]
    Vulkan,
    /// DirectX 12 (Windows only).
    #[serde(alias = "DX12", alias = "DirectX12")]
    Dx12,
}

impl RendererBackend {
    /// Every variant, in menu order.
    pub const ALL: [RendererBackend; 3] = [Self::Auto, Self::Vulkan, Self::Dx12];

    /// A short, ASCII, player-facing name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Vulkan => "Vulkan",
            Self::Dx12 => "DirectX 12",
        }
    }

    /// The choices worth offering in a menu on `platform` (always starts with `Auto`).
    pub fn offered_on(platform: Platform) -> &'static [RendererBackend] {
        match platform {
            Platform::Windows => &[Self::Auto, Self::Vulkan, Self::Dx12],
            Platform::Linux => &[Self::Auto, Self::Vulkan],
            Platform::MacOs | Platform::Other => &[Self::Auto],
        }
    }

    /// The backend that is actually forced on `platform`, or `None` for "wgpu decides" (`Auto`,
    /// or a backend this platform does not have).
    pub fn forced_on(self, platform: Platform) -> Option<RendererBackend> {
        match (self, platform) {
            (Self::Vulkan, Platform::Windows | Platform::Linux) => Some(Self::Vulkan),
            (Self::Dx12, Platform::Windows) => Some(Self::Dx12),
            _ => None,
        }
    }
}

/// How the primary window is shown.
///
/// - `Windowed`: a normal window; the only mode that uses [`DisplaySettings::resolution`].
/// - `Borderless` (**the default**): a borderless window covering the current monitor. Safe on
///   every system. On Windows it is presented through the desktop compositor, which caps the
///   frame rate at the monitor's refresh rate even with vsync off.
/// - `Fullscreen`: exclusive fullscreen at the monitor's current video mode. The only mode that
///   can run faster than the monitor's refresh rate with vsync off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DisplayMode {
    /// A normal, decorated window.
    Windowed,
    /// A borderless window the size of the monitor.
    #[default]
    Borderless,
    /// Exclusive fullscreen.
    #[serde(alias = "Exclusive")]
    Fullscreen,
}

impl DisplayMode {
    /// Every variant, in menu order.
    pub const ALL: [DisplayMode; 3] = [Self::Windowed, Self::Borderless, Self::Fullscreen];

    /// A short, ASCII, player-facing name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::Borderless => "Borderless",
            Self::Fullscreen => "Fullscreen",
        }
    }
}

/// The display and renderer settings. The plugin keeps the live copy as a resource; read it
/// freely, change it with [`ApplyDisplaySettings`](crate::ApplyDisplaySettings).
///
/// Serialized with `#[serde(default)]`: a file missing a field gets that field's default, and a
/// field added in a later version never breaks an older file.
#[derive(Resource, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplaySettings {
    /// The renderer backend. Applies at the next start ([`RendererState`](crate::RendererState)).
    #[serde(alias = "backend")]
    pub renderer: RendererBackend,
    /// Windowed, borderless or exclusive fullscreen.
    #[serde(alias = "display_mode")]
    pub mode: DisplayMode,
    /// The windowed client-area size in **physical** pixels. Used only in
    /// [`DisplayMode::Windowed`], and always through [`windowed_fit`](crate::windowed_fit).
    pub resolution: (u32, u32),
    /// Vsync on (`AutoVsync`) or off (`AutoNoVsync`).
    pub vsync: bool,
    /// Shadow maps on every directional light (feature `shadows`).
    pub shadows: bool,
}

impl Default for DisplaySettings {
    fn default() -> Self {
        Self { renderer: RendererBackend::Auto, mode: DisplayMode::Borderless, resolution: (1280, 720), vsync: true, shadows: true }
    }
}

impl DisplaySettings {
    /// A copy with every value in range: the resolution clamped to
    /// [`MIN_WINDOW`](crate::MIN_WINDOW)`..=`[`MAX_WINDOW`].
    pub fn sanitized(mut self) -> Self {
        self.resolution = (self.resolution.0.clamp(MIN_WINDOW.0, MAX_WINDOW.0), self.resolution.1.clamp(MIN_WINDOW.1, MAX_WINDOW.1));
        self
    }

    /// Would going from `self` to `next` change what is on screen in a way that needs the
    /// keep-or-revert confirmation? A mode change, or a resolution change in `Windowed`. Vsync,
    /// shadows and the renderer never do.
    pub fn needs_confirm(&self, next: &DisplaySettings) -> bool {
        self.mode != next.mode || (next.mode == DisplayMode::Windowed && self.resolution != next.resolution)
    }

    /// Parse RON text. Broken text gives the defaults (with a warning); never panics.
    pub fn from_ron(text: &str) -> Self {
        parse_ron_or_default::<Self>(text).value.sanitized()
    }

    /// Load a settings file. Missing gives the defaults; broken gives the defaults with a warning.
    /// Never panics. For the load status, use [`load_ron_or_default`] directly.
    pub fn load(path: impl AsRef<Path>) -> Self {
        load_ron_or_default::<Self>(path).value.sanitized()
    }

    /// Write the settings atomically (see [`save_ron_atomic`]).
    pub fn save(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        save_ron_atomic(path, self)
    }
}
