//! Choosing the renderer backend before the app is built, and restarting to change it.

use std::sync::Arc;

use bevy_ecs::resource::Resource;

use crate::settings::RendererBackend;

/// The environment variable wgpu reads itself to pick a backend (`vulkan`, `dx12`, `metal`, `gl`).
pub const WGPU_BACKEND_ENV: &str = "WGPU_BACKEND";

/// The operating system, as far as renderer choice is concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    /// Windows: DX12 and Vulkan.
    Windows,
    /// Linux: Vulkan.
    Linux,
    /// macOS: Metal only.
    MacOs,
    /// Anything else (the web, mobile, BSDs): wgpu decides.
    Other,
}

impl Platform {
    /// The platform this binary was compiled for.
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Self::Windows
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// Where the renderer choice came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererSource {
    /// A launch flag such as `--vulkan` (strongest).
    LaunchFlag,
    /// The `WGPU_BACKEND` environment variable (wgpu reads it itself).
    Environment,
    /// The saved setting (weakest).
    Settings,
}

impl RendererSource {
    /// A short ASCII description for logs.
    pub fn label(self) -> &'static str {
        match self {
            Self::LaunchFlag => "launch flag",
            Self::Environment => "WGPU_BACKEND",
            Self::Settings => "settings",
        }
    }
}

/// The launch flags that force a backend for one run. The rescue path when a backend will not
/// start: a menu you cannot reach cannot change it back.
///
/// Default: `--vulkan` and `--dx12`. [`RendererFlags::none`] turns flags off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererFlags {
    /// Flags that force Vulkan.
    pub vulkan: Vec<String>,
    /// Flags that force DirectX 12.
    pub dx12: Vec<String>,
}

impl Default for RendererFlags {
    fn default() -> Self {
        Self { vulkan: vec!["--vulkan".into()], dx12: vec!["--dx12".into()] }
    }
}

impl RendererFlags {
    /// No launch flags at all.
    pub fn none() -> Self {
        Self { vulkan: Vec::new(), dx12: Vec::new() }
    }

    /// The backend a flag in `arg` forces, if any.
    pub fn backend_of(&self, arg: &str) -> Option<RendererBackend> {
        if self.vulkan.iter().any(|f| f == arg) {
            Some(RendererBackend::Vulkan)
        } else if self.dx12.iter().any(|f| f == arg) {
            Some(RendererBackend::Dx12)
        } else {
            None
        }
    }

    /// Is `arg` one of these flags?
    pub fn is_flag(&self, arg: &str) -> bool {
        self.backend_of(arg).is_some()
    }
}

/// The renderer decision, made before the app is built.
///
/// Precedence, strongest first: a launch flag (the **last** one given wins), then a non-empty
/// `WGPU_BACKEND` environment variable (the choice is left to wgpu, which reads it), then the
/// saved setting.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererChoice {
    /// What was asked for (`Auto` when `WGPU_BACKEND` decides).
    pub requested: RendererBackend,
    /// What is actually forced on this platform; `None` = wgpu decides.
    pub forced: Option<RendererBackend>,
    /// Which source won.
    pub source: RendererSource,
    /// The platform the decision was made for.
    pub platform: Platform,
}

impl RendererChoice {
    /// Decide from explicit inputs (pure; this is what the tests call).
    ///
    /// `args` are the program's arguments (with or without the executable name), `env_backend`
    /// the value of `WGPU_BACKEND` if set.
    pub fn resolve(saved: RendererBackend, args: &[String], env_backend: Option<&str>, flags: &RendererFlags, platform: Platform) -> Self {
        let (requested, source) = if let Some(b) = args.iter().rev().find_map(|a| flags.backend_of(a)) {
            (b, RendererSource::LaunchFlag)
        } else if env_backend.is_some_and(|v| !v.trim().is_empty()) {
            (RendererBackend::Auto, RendererSource::Environment)
        } else {
            (saved, RendererSource::Settings)
        };
        Self { requested, forced: requested.forced_on(platform), source, platform }
    }

