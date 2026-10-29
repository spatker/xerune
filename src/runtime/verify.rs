//! `XERUNE_VERIFY=1` / [`Runtime::set_verify`]: checks the runtime's
//! shortcuts against the long way round while an app runs, and reports
//! where they differ.
//!
//! - A rebuilt view reuses the previous layout tree (`Ui::reuse_layout`)
//!   and cached styles; it must draw what a view built and laid out from
//!   scratch draws.
//! - A view [`Model::view_fingerprint`] says is unchanged is not rebuilt;
//!   building it anyway must draw the same, or the fingerprint misses
//!   some of the state the view shows.
//! - A frame repaints only the damaged areas; the result must equal a
//!   full repaint.
//!
//! Every check costs a view build or a full repaint, so this is for
//! development, on a desktop or on the board, not for shipping.
//!
//! [`Runtime::set_verify`]: crate::Runtime::set_verify
//! [`Model::view_fingerprint`]: crate::Model::view_fingerprint

use crate::alloc_prelude::*;
use crate::graphics::{Damage, DrawCommand, Pixels, Rect, RenderResources, Renderer, Surface};

/// What has been checked so far, and the incrementally drawn frame.
#[derive(Default)]
pub(crate) struct Verifier {
    /// Checks that found a difference.
    pub(crate) failures: usize,
    /// The frames as the damaged areas drew them, at `size`; `None` until a
    /// full frame was drawn into it.
    shadow: Option<Vec<u32>>,
    size: (u32, u32),
}

impl Verifier {
    /// Compares the commands of the view the runtime shows (`actual`) with
    /// those of the view built from scratch (`expected`).
    pub(crate) fn compare_views(&mut self, what: &str, expected: &[DrawCommand], actual: &[DrawCommand]) {
        if expected == actual {
            return;
        }
        self.failures += 1;
        let index = expected.iter().zip(actual).position(|(e, a)| e != a).unwrap_or(expected.len().min(actual.len()));
        let show = |command: Option<&DrawCommand>| {
            let mut text = command.map_or("nothing".into(), |c| format!("{c:?}"));
            text.truncate(300);
            text
        };
        report(&format!(
            "{what} draws differently from the view built from scratch ({} vs {} commands); first difference, command {index}:\n  shown:   {}\n  scratch: {}",
            actual.len(),
            expected.len(),
            show(actual.get(index)),
            show(expected.get(index)),
        ));
    }

    /// Draws `damage` into the shadow frame, as the runtime drew it into
    /// the target, and compares the result with a full repaint.
    pub(crate) fn check_frame(
        &mut self,
        renderer: &mut impl Renderer,
        commands: &[DrawCommand],
        resources: &RenderResources<'_>,
        damage: &Damage,
        (width, height): (u32, u32),
    ) {
        let len = width as usize * height as usize;
        let mut reference = vec![0u32; len];
        let screen = Rect::new(0.0, 0.0, width as f32, height as f32);
        renderer.render(&mut Surface::new(Pixels::Xrgb8888(&mut reference), width, height), commands, resources, screen);
        if self.size != (width, height) {
            self.shadow = None;
            self.size = (width, height);
        }
        // Enabled mid-run or resized: the shadow starts from this frame.
        let Some(shadow) = &mut self.shadow else {
            self.shadow = Some(reference);
            return;
        };
        let mut target = Surface::new(Pixels::Xrgb8888(shadow), width, height);
        for area in damage.rects() {
            renderer.render(&mut target, commands, resources, *area);
        }
        let mut differing = 0usize;
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (i, (s, r)) in shadow.iter().zip(&reference).enumerate() {
            if s != r {
                differing += 1;
                let (x, y) = ((i % width as usize) as u32, (i / width as usize) as u32);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
        if differing > 0 {
            self.failures += 1;
            report(&format!(
                "repainting the damaged areas {:?} left {differing} pixels different from a full repaint, within x {x0}..={x1}, y {y0}..={y1}",
                damage.rects(),
            ));
            // Report the next difference, not this one again.
            *shadow = reference;
        }
    }
}

fn report(message: &str) {
    #[cfg(feature = "std")]
    std::eprintln!("xerune verify: {message}");
    #[cfg(not(feature = "std"))]
    log::warn!("xerune verify: {message}");
}
