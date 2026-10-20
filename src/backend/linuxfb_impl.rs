use crate::{Model, Runtime, TextMeasurer};
use super::{
    Backend, BackendError, Messenger, channel_messenger,
    input::{EvdevInputSource, SurfaceInfo},
    common_loop::{FramePresenter, run_embedded_event_loop},
};
use std::sync::mpsc::channel;
use linuxfb::Framebuffer;

/// Backend implementation for Linux framebuffers (/dev/fb0).
pub struct LinuxFbBackend;

impl LinuxFbBackend {
    /// Create a new LinuxFbBackend.
    pub fn new() -> Self {
        Self
    }
}

struct LinuxFbPresenter<M> {
    fb: Framebuffer,
    fb_mmap: M,
    bytes_per_pixel: usize,
    fb_w: u32,
    fb_h: u32,
    double_buffered: bool,
    active_page: usize,
    page0_needs_full: bool,
    page1_needs_full: bool,
    prev_damage: Option<crate::graphics::Rect>,
}

impl<M> FramePresenter for LinuxFbPresenter<M>
where
    M: std::ops::DerefMut<Target = [u8]>,
{
    fn present(&mut self, local_buffer: &[u32], surface: &SurfaceInfo, damage: Option<crate::graphics::Rect>) -> Result<(), BackendError> {
        let combined_damage = if self.double_buffered {
            let target_page = if self.active_page == 0 { 1 } else { 0 };
            let target_needs_full = if target_page == 1 {
                let needs = self.page1_needs_full;
                self.page1_needs_full = false;
                needs
            } else {
                let needs = self.page0_needs_full;
                self.page0_needs_full = false;
                needs
            };

            if target_needs_full {
                None
            } else {
                match (damage, self.prev_damage) {
                    (Some(d), Some(p)) => Some(d.expand(p)),
                    _ => None,
                }
            }
        } else {
            damage
        };
        self.prev_damage = damage;

        // The frame is upright; the display may be mounted turned.
        let screen = crate::graphics::Rect::new(0.0, 0.0, surface.logical_w as f32, surface.logical_h as f32);
        let rotation = crate::Rotation::from_degrees(surface.rotation).unwrap_or_default();

        if self.bytes_per_pixel == 4 {
            let page_size = (self.fb_w * self.fb_h * 4) as usize;
            let mmap_len = self.fb_mmap.len();
            
            let y_offset = if self.double_buffered && mmap_len >= page_size * 2 {
                if self.active_page == 0 { self.fb_h } else { 0 }
            } else {
                0
            };
            
            let target_offset = (y_offset * self.fb_w * 4) as usize;
            let draw_slice = if target_offset + page_size <= mmap_len {
                &mut self.fb_mmap[target_offset..target_offset + page_size]
            } else {
                &mut self.fb_mmap[0..page_size]
            };

            let display: &mut [u32] = bytemuck::cast_slice_mut(draw_slice);
            rotation.copy_frame(local_buffer, surface.logical_w, surface.logical_h, combined_damage.unwrap_or(screen), display, |p| p);

            if self.double_buffered && mmap_len >= page_size * 2 {
                if let Err(e) = self.fb.set_offset(0, y_offset) {
                    log::warn!("Failed to flip page: {:?}", e);
                } else {
                    self.active_page = if self.active_page == 0 { 1 } else { 0 };
                }
            } else {
                let _ = self.fb.set_offset(0, 0);
            }
        } else if self.bytes_per_pixel == 2 {
            let page = (self.fb_w * self.fb_h * 2) as usize;
            let display: &mut [u16] = bytemuck::cast_slice_mut(&mut self.fb_mmap[..page]);
            rotation.copy_frame(local_buffer, surface.logical_w, surface.logical_h, combined_damage.unwrap_or(screen), display, |pixel| {
                let (r, g, b) = ((pixel >> 16) & 0xff, (pixel >> 8) & 0xff, pixel & 0xff);
                (((r & 0xf8) << 8) | ((g & 0xfc) << 3) | (b >> 3)) as u16
            });
            let _ = self.fb.set_offset(0, 0);
        }

        Ok(())
    }
}

impl Backend for LinuxFbBackend {
    fn run<M, TM, R>(
        self,
        title: &str,
        width: u32,
        height: u32,
        runtime: Runtime<M, TM>,
        renderer: R,
        setup: impl FnOnce(Messenger<M>) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        R: crate::Renderer + 'static,
    {
        // `XERUNE_SNAPSHOT=out.png` renders headlessly instead (see `backend::snapshot`).
        if let Some(snapshot) = super::SnapshotBackend::from_env() {
            return snapshot?.run(title, width, height, runtime, renderer, |_| {});
        }
        log::info!("Initializing Framebuffer Backend...");
        let mut fb = Framebuffer::new("/dev/fb0")
            .map_err(|e| BackendError::Init(format!("Failed to open framebuffer: {:?}", e)))?;
        
        let (fb_w, fb_h) = fb.get_size();
        let bytes_per_pixel = fb.get_bytes_per_pixel();
        let bpp = bytes_per_pixel * 8;
        
        let rotate = fb_w < fb_h || std::env::var("XERUNE_ROTATION").map(|s| s == "90" || s == "270").unwrap_or(false);
        let rotation = if rotate { 90 } else { 0 };
        let (w, h) = if rotate { (fb_h, fb_w) } else { (fb_w, fb_h) };
        
        let layout = fb.get_pixel_layout();
        let fb_is_bgra = layout.blue.offset < layout.red.offset;
        log::info!("Framebuffer: {}x{} @ {}bpp, is_bgra: {}", fb_w, fb_h, bpp, fb_is_bgra);
        
        let mut double_buffered = false;
        if let Err(e) = fb.set_virtual_size(fb_w, fb_h * 2) {
            log::warn!("Could not set virtual size for hardware double buffering: {:?}", e);
        } else {
             let (vw, vh) = fb.get_virtual_size();
             if vh >= fb_h * 2 {
                 log::info!("Hardware Double Buffering activated seamlessly (Virtual size: {}x{})", vw, vh);
                 double_buffered = true;
             }
        }
        
        let fb_mmap = fb.map()
            .map_err(|e| BackendError::Init(format!("Failed to map framebuffer: {:?}", e)))?;
        let _ = fb.set_offset(0, 0);

        let (msg_tx, msg_rx) = channel::<super::Command<M>>();
        let event_loop_thread = std::thread::current();
        setup(channel_messenger(msg_tx, event_loop_thread.clone()));

        let input_source = EvdevInputSource::with_waker(Some(event_loop_thread));
        let presenter = LinuxFbPresenter {
            fb,
            fb_mmap,
            bytes_per_pixel: bytes_per_pixel as usize,
            fb_w,
            fb_h,
            double_buffered,
            active_page: 0,
            page0_needs_full: true,
            page1_needs_full: true,
            prev_damage: None,
        };
        let surface = SurfaceInfo {
            logical_w: w,
            logical_h: h,
            disp_w: fb_w,
            disp_h: fb_h,
            rotation,
        };

        run_embedded_event_loop(runtime, renderer, input_source, presenter, surface, msg_rx)
    }
}