    /// Decide for this process: its arguments, its `WGPU_BACKEND`, [`Platform::current`].
    pub fn from_process(saved: RendererBackend, flags: &RendererFlags) -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let env = std::env::var(WGPU_BACKEND_ENV).ok();
        Self::resolve(saved, &args, env.as_deref(), flags, Platform::current())
    }

    /// One ASCII line saying what was chosen and why, e.g.
    /// `>>> RENDERER: Vulkan (from launch flag; applies at startup only)`.
    ///
    /// Print it yourself before building the app (no log subscriber exists yet); the plugin also
    /// logs it once at build time.
    pub fn describe(&self) -> String {
        let what = match self.forced {
            Some(b) => b.label().to_string(),
            None if self.requested == RendererBackend::Auto => "Auto (wgpu decides)".to_string(),
            None => format!("Auto (wgpu decides; {} is not available on this platform)", self.requested.label()),
        };
        format!(">>> RENDERER: {what} (from {}; applies at startup only)", self.source.label())
    }

    /// Write the forced backend into `settings.backends`; leave it alone when wgpu decides (so
    /// `WgpuSettings::default()`'s reading of `WGPU_BACKEND` stays in effect).
    #[cfg(feature = "render")]
    pub fn apply_to(&self, settings: &mut bevy_render::settings::WgpuSettings) {
        use bevy_render::settings::Backends;
        match self.forced {
            Some(RendererBackend::Vulkan) => settings.backends = Some(Backends::VULKAN),
            Some(RendererBackend::Dx12) => settings.backends = Some(Backends::DX12),
            Some(RendererBackend::Auto) | None => {}
        }
    }

    /// `WgpuSettings::default()` with the backend applied ([`RendererChoice::apply_to`]).
    #[cfg(feature = "render")]
    pub fn wgpu_settings(&self) -> bevy_render::settings::WgpuSettings {
        let mut s = bevy_render::settings::WgpuSettings::default();
        self.apply_to(&mut s);
        s
    }

    /// A `RenderPlugin` for `DefaultPlugins.set(..)`, created from [`RendererChoice::wgpu_settings`].
    /// With no backend forced it is equivalent to Bevy's default.
    #[cfg(feature = "render")]
    pub fn render_plugin(&self) -> bevy_render::RenderPlugin {
        bevy_render::RenderPlugin { render_creation: self.wgpu_settings().into(), ..Default::default() }
    }
}

/// The arguments a relaunch passes on: this run's, minus the renderer flags (the player just
/// picked a renderer in the menu, and a `--vulkan` carried over would override it).
pub fn relaunch_args(args: &[String], flags: &RendererFlags) -> Vec<String> {
    args.iter().filter(|a| !flags.is_flag(a)).cloned().collect()
}

/// Start a new copy of this executable with [`relaunch_args`] and without `WGPU_BACKEND` (same
/// reason), in the same working directory, for the caller to exit behind.
///
/// Save the settings **first**: the new process reads them at startup. An error is returned, never
/// swallowed, so the caller can keep the game running instead of closing it with no replacement.
pub fn relaunch(flags: &RendererFlags) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(exe).args(relaunch_args(&args, flags)).env_remove(WGPU_BACKEND_ENV).spawn()?;
    Ok(())
}

/// The function [`RequestRestart`](crate::RequestRestart) calls to start the new process.
/// Default: [`relaunch`] with the plugin's [`RendererFlags`]. Replace it in tests (so no process
/// is spawned) or to relaunch through a launcher.
#[derive(Resource, Clone)]
pub struct Relauncher(pub Arc<dyn Fn() -> std::io::Result<()> + Send + Sync>);

impl Relauncher {
    /// A relauncher from any closure.
    pub fn new(f: impl Fn() -> std::io::Result<()> + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    /// The default: [`relaunch`] with `flags`.
    pub fn process(flags: RendererFlags) -> Self {
        Self::new(move || relaunch(&flags))
    }
}

impl Default for Relauncher {
    fn default() -> Self {
        Self::process(RendererFlags::default())
    }
}

/// The renderer this process runs and whether the setting now asks for another one.
///
/// A restart is required when the saved choice ([`DisplaySettings::renderer`](crate::DisplaySettings))
/// differs from what this run requested at startup. Owned by the plugin; read-only for the game.
#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub struct RendererState {
    choice: RendererChoice,
    pending: Option<RendererBackend>,
}

impl RendererState {
    pub(crate) fn new(choice: RendererChoice, setting: RendererBackend) -> Self {
        let mut s = Self { choice, pending: None };
        s.update(setting);
        s
    }

    /// Update for a new setting; `true` when a restart just became required (or its target
    /// changed).
    pub(crate) fn update(&mut self, setting: RendererBackend) -> bool {
        let next = (setting != self.choice.requested).then_some(setting);
        let became = next.is_some() && next != self.pending;
        self.pending = next;
        became
    }

    /// The startup decision.
    pub fn choice(&self) -> &RendererChoice {
        &self.choice
    }

    /// The backend the next start will request, when it differs from this run's.
    pub fn pending(&self) -> Option<RendererBackend> {
        self.pending
    }

    /// Does the current setting need a restart to take effect?
    pub fn restart_required(&self) -> bool {
        self.pending.is_some()
    }
}
