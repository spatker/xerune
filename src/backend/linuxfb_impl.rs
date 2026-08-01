use crate::{Model, InputEvent, Runtime, TextMeasurer};
use super::{Backend, BackendError, MpscProxy};
use std::time::Instant;
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use linuxfb::Framebuffer;
use evdev::{Device, AbsoluteAxisType, InputEventKind, Key};

/// Backend implementation for Linux framebuffers (/dev/fb0).
pub struct LinuxFbBackend;

impl LinuxFbBackend {
    /// Create a new LinuxFbBackend.
    pub fn new() -> Self {
        Self
    }
}

impl Backend for LinuxFbBackend {
    type Proxy = MpscProxy;

    fn run<M, TM, F>(
        self,
        _title: &str,
        _width: u32,
        _height: u32,
        mut runtime: Runtime<M, TM>,
        mut render_fn: F,
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
        
        let rotate = fb_w < fb_h;
        let (w, h) = if rotate { (fb_h, fb_w) } else { (fb_w, fb_h) };
        
        let layout = fb.get_pixel_layout();
        let fb_is_bgra = layout.blue.offset < layout.red.offset;
        println!("Framebuffer: {}x{} @ {}bpp, is_bgra: {}", fb_w, fb_h, bpp, fb_is_bgra);
        
        let mut double_buffered = false;
        
        // Attempt to request virtual framebuffer space for true hardware page-flipping
        if let Err(e) = fb.set_virtual_size(fb_w, fb_h * 2) {
            println!("Warning: Could not set virtual size for hardware double buffering: {:?}", e);
        } else {
             let (vw, vh) = fb.get_virtual_size();
             if vh >= fb_h * 2 {
                 println!("Hardware Double Buffering activated seamlessly (Virtual size: {}x{})", vw, vh);
                 double_buffered = true;
             }
        }
        
        let mut fb_mmap = fb.map()
            .map_err(|e| BackendError::Init(format!("Failed to map framebuffer: {:?}", e)))?;
        let (rx_input, calibration) = spawn_input_thread();
        
        runtime.set_size(w as f32, h as f32);
        
        let (msg_tx, msg_rx) = channel::<String>();
        setup(MpscProxy { sender: msg_tx });
        
        let _ = fb.set_offset(0, 0); // Ensure no panning is applied
        
        let mut mouse_x = 0.0;
        let mut mouse_y = 0.0;
        let mut touch_x = 0.0;
        let mut touch_y = 0.0;
        let mut touch_down = false;
        
        let mut force_redraw = true;
        let mut active_page = 0;

        let mut local_buffer = vec![0xFF222222u32; (w * h) as usize];

        let mut buffered_event: Option<evdev::InputEvent> = None;

        loop {
            let frame_start = Instant::now();
            let mut dirty = force_redraw;
            force_redraw = false;

            // Poll Input
            while let Some(ev) = buffered_event.take().or_else(|| rx_input.try_recv().ok()) {
                match ev.kind() {
                    InputEventKind::AbsAxis(AbsoluteAxisType::ABS_X) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_X) => {
                        let raw_val = ev.value() as f32;
                        if let Some(ref cal) = calibration {
                            touch_x = ((raw_val - cal.x_min) / (cal.x_max - cal.x_min) * fb_w as f32).clamp(0.0, fb_w as f32 - 1.0);
                        } else {
                            touch_x = raw_val;
                        }
                        if rotate { mouse_y = fb_w as f32 - 1.0 - touch_x; } else { mouse_x = touch_x; }
                        
                        if touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                        }
                    },
                    InputEventKind::AbsAxis(AbsoluteAxisType::ABS_Y) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_Y) => {
                        let raw_val = ev.value() as f32;
                        if let Some(ref cal) = calibration {
                            touch_y = ((raw_val - cal.y_min) / (cal.y_max - cal.y_min) * fb_h as f32).clamp(0.0, fb_h as f32 - 1.0);
                        } else {
                            touch_y = raw_val;
                        }
                        if rotate { mouse_x = touch_y; } else { mouse_y = touch_y; }
                        
                        if touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                        }
                    },
                    InputEventKind::Key(Key::BTN_TOUCH) => {
                        if ev.value() == 1 {
                            touch_down = true;
                            dirty |= runtime.handle_event(InputEvent::TouchStart { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            touch_down = false;
                            dirty |= runtime.handle_event(InputEvent::TouchEnd { id: 0, x: mouse_x, y: mouse_y });
                        }
                    },
                    InputEventKind::Key(Key::BTN_LEFT) => {
                        if ev.value() == 1 {
                            dirty |= runtime.handle_event(InputEvent::Click { x: mouse_x, y: mouse_y });
                        }
                    },
                    InputEventKind::Key(key) => {
                        let raw_name = format!("{:?}", key);
                        let key_name = match raw_name.as_str() {
                            "KEY_UP" | "103" => "Up",
                            "KEY_DOWN" | "108" => "Down",
                            "KEY_LEFT" | "105" => "Left",
                            "KEY_RIGHT" | "106" => "Right",
                            "KEY_PLAYPAUSE" | "164" | "113" | "KEY_MUTE" => "PlayPause",
                            "KEY_NEXTSONG" | "163" | "115" | "KEY_VOLUMEUP" => "Next",
                            "KEY_PREVIOUSSONG" | "165" | "114" | "KEY_VOLUMEDOWN" => "Prev",
                            "KEY_SUSPEND" | "205" => "Back",
                            _ => &raw_name,
                        }.to_string();

                        eprintln!("[XERUNE FB INPUT] Key: {:?}, name: {}, val: {}", key, key_name, ev.value());
                        if ev.value() == 1 || ev.value() == 2 {
                            dirty |= runtime.handle_event(InputEvent::KeyDown(key_name));
                        } else if ev.value() == 0 {
                            dirty |= runtime.handle_event(InputEvent::KeyUp(key_name));
                        }
                    },
                    _ => {}
                }
            }

