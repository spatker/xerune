#[cfg(feature = "winit")]
pub(crate) mod winit_impl;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
pub(crate) mod linuxfb_impl;

#[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
pub(crate) mod drm_impl;

#[cfg(feature = "winit")]
pub use winit_impl::WinitBackend;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
pub use linuxfb_impl::LinuxFbBackend;

#[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
pub use drm_impl::DrmBackend;

/// Trait defining a message-passing proxy wrapper to dispatch events back to the MVU event loop.
pub trait EventProxy: Send + Sync + Clone + 'static {
    /// Send an event message string.
    fn send_message(&self, message: String) -> Result<(), SendError>;
}

/// Error type when event transmission fails.
#[derive(Debug)]
pub struct SendError(pub String);

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Failed to send event: {}", self.0)
    }
}
impl std::error::Error for SendError {}

/// Standard MPSC channel proxy wrapper implementing EventProxy.
#[cfg(any(
    all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
    all(target_os = "linux", feature = "drm", feature = "evdev")
))]
#[derive(Clone)]
pub struct MpscProxy {
    pub(crate) sender: std::sync::mpsc::Sender<String>,
}

#[cfg(any(
    all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
    all(target_os = "linux", feature = "drm", feature = "evdev")
))]
impl EventProxy for MpscProxy {
    fn send_message(&self, message: String) -> Result<(), SendError> {
        self.sender.send(message)
            .map_err(|e| SendError(e.to_string()))
    }
}

/// Error representation when initializing or executing windowing/hardware backends.
#[derive(Debug)]
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
    /// Associated EventProxy type.
    type Proxy: EventProxy + Clone + Send + 'static;

    /// Runs the backend application event loop.
    fn run<M, TM, F>(
        self,
        title: &str,
        width: u32,
        height: u32,
        runtime: crate::Runtime<M, TM>,
        render_fn: F,
        setup: impl FnOnce(Self::Proxy) + 'static,
    ) -> Result<(), BackendError>
    where
        M: crate::Model + crate::ui::TemplateLayout + 'static,
        TM: crate::TextMeasurer + 'static,
        F: FnMut(&mut crate::Runtime<M, TM>, &mut [u32], u32, u32) + 'static;
}
