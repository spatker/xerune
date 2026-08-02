use std::time::Instant;
use std::sync::mpsc::Receiver;
use crate::{Model, Runtime, TextMeasurer};
use super::{BackendError, input::{InputSource, SurfaceInfo}};

/// Trait defining a display surface frame presenter.
pub trait FramePresenter {
    /// Sync step called before event polling and rendering (e.g. waiting for DRM page flip completion).
    fn prepare_frame(&mut self) -> Result<(), BackendError> {
        Ok(())
    }

    /// Present/blit the rendered logical ARGB8888 buffer to the display device.
    fn present(&mut self, local_buffer: &[u32], surface: &SurfaceInfo) -> Result<(), BackendError>;
}

/// Generic event loop for embedded Linux backends.
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
    F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) + 'static,
    I: InputSource,
    P: FramePresenter,
{
    let w = surface.logical_w;
    let h = surface.logical_h;

    runtime.set_size(w as f32, h as f32);

    let mut force_redraw = true;
    let mut local_buffer = vec![0xFF222222u32; (surface.disp_w * surface.disp_h) as usize];

    loop {
        let frame_start = Instant::now();

        presenter.prepare_frame()?;

        let mut dirty = force_redraw;
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
        if !messages.is_empty() {
            dirty |= runtime.handle_messages(messages);
        }

        // Update runtime state
        let tick_res = runtime.tick();
        dirty |= tick_res.needs_redraw;

        // Render and present
        if dirty {
            render_fn(&mut runtime, &mut local_buffer, w, h);
            presenter.present(&local_buffer, &surface)?;
        }

        // Dynamic sleeping & idle frame pacing
        if !dirty {
            let elapsed = frame_start.elapsed();
            let is_idle = tick_res.next_tick_in > std::time::Duration::from_secs(3600);
            if is_idle {
                input_source.wait_for_event(None);
            } else if let Some(sleep_dur) = tick_res.next_tick_in.checked_sub(elapsed) {
                if !sleep_dur.is_zero() {
                    input_source.wait_for_event(Some(sleep_dur));
                }
            }
        }
    }
}