            // Process Custom Messages
            let mut messages = Vec::new();
            while let Ok(msg) = msg_rx.try_recv() {
                messages.push(msg);
                if messages.len() > 300 { break; } // Safety limit
            }
            if !messages.is_empty() {
                dirty |= runtime.handle_messages(messages);
            }

            // Update
            let tick_res = runtime.tick();
            dirty |= tick_res.needs_redraw;
            
            // Draw
            if dirty {
                if bytes_per_pixel == 4 {
                    let page_size = (fb_w * fb_h * 4) as usize;
                    let mmap_len = fb_mmap.len();
                    
                    // Hardware page flip targeting logic
                    let y_offset = if double_buffered && mmap_len >= page_size * 2 {
                        if active_page == 0 { fb_h } else { 0 }
                    } else {
                        0
                    };
                    
                    let target_offset = (y_offset * fb_w * 4) as usize;
                    let draw_slice = if target_offset + page_size <= mmap_len {
                        &mut fb_mmap[target_offset..target_offset + page_size]
                    } else {
                        &mut fb_mmap[0..page_size]
                    };

                    render_fn(&mut runtime, &mut local_buffer, w, h);
                    
                    // Copy from local_buffer to physical framebuffer (draw_slice) with rotation!
                    let draw_slice_u32 = unsafe {
                        std::slice::from_raw_parts_mut(
                            draw_slice.as_mut_ptr() as *mut u32,
                            draw_slice.len() / 4,
                        )
                    };
                    let rotation = if rotate { 90 } else { 0 };
                    blit_rotated(&local_buffer, draw_slice_u32, w, h, fb_w, fb_h, rotation);
                    
                    // Flip display registers
                    if double_buffered && mmap_len >= page_size * 2 {
                        if let Err(e) = fb.set_offset(0, y_offset) {
                            log::warn!("Failed to flip page: {:?}", e);
                        } else {
                            active_page = if active_page == 0 { 1 } else { 0 };
                        }
                    } else {
                        // Force flush for single-buffered display
                        let _ = fb.set_offset(0, 0);
                    }
                } else if bytes_per_pixel == 2 {
                    // 16-bit support
                    render_fn(&mut runtime, &mut local_buffer, w, h);
                    
                    // Blit 16-bit fallback directly
                    let dest_ptr = fb_mmap.as_mut_ptr();
                    for y in 0..h {
                        for x in 0..w {
                            let src_idx = (y * w + x) as usize;
                            let dest_x = if rotate { fb_w - 1 - y } else { x };
                            let dest_y = if rotate { x } else { y };
                            
                            unsafe {
                                let pixel = local_buffer[src_idx];
                                let r = ((pixel >> 16) & 0xFF) as u16;
                                let g = ((pixel >> 8) & 0xFF) as u16;
                                let b = (pixel & 0xFF) as u16;
                                let rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                                let fb_idx = (dest_y * fb_w + dest_x) as usize * 2;
                                let d = dest_ptr.add(fb_idx) as *mut u16;
                                d.write_unaligned(rgb565);
                            }
                        }
                    }
                    
                    let _ = fb.set_offset(0, 0);
                }
            }
            
