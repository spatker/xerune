/// Hardware input abstraction interface.
#[cfg(feature = "evdev")]
pub mod evdev;

#[cfg(feature = "evdev")]
pub use evdev::EvdevInputSource;

use crate::{Model, Runtime, TextMeasurer};
use super::BackendError;

/// Geometry and dimension context for display surfaces.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceInfo {
    /// Logical layout width.
    pub logical_w: u32,
    /// Logical layout height.
    pub logical_h: u32,
    /// Physical display width.
    pub disp_w: u32,
    /// Physical display height.
    pub disp_h: u32,
    /// Display rotation in degrees (0, 90, 180, 270).
    pub rotation: u32,
}

/// Trait defining an input event source.
pub trait InputSource {
    /// Poll or drain input events, translating and dispatching them to `runtime`.
    /// Returns `Ok(true)` if any dispatched event requires a display redraw.
    fn dispatch_events<M, TM>(
        &mut self,
        runtime: &mut Runtime<M, TM>,
        surface: &SurfaceInfo,
    ) -> Result<bool, BackendError>
    where
        M: Model + crate::ui::TemplateLayout,
        TM: TextMeasurer;

    /// Wait for an incoming input event or until `timeout` expires (used for idle sleeping).
    fn wait_for_event(&mut self, timeout: Option<std::time::Duration>);
}
