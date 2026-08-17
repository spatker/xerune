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

fn logical_rect_to_phys_bounds(r: &crate::graphics::Rect, surface: &SurfaceInfo) -> (usize, usize, usize, usize) {
    let disp_w = surface.disp_w as usize;
    let disp_h = surface.disp_h as usize;
    match surface.rotation {
        90 => {
            let p_min_y = (r.x.max(0.0) as usize).min(disp_h);
            let p_max_y = ((r.x + r.width).ceil().max(0.0) as usize).min(disp_h);
            let p_min_x = disp_w.saturating_sub((r.y + r.height).ceil().max(0.0) as usize).min(disp_w);
            let p_max_x = disp_w.saturating_sub(r.y.max(0.0) as usize).min(disp_w);
            (p_min_x, p_min_y, p_max_x, p_max_y)
        }
        180 => {
            let p_min_x = disp_w.saturating_sub((r.x + r.width).ceil().max(0.0) as usize).min(disp_w);
            let p_max_x = disp_w.saturating_sub(r.x.max(0.0) as usize).min(disp_w);
            let p_min_y = disp_h.saturating_sub((r.y + r.height).ceil().max(0.0) as usize).min(disp_h);
            let p_max_y = disp_h.saturating_sub(r.y.max(0.0) as usize).min(disp_h);
            (p_min_x, p_min_y, p_max_x, p_max_y)
        }
        270 => {
            let p_min_x = (r.y.max(0.0) as usize).min(disp_w);
            let p_max_x = ((r.y + r.height).ceil().max(0.0) as usize).min(disp_w);
            let p_min_y = disp_h.saturating_sub((r.x + r.width).ceil().max(0.0) as usize).min(disp_h);
            let p_max_y = disp_h.saturating_sub(r.x.max(0.0) as usize).min(disp_h);
            (p_min_x, p_min_y, p_max_x, p_max_y)
        }
        _ => {
            let p_min_x = (r.x.max(0.0) as usize).min(disp_w);
            let p_max_x = ((r.x + r.width).ceil().max(0.0) as usize).min(disp_w);
            let p_min_y = (r.y.max(0.0) as usize).min(disp_h);
            let p_max_y = ((r.y + r.height).ceil().max(0.0) as usize).min(disp_h);
            (p_min_x, p_min_y, p_max_x, p_max_y)
        }
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

            let local_bytes: &[u8] = bytemuck::cast_slice(local_buffer);
            let disp_w = surface.disp_w as usize;

            if let Some(ref rect) = combined_damage {
                let (min_x, min_y, max_x, max_y) = logical_rect_to_phys_bounds(rect, surface);
                if min_x < max_x && min_y < max_y {
                    let row_bytes = (max_x - min_x) * 4;
                    let stride_bytes = disp_w * 4;
                    for y in min_y..max_y {
                        let row_offset = y * stride_bytes + min_x * 4;
                        if row_offset + row_bytes <= draw_slice.len() && row_offset + row_bytes <= local_bytes.len() {
                            draw_slice[row_offset..row_offset + row_bytes]
                                .copy_from_slice(&local_bytes[row_offset..row_offset + row_bytes]);
                        }
                    }
                }
            } else {
                let copy_len = local_bytes.len().min(draw_slice.len());
                draw_slice[..copy_len].copy_from_slice(&local_bytes[..copy_len]);
            }

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
            let fb_w = self.fb_w as usize;
            let fb_h = self.fb_h as usize;

            if let Some(ref rect) = combined_damage {
                let (min_x, min_y, max_x, max_y) = logical_rect_to_phys_bounds(rect, surface);
                for y in min_y..max_y.min(fb_h) {
                    for x in min_x..max_x.min(fb_w) {
                        let idx = y * fb_w + x;
                        if idx < local_buffer.len() {
                            unsafe {
                                let pixel = local_buffer[idx];
                                let r = ((pixel >> 16) & 0xFF) as u16;
                                let g = ((pixel >> 8) & 0xFF) as u16;
                                let b = (pixel & 0xFF) as u16;
                                let rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                                let d = dest_ptr.add(idx * 2) as *mut u16;
                                d.write_unaligned(rgb565);
                            }
                        }
                    }
                }
            } else {
                let total_pixels = fb_w * fb_h;
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
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) -> Option<crate::graphics::Rect> + 'static,
    {
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

        run_embedded_event_loop(runtime, render_fn, input_source, presenter, surface, msg_rx)
    }
}
