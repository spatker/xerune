#[cfg(feature = "winit")]
pub(crate) mod winit_impl;

// Without a display backend, an `App` still renders snapshots.
#[cfg(not(target_arch = "wasm32"))]
mod app;
#[cfg(not(target_arch = "wasm32"))]
pub use app::{App, BackendKind};

/// Hardware input abstraction interface.
pub mod input;

#[cfg(all(target_os = "linux", feature = "evdev"))]
pub(crate) mod common_loop;

#[cfg(all(target_os = "linux", feature = "evdev"))]
pub(crate) mod render_utils;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
pub(crate) mod linuxfb_impl;

#[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
pub(crate) mod drm_impl;

#[cfg(feature = "browser")]
pub mod browser_impl;

#[cfg(not(target_arch = "wasm32"))]
pub mod snapshot;
#[cfg(not(target_arch = "wasm32"))]
pub use snapshot::SnapshotBackend;

#[cfg(feature = "winit")]
pub use winit_impl::WinitBackend;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
pub use linuxfb_impl::LinuxFbBackend;

#[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
pub use drm_impl::DrmBackend;

/// What other threads send a running app through a [`Messenger`]. Only
/// the display backends run one.
#[cfg_attr(not(any(feature = "winit", all(target_os = "linux", feature = "evdev", any(feature = "drm", feature = "linuxfb")))), allow(dead_code))]
pub(crate) enum Command<M: crate::Model> {
    /// A message for [`Model::update`](crate::Model::update), parsed like a
    /// template's `data-on-click` message.
    Message(String),
    /// Changes the model directly.
    Update(Box<dyn FnOnce(&mut M, &mut crate::Context<M::Message>) + Send>),
    /// Only wakes the loop (e.g. a background image load finished).
    #[cfg_attr(not(feature = "winit"), allow(dead_code))]
    Wake,
    /// Ends the app: its `run` returns.
    Quit,
}

/// Sends messages and changes into a running app from other threads
/// (sensor readers, network clients, a program the UI is part of), as if
/// they came from the UI.
pub struct Messenger<M: crate::Model>(std::sync::Arc<dyn Fn(Command<M>) -> Result<(), SendError> + Send + Sync>);

impl<M: crate::Model> Clone for Messenger<M> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<M: crate::Model> Messenger<M> {
    /// A messenger handing its commands to `send`, which wakes the loop.
    #[cfg_attr(not(any(feature = "winit", all(target_os = "linux", feature = "evdev", any(feature = "drm", feature = "linuxfb")))), allow(dead_code))]
    pub(crate) fn new(send: impl Fn(Command<M>) -> Result<(), SendError> + Send + Sync + 'static) -> Self {
        Self(std::sync::Arc::new(send))
    }

    /// Queues `message` for [`Model::update`](crate::Model::update); it is
    /// parsed like a template's `data-on-click` message.
    pub fn send(&self, message: impl Into<String>) -> Result<(), SendError> {
        (self.0)(Command::Message(message.into()))
    }

    /// Runs `f` on the model in the app's thread, as if it were an
    /// `update`; the view follows.
    pub fn update(&self, f: impl FnOnce(&mut M, &mut crate::Context<M::Message>) + Send + 'static) -> Result<(), SendError> {
        (self.0)(Command::Update(Box::new(f)))
    }

    /// Ends the app: the backend's `run` (and [`App::run`]) returns.
    pub fn quit(&self) -> Result<(), SendError> {
        (self.0)(Command::Quit)
    }
}

/// A [`Messenger`] whose app is gone, or that has none.
#[derive(Debug)]
pub struct SendError(pub String);

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to send event: {}", self.0)
    }
}
impl std::error::Error for SendError {}

/// A [`Messenger`] over a channel, waking the embedded loop's thread.
#[cfg(any(
    all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
    all(target_os = "linux", feature = "drm", feature = "evdev")
))]
pub(crate) fn channel_messenger<M: crate::Model + 'static>(sender: std::sync::mpsc::Sender<Command<M>>, loop_thread: std::thread::Thread) -> Messenger<M> {
    Messenger::new(move |command| {
        sender.send(command).map_err(|e| SendError(e.to_string()))?;
        loop_thread.unpark();
        Ok(())
    })
}

/// Error representation when initializing or executing windowing/hardware backends.
#[derive(Debug)]
#[non_exhaustive]
pub enum BackendError {
    /// Error during platform initialization.
    Init(String),
    /// Error during application execution.
    Run(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BackendError::Init(s) => write!(f, "Initialization error: {}", s),
            BackendError::Run(s) => write!(f, "Execution error: {}", s),
        }
    }
}
impl std::error::Error for BackendError {}

/// Trait defining an operating system or hardware display backend.
pub trait Backend {
    /// Runs the backend application event loop.
    ///
    /// `renderer` draws each frame into the display's pixels, which the
    /// backend describes (size, format, rotation) as a [`crate::Surface`].
    /// `setup` gets a [`Messenger`] once the app runs.
    fn run<M, TM, R>(
        self,
        title: &str,
        width: u32,
        height: u32,
        runtime: crate::Runtime<M, TM>,
        renderer: R,
        setup: impl FnOnce(Messenger<M>) + 'static,
    ) -> Result<(), BackendError>
    where
        M: crate::Model + crate::ui::TemplateLayout + 'static,
        TM: crate::TextMeasurer + 'static,
        R: crate::Renderer + 'static;
}
