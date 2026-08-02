use crate::{Model, Runtime, TextMeasurer};
use super::{
    Backend, BackendError, MpscProxy,
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
}

impl<M> FramePresenter for LinuxFbPresenter<M>
where
    M: std::ops::DerefMut<Target = [u8]>,
{
    fn present(&mut self, local_buffer: &[u32], _surface: &SurfaceInfo) -> Result<(), BackendError> {
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

            let draw_slice_u32 = unsafe {
                std::slice::from_raw_parts_mut(
                    draw_slice.as_mut_ptr() as *mut u32,
                    draw_slice.len() / 4,
                )
            };

            draw_slice_u32.copy_from_slice(local_buffer);

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
            let dest_ptr = self.fb_mmap.as_mut_ptr();
            let total_pixels = (self.fb_w * self.fb_h) as usize;
            for i in 0..total_pixels.min(local_buffer.len()) {
                unsafe {
                    let pixel = local_buffer[i];
                    let r = ((pixel >> 16) & 0xFF) as u16;
                    let g = ((pixel >> 8) & 0xFF) as u16;
                    let b = (pixel & 0xFF) as u16;
                    let rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                    let d = dest_ptr.add(i * 2) as *mut u16;
                    d.write_unaligned(rgb565);
                }
            }
            let _ = self.fb.set_offset(0, 0);
        }

        Ok(())
    }
}

impl Backend for LinuxFbBackend {
    type Proxy = MpscProxy;

    fn run<M, TM, F>(
        self,
        _title: &str,
        _width: u32,
        _height: u32,
        runtime: Runtime<M, TM>,
        render_fn: F,
        setup: impl FnOnce(Self::Proxy) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) + 'static,
    {
        println!("Initializing Framebuffer Backend...");
        let mut fb = Framebuffer::new("/dev/fb0")
            .map_err(|e| BackendError::Init(format!("Failed to open framebuffer: {:?}", e)))?;
        
        let (fb_w, fb_h) = fb.get_size();
        let bytes_per_pixel = fb.get_bytes_per_pixel();
        let bpp = bytes_per_pixel * 8;
        
        let rotate = fb_w < fb_h || std::env::var("XERUNE_ROTATION").map(|s| s == "90" || s == "270").unwrap_or(false);
        let rotation = if rotate { 90 } else { 0 };
        if rotate && std::env::var("XERUNE_ROTATION").is_err() {
            unsafe { std::env::set_var("XERUNE_ROTATION", "90"); }
        }
        let (w, h) = if rotate { (fb_h, fb_w) } else { (fb_w, fb_h) };
        
        let layout = fb.get_pixel_layout();
        let fb_is_bgra = layout.blue.offset < layout.red.offset;
        println!("Framebuffer: {}x{} @ {}bpp, is_bgra: {}", fb_w, fb_h, bpp, fb_is_bgra);
        
        let mut double_buffered = false;
        if let Err(e) = fb.set_virtual_size(fb_w, fb_h * 2) {
            println!("Warning: Could not set virtual size for hardware double buffering: {:?}", e);
        } else {
             let (vw, vh) = fb.get_virtual_size();
             if vh >= fb_h * 2 {
                 println!("Hardware Double Buffering activated seamlessly (Virtual size: {}x{})", vw, vh);
                 double_buffered = true;
             }
        }
        
        let fb_mmap = fb.map()
            .map_err(|e| BackendError::Init(format!("Failed to map framebuffer: {:?}", e)))?;
        let _ = fb.set_offset(0, 0);

        let (msg_tx, msg_rx) = channel::<String>();
        setup(MpscProxy { sender: msg_tx });

        let input_source = EvdevInputSource::new();
        let presenter = LinuxFbPresenter {
            fb,
            fb_mmap,
            bytes_per_pixel: bytes_per_pixel as usize,
            fb_w,
            fb_h,
            double_buffered,
            active_page: 0,
        };
        let surface = SurfaceInfo {
            logical_w: w,
            logical_h: h,
            disp_w: fb_w,
            disp_h: fb_h,
            rotation,
        };

        run_embedded_event_loop(runtime, render_fn, input_source, presenter, surface, msg_rx)
    }
}
