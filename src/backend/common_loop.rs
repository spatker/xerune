use std::time::Instant;
use std::sync::mpsc::Receiver;
use crate::{Model, Runtime, TextMeasurer, graphics::Rect};
use super::{BackendError, Command, input::{InputSource, SurfaceInfo}};

/// Trait defining a display surface frame presenter.
pub trait FramePresenter {
    /// Sync step called before event polling and rendering (e.g. waiting for DRM page flip completion).
    fn prepare_frame(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    /// Present/blit the rendered logical ARGB8888 buffer to the display device.
    fn present(&mut self, local_buffer: &[u32], surface: &SurfaceInfo, damage: Option<Rect>) -> Result<(), BackendError>;
}

/// Generic event loop for embedded Linux backends, until a
/// [`Command::Quit`]. Input and message producers must unpark this thread
/// after queuing events.
pub fn run_embedded_event_loop<M, TM, R, I, P>(
    mut runtime: Runtime<M, TM>,
    mut renderer: R,
    mut input_source: I,
    mut presenter: P,
    surface: SurfaceInfo,
    msg_rx: Receiver<Command<M>>,
) -> Result<(), BackendError>
where
    M: Model + crate::ui::TemplateLayout + 'static,
    TM: TextMeasurer + 'static,
    R: crate::Renderer,
    I: InputSource,
    P: FramePresenter,
{
    let w = surface.logical_w;
    let h = surface.logical_h;

    runtime.set_size(w as f32, h as f32);

    // Background image loads unpark this thread; the next `poll()` applies them.
    let loop_thread = std::thread::current();
    runtime.set_image_waker(move || loop_thread.unpark());

    let mut force_redraw = true;
    let mut local_buffer = vec![0xFF222222u32; (surface.disp_w * surface.disp_h) as usize];

    loop {
        presenter.prepare_frame()?;

        let mut dirty = force_redraw;
        let is_forced = force_redraw;
        force_redraw = false;

        // Poll & dispatch hardware input
        dirty |= input_source.dispatch_events(&mut runtime, &surface)?;

        // Messages and changes from other threads, in order.
        let mut messages = Vec::new();
        let mut received = 0;
        while received <= 300 {
            let Ok(command) = msg_rx.try_recv() else { break };
            received += 1;
            match command {
                Command::Message(message) => messages.push(message),
                Command::Update(f) => {
                    if !messages.is_empty() {
                        dirty |= runtime.handle_messages(messages.drain(..));
                    }
                    dirty |= runtime.update_with(f);
                }
                Command::Wake => {}
                Command::Quit => return Ok(()),
            }
        }
        let batch_full = received > 300;
        if !messages.is_empty() {
            dirty |= runtime.handle_messages(messages);
        }

        // Timers, subscriptions, frames.
        let poll = runtime.poll();
        dirty |= poll.redraw;

        // Render and present
        if dirty {
            // Drawn upright; the presenter turns it for the display as it
            // copies it, which costs far less than drawing turned.
            let mut target = crate::Surface::new(crate::Pixels::Xrgb8888(&mut local_buffer), w, h);
            let damage = runtime.render(&mut renderer, &mut target);
            // Presenters copy one rectangle (`None`: everything); an empty one
            // when nothing changed.
            let effective_damage = if is_forced {
                None
            } else {
                Some(damage.bounds().unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)))
            };
            presenter.present(&local_buffer, &surface, effective_damage)?;
        }

        // Input and proxy messages both unpark this thread. A wake sent before
        // parking is retained, so events arriving during rendering are not lost.
        // The runtime paces frames; never sleep with a full message batch.
        if !batch_full {
            match poll.wake.deadline() {
                Some(at) => {
                    let sleep = at.saturating_duration_since(Instant::now());
                    if !sleep.is_zero() {
                        std::thread::park_timeout(sleep);
                    }
                }
                None => std::thread::park(),
            }
        }
    }
}

#[cfg(all(test, any(feature = "drm", feature = "linuxfb")))]
#[path = "common_loop_tests.rs"]
mod tests;
