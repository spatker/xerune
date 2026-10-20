//! Running a [`Runtime`] on whichever display is available.
//!
//! ```ignore
//! let fonts = xerune_render::Fonts::ttf(REGULAR_TTF, Some(BOLD_TTF))?;
//! let runtime = Runtime::new(MyModel::default(), fonts.clone());
//! App::new(runtime)
//!     .title("Thermostat")
//!     .size(800, 480)
//!     .run(xerune_render::SoftwareRenderer::new(fonts))?;
//! ```
//!
//! The backend is chosen when the app starts, among those compiled in:
//!
//! 1. `XERUNE_SNAPSHOT=out.png` renders one frame headlessly instead (see
//!    [`SnapshotBackend`](super::SnapshotBackend)).
//! 2. `XERUNE_BACKEND=winit|drm|fbdev` picks one explicitly.
//! 3. Otherwise a window (`winit`) when a desktop session is running
//!    (`WAYLAND_DISPLAY` or `DISPLAY`, or not on Linux), else DRM/KMS, else the
//!    fbdev framebuffer. One binary can thus run on a desktop and on the
//!    device.

use super::{Backend, BackendError, Messenger};
use crate::{Model, Renderer, Runtime, TextMeasurer, TemplateLayout};

/// A display backend [`App::run`] can use.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendKind {
    /// A desktop window (`winit` feature).
    Winit,
    /// DRM/KMS (`drm` and `evdev` features).
    Drm,
    /// The fbdev framebuffer `/dev/fb0` (`linuxfb` and `evdev` features).
    LinuxFb,
}

impl BackendKind {
    /// The backends compiled into this binary, in order of preference
    /// when nothing is configured.
    pub fn available() -> &'static [BackendKind] {
        &[
            #[cfg(feature = "winit")]
            BackendKind::Winit,
            #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
            BackendKind::Drm,
            #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
            BackendKind::LinuxFb,
        ]
    }

    fn name(self) -> &'static str {
        match self {
            BackendKind::Winit => "winit",
            BackendKind::Drm => "drm",
            BackendKind::LinuxFb => "fbdev",
        }
    }

    /// The backend to use: `XERUNE_BACKEND` if set, else a window in a
    /// desktop session, else the first display backend compiled in.
    pub fn detect() -> Result<BackendKind, BackendError> {
        let available = Self::available();
        let names = || available.iter().map(|b| b.name()).collect::<Vec<_>>().join(", ");
        if let Ok(wanted) = std::env::var("XERUNE_BACKEND") {
            return available
                .iter()
                .copied()
                .find(|b| b.name() == wanted.trim())
                .ok_or_else(|| BackendError::Init(format!("XERUNE_BACKEND={wanted} is not compiled in (available: {})", names())));
        }
        let desktop = !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_some() || std::env::var_os("DISPLAY").is_some();
        let display_backend = available.iter().copied().find(|b| *b != BackendKind::Winit);
        let preferred = if desktop { available.first().copied() } else { display_backend.or(available.first().copied()) };
        preferred.ok_or_else(|| BackendError::Init("no display backend compiled in (enable winit, drm or linuxfb)".into()))
    }
}

/// An app ready to run: its [`Runtime`], window title and size, and what to
/// do once it is running.
pub struct App<M: Model, TM> {
    runtime: Runtime<M, TM>,
    title: String,
    size: (u32, u32),
    on_start: Option<Box<dyn FnOnce(Messenger<M>)>>,
}

impl<M, TM> App<M, TM>
where
    M: Model + TemplateLayout + 'static,
    TM: TextMeasurer + 'static,
{
    /// An app around `runtime` (with its model, fonts and image
    /// loader set up), titled "Xerune", 800x480.
    pub fn new(runtime: Runtime<M, TM>) -> Self {
        Self { runtime, title: "Xerune".into(), size: (800, 480), on_start: None }
    }

    /// The window title (desktop only).
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// The window size, and the snapshot size unless `XERUNE_SNAPSHOT_SIZE`
    /// sets one. Displays use their mode's size.
    pub fn size(mut self, width: u32, height: u32) -> Self {
        self.size = (width, height);
        self
    }

    /// The frame rate while something animates (see
    /// [`Runtime::set_target_fps`]); `XERUNE_TARGET_FPS` overrides it.
    pub fn target_fps(mut self, fps: u32) -> Self {
        self.runtime.set_target_fps(fps);
        self
    }

    /// Tapping a text input opens the on-screen keyboard, for devices
    /// without a keyboard (see [`Runtime::set_input_keyboard`]).
    pub fn input_keyboard(mut self, on: bool) -> Self {
        self.runtime.set_input_keyboard(on);
        self
    }

    /// Runs `f` once the app is running, with a [`Messenger`] to send it
    /// messages and changes from other threads, or to end it. Not called for snapshots, which must be
    /// deterministic: reach a state with `XERUNE_SNAPSHOT_MESSAGES` instead.
    pub fn on_start(mut self, f: impl FnOnce(Messenger<M>) + 'static) -> Self {
        self.on_start = Some(Box::new(f));
        self
    }

    /// Runs the app with `renderer` on the backend [`BackendKind::detect`]
    /// picks (or headlessly, see the module docs), until it exits.
    pub fn run(self, renderer: impl Renderer + 'static) -> Result<(), BackendError> {
        if let Some(snapshot) = super::SnapshotBackend::from_env() {
            let (width, height) = self.size;
            return snapshot?.run(&self.title, width, height, self.runtime, renderer, |_| {});
        }
        let kind = BackendKind::detect()?;
        log::info!("xerune: starting the {} backend", kind.name());
        self.run_on(kind, renderer)
    }

    /// Runs the app on a specific backend.
    // Without display backends only the error arm uses nothing.
    #[cfg_attr(not(any(feature = "winit", all(target_os = "linux", feature = "evdev", any(feature = "drm", feature = "linuxfb")))), allow(unused_variables))]
    pub fn run_on(self, kind: BackendKind, renderer: impl Renderer + 'static) -> Result<(), BackendError> {
        let App { runtime, title, size: (width, height), on_start } = self;
        let start = move |messenger: Messenger<M>| {
            if let Some(f) = on_start {
                f(messenger)
            }
        };
        match kind {
            #[cfg(feature = "winit")]
            BackendKind::Winit => super::WinitBackend::new().run(&title, width, height, runtime, renderer, start),
            #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
            BackendKind::Drm => super::DrmBackend::new().run(&title, width, height, runtime, renderer, start),
            #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
            BackendKind::LinuxFb => super::LinuxFbBackend::new().run(&title, width, height, runtime, renderer, start),
            #[allow(unreachable_patterns)]
            other => Err(BackendError::Init(format!("the {} backend is not compiled in", other.name()))),
        }
    }
}
