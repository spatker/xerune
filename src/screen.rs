//! Global viewport (screen) state and breakpoint helpers.
//!
//! The active viewport size is kept in process-global atomics so that it can be
//! queried from anywhere at any time: the CSS engine (for `vw`/`vh` units),
//! the media-query matcher (for `@media` rules in compiled templates), and
//! application code via [`width`], [`height`] and [`breakpoint`].
//!
//! The [`crate::runtime::Runtime`] updates these values whenever the underlying
//! surface/window is resized (via [`crate::Model::on_resize`]).

use core::sync::atomic::{AtomicU32, Ordering};

/// Semantic screen-size buckets used by [`breakpoint`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    /// Viewport width below [`MOBILE_MAX`] (small handheld/kiosk displays).
    Mobile,
    /// Viewport width below [`TABLET_MAX`] (mid-size displays).
    Tablet,
    /// Viewport width at or above [`TABLET_MAX`].
    Desktop,
}

/// Upper bound (exclusive) of the mobile breakpoint in CSS pixels.
pub const MOBILE_MAX: f32 = 600.0;
/// Upper bound (exclusive) of the tablet breakpoint in CSS pixels.
pub const TABLET_MAX: f32 = 1024.0;

/// Width of the viewport in CSS pixels. `0.0` until a runtime first reports a size.
static VIEWPORT_WIDTH: AtomicU32 = AtomicU32::new(0);
/// Height of the viewport in CSS pixels. `0.0` until a runtime first reports a size.
static VIEWPORT_HEIGHT: AtomicU32 = AtomicU32::new(0);

/// Updates the global viewport size. Called by the runtime; rarely needed directly.
pub fn set_viewport(width: f32, height: f32) {
    VIEWPORT_WIDTH.store(width.to_bits(), Ordering::Relaxed);
    VIEWPORT_HEIGHT.store(height.to_bits(), Ordering::Relaxed);
}

/// Current viewport width in CSS pixels (0.0 when unknown).
pub fn width() -> f32 {
    f32::from_bits(VIEWPORT_WIDTH.load(Ordering::Relaxed))
}

/// Current viewport height in CSS pixels (0.0 when unknown).
pub fn height() -> f32 {
    f32::from_bits(VIEWPORT_HEIGHT.load(Ordering::Relaxed))
}

/// Current viewport size in CSS pixels as `(width, height)`.
pub fn size() -> (f32, f32) {
    (width(), height())
}

/// Classifies the current viewport width into a semantic [`Breakpoint`].
pub fn breakpoint() -> Breakpoint {
    let w = width();
    if w < MOBILE_MAX {
        Breakpoint::Mobile
    } else if w < TABLET_MAX {
        Breakpoint::Tablet
    } else {
        Breakpoint::Desktop
    }
}

/// Returns true if the current viewport width is below [`MOBILE_MAX`].
pub fn is_mobile() -> bool {
    breakpoint() == Breakpoint::Mobile
}

/// Returns true if the current viewport width is in the tablet range.
pub fn is_tablet() -> bool {
    breakpoint() == Breakpoint::Tablet
}

/// Returns true if the current viewport width is at or above [`TABLET_MAX`].
pub fn is_desktop() -> bool {
    breakpoint() == Breakpoint::Desktop
}
