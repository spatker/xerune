use std::time::Instant;
use std::sync::mpsc::Receiver;
use crate::{Model, Runtime, TextMeasurer, graphics::Rect};
use super::{BackendError, input::{InputSource, SurfaceInfo}};

/// Trait defining a display surface frame presenter.
pub trait FramePresenter {
    /// Sync step called before event polling and rendering (e.g. waiting for DRM page flip completion).
    fn prepare_frame(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    /// Present/blit the rendered logical ARGB8888 buffer to the display device.
    fn present(&mut self, local_buffer: &[u32], surface: &SurfaceInfo, damage: Option<Rect>) -> Result<(), BackendError>;
}

/// Generic event loop for embedded Linux backends.
/// Input and message producers must unpark this thread after queuing events.
pub fn run_embedded_event_loop<M, TM, F, I, P>(
    mut runtime: Runtime<M, TM>,
    mut render_fn: F,
    mut input_source: I,
    mut presenter: P,
    surface: SurfaceInfo,
    msg_rx: Receiver<String>,
) -> Result<(), BackendError>
where
    M: Model + crate::ui::TemplateLayout + 'static,
    TM: TextMeasurer + 'static,
    F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) -> Option<Rect> + 'static,
    I: InputSource,
    P: FramePresenter,
{
    let w = surface.logical_w;
    let h = surface.logical_h;

    runtime.set_size(w as f32, h as f32);

    // Background image loads unpark this thread; the next `tick()` applies them.
    let loop_thread = std::thread::current();
    runtime.set_image_waker(move || loop_thread.unpark());

    let mut force_redraw = true;
    let mut local_buffer = vec![0xFF222222u32; (surface.disp_w * surface.disp_h) as usize];

    loop {
        let frame_start = Instant::now();

        presenter.prepare_frame()?;

        let mut dirty = force_redraw;
        let is_forced = force_redraw;
        force_redraw = false;

        // Poll & dispatch hardware input
        dirty |= input_source.dispatch_events(&mut runtime, &surface)?;

        // Process custom proxy messages
        let mut messages = Vec::new();
        while let Ok(msg) = msg_rx.try_recv() {
            messages.push(msg);
            if messages.len() > 300 {
                break;
            }
        }
        let batch_full = messages.len() > 300;
        if !messages.is_empty() {
            dirty |= runtime.handle_messages(messages);
        }

        // Update runtime state
        let tick_res = runtime.tick();
        dirty |= tick_res.needs_redraw;

        // Render and present
        if dirty {
            let damage = render_fn(&mut runtime, &mut local_buffer, w, h);
            let effective_damage = if is_forced { None } else { damage };
            presenter.present(&local_buffer, &surface, effective_damage)?;
        }

        // Input and proxy messages both unpark this thread. A wake sent before
        // parking is retained, so events arriving during rendering are not lost.
        // Also pace dirty frames, and never sleep with a full message batch.
        if !batch_full {
            if let Some(sleep_dur) = tick_res.next_tick_in.checked_sub(frame_start.elapsed()) {
                if !sleep_dur.is_zero() {
                    std::thread::park_timeout(sleep_dur);
                }
            }
        }
    }
}

#[cfg(all(test, any(feature = "drm", feature = "linuxfb")))]
#[path = "common_loop_tests.rs"]
mod tests;