            // Frame limiting and dynamic sleeping
            if !dirty {
                let elapsed = frame_start.elapsed();
                let is_idle = tick_res.next_tick_in > std::time::Duration::from_secs(3600);
                if is_idle {
                    if let Ok(ev) = rx_input.recv() {
                        buffered_event = Some(ev);
                    }
                } else if let Some(sleep_dur) = tick_res.next_tick_in.checked_sub(elapsed) {
                    if !sleep_dur.is_zero() {
                        if let Ok(ev) = rx_input.recv_timeout(sleep_dur) {
                            buffered_event = Some(ev);
                        }
                    }
                }
            }
        }
    }
}

/// Input touch screen bounds calibration values.
#[derive(Debug, Clone)]
pub struct TouchCalibration {
    /// Minimum X coordinate bound.
    pub x_min: f32,
    /// Maximum X coordinate bound.
    pub x_max: f32,
    /// Minimum Y coordinate bound.
    pub y_min: f32,
    /// Maximum Y coordinate bound.
    pub y_max: f32,
}

fn spawn_input_thread() -> (Receiver<evdev::InputEvent>, Option<TouchCalibration>) {
    let mut calibration = None;
    let mut open_devices = Vec::new();

    for id in 0..32 {
        let path = format!("/dev/input/event{}", id);
        if let Ok(dev) = Device::open(&path) {
            println!("Opened input device: {} ({})", dev.name().unwrap_or("?"), path);
            
            if calibration.is_none() {
                let axes = dev.supported_absolute_axes().unwrap_or_default();
                if axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X) || axes.contains(AbsoluteAxisType::ABS_X) {
                    if let Ok(abs_state) = dev.get_abs_state() {
                        let x_info = &abs_state[AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize];
                        let y_info = &abs_state[AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize];
                        let (mut xm, mut xM) = (x_info.minimum as f32, x_info.maximum as f32);
                        let (mut ym, mut yM) = (y_info.minimum as f32, y_info.maximum as f32);
                        
                        if xM - xm <= 0.0 {
                            let x_info_fallback = &abs_state[AbsoluteAxisType::ABS_X.0 as usize];
                            xm = x_info_fallback.minimum as f32;
                            xM = x_info_fallback.maximum as f32;
                        }
                        if yM - ym <= 0.0 {
                            let y_info_fallback = &abs_state[AbsoluteAxisType::ABS_Y.0 as usize];
                            ym = y_info_fallback.minimum as f32;
                            yM = y_info_fallback.maximum as f32;
                        }
                        
                        if xM - xm > 0.0 && yM - ym > 0.0 {
                            calibration = Some(TouchCalibration {
                                x_min: xm,
                                x_max: xM,
                                y_min: ym,
                                y_max: yM,
                            });
                            println!("Touch screen calibration: X=[{}, {}], Y=[{}, {}]", xm, xM, ym, yM);
                        }
                    }
                }
            }
            open_devices.push(dev);
        }
    }
    
    let (tx, rx) = channel();
    for mut dev in open_devices {
        let tx = tx.clone();
        thread::spawn(move || {
            loop {
                match dev.fetch_events() {
                    Ok(events) => {
                        for ev in events {
                            let _ = tx.send(ev);
                        }
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(std::time::Duration::from_millis(16));
                    },
                    Err(_) => {
                        thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            }
        });
    }
    (rx, calibration)
}

fn blit_rotated(
    local_buffer: &[u32],
    draw_slice: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    disp_w: u32,
    disp_h: u32,
    rotation: u32,
) {
    let lw = logical_w as usize;
    let lh = logical_h as usize;
    let dw = disp_w as usize;
    let dh = disp_h as usize;

    const BLOCK: usize = 32;
    let mut block_buf = [0u32; BLOCK * BLOCK];

    match rotation {
        90 => {
            for ty in (0..dh).step_by(BLOCK) {
                let ty_end = (ty + BLOCK).min(dh);
                let bh = ty_end - ty;
                for tx in (0..dw).step_by(BLOCK) {
                    let tx_end = (tx + BLOCK).min(dw);
                    let bw = tx_end - tx;

                    for py in 0..bh {
                        let y_p = ty + py;
                        let x_l = y_p;
                        let block_row = py * BLOCK;
                        for px in 0..bw {
                            let x_p = tx + px;
                            let y_l = (lh - 1) - x_p;
                            block_buf[block_row + px] = local_buffer[y_l * lw + x_l];
                        }
                    }

                    for py in 0..bh {
                        let dst_offset = (ty + py) * dw + tx;
                        let src_offset = py * BLOCK;
                        draw_slice[dst_offset..dst_offset + bw].copy_from_slice(&block_buf[src_offset..src_offset + bw]);
                    }
                }
            }
        }
        180 => {
            for py in 0..dh {
                let dst_offset = py * dw;
                let src_y = (lh - 1 - py) * lw;
                for px in 0..dw {
                    let src_x = lw - 1 - px;
                    draw_slice[dst_offset + px] = local_buffer[src_y + src_x];
                }
            }
        }
        270 => {
            for ty in (0..dh).step_by(BLOCK) {
                let ty_end = (ty + BLOCK).min(dh);
                let bh = ty_end - ty;
                for tx in (0..dw).step_by(BLOCK) {
                    let tx_end = (tx + BLOCK).min(dw);
                    let bw = tx_end - tx;

                    for py in 0..bh {
                        let y_p = ty + py;
                        let x_l = (lw - 1) - y_p;
                        let block_row = py * BLOCK;
                        for px in 0..bw {
                            let x_p = tx + px;
                            let y_l = x_p;
                            block_buf[block_row + px] = local_buffer[y_l * lw + x_l];
                        }
                    }

                    for py in 0..bh {
                        let dst_offset = (ty + py) * dw + tx;
                        let src_offset = py * BLOCK;
                        draw_slice[dst_offset..dst_offset + bw].copy_from_slice(&block_buf[src_offset..src_offset + bw]);
                    }
                }
            }
        }
        _ => {
            draw_slice.copy_from_slice(local_buffer);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evdev_key_debug_format() {
        assert_eq!(format!("{:?}", evdev::Key::KEY_UP), "KEY_UP");
        assert_eq!(format!("{:?}", evdev::Key::KEY_DOWN), "KEY_DOWN");
        assert_eq!(format!("{:?}", evdev::Key::KEY_PLAYPAUSE), "KEY_PLAYPAUSE");
    }
}
