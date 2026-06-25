use xerune::{Model, InputEvent, Runtime, TextMeasurer};
#[cfg(not(feature = "fast-renderer"))]
use skia_renderer::TinySkiaRenderer;
#[cfg(feature = "fast-renderer")]
use fast_renderer::FastRenderer;
use fontdue::Font;
use tiny_skia::Pixmap;
use std::time::Instant;

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
use {
    linuxfb::Framebuffer,
    evdev::{Device, AbsoluteAxisType, InputEventKind, Key},
    std::sync::mpsc::{channel, Receiver, Sender},
    std::thread,
};

pub fn run_app<M: Model + 'static, TM: TextMeasurer + 'static>(
    _title: &str, // Unused in FB
    _width: u32,
    _height: u32,
    mut runtime: Runtime<M, TM>,
    fonts: &'static [Font],
    setup: impl FnOnce(std::sync::mpsc::Sender<String>),
) -> anyhow::Result<()> {
    #[cfg(feature = "profile")]
    use coarse_prof::profile;
    #[cfg(not(feature = "profile"))]
    macro_rules! profile { ($($tt:tt)*) => {}; }
    
    // Attempt to open framebuffer
    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
        println!("Initializing Framebuffer Backend...");
        let mut fb = Framebuffer::new("/dev/fb0").map_err(|e| anyhow::anyhow!("Failed to open framebuffer: {:?}", e))?;
        
        let (mut fb_w, mut fb_h) = fb.get_size();
        let bytes_per_pixel = fb.get_bytes_per_pixel();
        let bpp = bytes_per_pixel * 8;
        
        let rotate = fb_w < fb_h;
        let (w, h) = if rotate { (fb_h, fb_w) } else { (fb_w, fb_h) };
        
        println!("Framebuffer: {}x{} @ {}bpp", fb_w, fb_h, bpp);
        
        let mut double_buffered = false;
        
        // Attempt to request virtual framebuffer space for true hardware page-flipping!
        if let Err(e) = fb.set_virtual_size(fb_w, fb_h * 2) {
            println!("Warning: Could not set virtual size for hardware double buffering: {:?}", e);
        } else {
             let (vw, vh) = fb.get_virtual_size();
             if vh >= fb_h * 2 {
                 println!("Hardware Double Buffering activated seamlessly (Virtual size: {}x{})", vw, vh);
                 double_buffered = true;
             }
        }
        
        let mut fb_mmap = fb.map().map_err(|e| anyhow::anyhow!("Failed to map framebuffer: {:?}", e))?;
        let rx_input = spawn_input_thread();
        
        runtime.set_size(w as f32, h as f32);
        
        let (msg_tx, msg_rx) = std::sync::mpsc::channel::<String>();
        setup(msg_tx);
        
        let _ = fb.set_offset(0, 0); // Ensure no panning is applied
        
        #[cfg(not(feature = "fast-renderer"))]
        let mut image_cache = std::collections::HashMap::new();
        #[cfg(feature = "fast-renderer")]
        let mut image_cache = std::collections::HashMap::new();

        #[cfg(not(feature = "fast-renderer"))]
        let mut gradient_cache = std::collections::HashMap::new();

        #[cfg(not(feature = "fast-renderer"))]
        let mut glyph_cache = std::collections::HashMap::new();
        #[cfg(feature = "fast-renderer")]
        let mut glyph_cache = std::collections::HashMap::new();

        let mut mouse_x = 0.0;
        let mut mouse_y = 0.0;
        let mut touch_x = 0.0;
        let mut touch_y = 0.0;
        
        let mut force_redraw = true;
        let mut prev_render_time_ms: Option<f32> = None;
        let mut active_page = 0;

        loop {
            let frame_start = Instant::now();
            let mut dirty = force_redraw;
            force_redraw = false;

            // Poll Input
            while let Ok(ev) = rx_input.try_recv() {
                match ev.kind() {
                    InputEventKind::AbsAxis(AbsoluteAxisType::ABS_X) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_X) => {
                        touch_x = ev.value() as f32;
                        if rotate { mouse_y = fb_w as f32 - 1.0 - touch_x; } else { mouse_x = touch_x; }
                        dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                    },
                    InputEventKind::AbsAxis(AbsoluteAxisType::ABS_Y) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_Y) => {
                        touch_y = ev.value() as f32;
                        if rotate { mouse_x = touch_y; } else { mouse_y = touch_y; }
                        dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                    },
                    InputEventKind::Key(Key::BTN_LEFT) | InputEventKind::Key(Key::BTN_TOUCH) => {
                        if ev.value() == 1 {
                            dirty |= runtime.handle_event(InputEvent::Click { x: mouse_x, y: mouse_y });
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
            if let Some(ms) = prev_render_time_ms {
                messages.push(format!("render_time_ms:{:.2}", ms));
            }
            if !messages.is_empty() {
                dirty |= runtime.handle_messages(messages);
            }

            // Update
            let tick_res = runtime.tick();
            dirty |= tick_res.needs_redraw;
            
            // Draw
            if dirty {
                let render_start = Instant::now();
                
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

                    #[cfg(not(feature = "fast-renderer"))]
                    {
                        if let Some(fb_pixmap) = tiny_skia::PixmapMut::from_bytes(draw_slice, fb_w, fb_h) {
                             let mut renderer = TinySkiaRenderer::new(fb_pixmap, fonts, &mut image_cache, &mut gradient_cache, &mut glyph_cache);
                             renderer.swap_rb = true; // FB is BGRA
                             if rotate {
                                 renderer.transform = tiny_skia::Transform::from_rotate(90.0).post_translate(fb_w as f32, 0.0);
                             }
                             runtime.render(&mut renderer);
                        }
                    }

                    #[cfg(feature = "fast-renderer")]
                    {
                        let ptr = draw_slice.as_mut_ptr() as *mut u32;
                        let len = draw_slice.len() / 4;
                        let raw_buf = unsafe { std::slice::from_raw_parts_mut(ptr, len) };
                        
                        let mut renderer = FastRenderer::new(raw_buf, w, h, fonts, &mut image_cache, &mut glyph_cache);
                        renderer.swap_rb = true; // FB is BGRA
                        renderer.rotate = rotate;
                        renderer.physical_width = fb_w;
                        renderer.physical_height = fb_h;
                        runtime.render(&mut renderer);
                    }
                    
                    // Flip the display registers to instantly show the newly drawn virtual offset!
                    if double_buffered && mmap_len >= page_size * 2 {
                        if let Err(e) = fb.set_offset(0, y_offset) {
                            log::warn!("Failed to flip page: {:?}", e);
                        } else {
                            active_page = if active_page == 0 { 1 } else { 0 };
                        }
                    }
                } else if bytes_per_pixel == 2 {
                    // Fallback: draw to local (RGBA) and convert to RGB565 during blit
                    render_16bit_fallback(&mut runtime, w, h, rotate, fb_w, fb_mmap.as_mut(), fonts, &mut image_cache, &mut gradient_cache, &mut glyph_cache)?;
                }
                prev_render_time_ms = Some(render_start.elapsed().as_secs_f32() * 1000.0);
            } else {
                prev_render_time_ms = None;
            }
            
            // Frame limiting and dynamic sleeping
            let elapsed = frame_start.elapsed();
            let sleep_duration = tick_res.next_tick_in.saturating_sub(elapsed);
            if !sleep_duration.is_zero() {
                std::thread::sleep(sleep_duration);
            }
        }
    }

    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        println!("Linux framebuffer backend not enabled or not on Linux. Returning error.");
        Err(anyhow::anyhow!("Linux backend disabled"))
    }
}

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
fn render_16bit_fallback<M: Model, TM: TextMeasurer>(
    runtime: &mut Runtime<M, TM>,
    w: u32,
    h: u32,
    rotate: bool,
    fb_w: u32,
    fb_mmap: &mut [u8],
    fonts: &[Font],
    image_cache: &mut std::collections::HashMap<String, Pixmap>,
    gradient_cache: &mut std::collections::HashMap<String, Pixmap>,
    glyph_cache: &mut std::collections::HashMap<(usize, u16, u32, [u8; 4]), Pixmap>,
) -> anyhow::Result<()> {
    #[cfg(feature = "profile")]
    use coarse_prof::profile;
    #[cfg(not(feature = "profile"))]
    macro_rules! profile { ($($tt:tt)*) => {}; }

    let mut pixmap = Pixmap::new(w, h).ok_or_else(|| anyhow::anyhow!("Failed to create pixmap"))?;
    let mut renderer = TinySkiaRenderer::new(pixmap.as_mut(), fonts, image_cache, gradient_cache, glyph_cache);
    let dirty_region = runtime.render(&mut renderer);
    
    profile!("blit_16bit");
    let data = pixmap.data();
    let (dx, dy, dw, dh) = if let Some(r) = dirty_region {
        let x1 = (r.x.floor() as i32).max(0).min(w as i32) as u32;
        let y1 = (r.y.floor() as i32).max(0).min(h as i32) as u32;
        let x2 = ((r.x + r.width).ceil() as i32).max(0).min(w as i32) as u32;
        let y2 = ((r.y + r.height).ceil() as i32).max(0).min(h as i32) as u32;
        (x1, y1, x2 - x1, y2 - y1)
    } else {
        (0, 0, w, h)
    };

    if dw > 0 && dh > 0 {
        let src_ptr = data.as_ptr();
        let dest_ptr = fb_mmap.as_mut_ptr();
        for y in dy..(dy + dh) {
            for x in dx..(dx + dw) {
                let src_idx = (y * w + x) as usize * 4;
                let dest_x = if rotate { fb_w - 1 - y } else { x };
                let dest_y = if rotate { x } else { y };
                
                unsafe {
                    let r = *src_ptr.add(src_idx) as u16;
                    let g = *src_ptr.add(src_idx + 1) as u16;
                    let b = *src_ptr.add(src_idx + 2) as u16;
                    let rgb565 = ((r & 0xF8) << 8) | ((g & 0xFC) << 3) | (b >> 3);
                    let fb_idx = (dest_y * fb_w + dest_x) as usize * 2;
                    let d = dest_ptr.add(fb_idx) as *mut u16;
                    d.write_unaligned(rgb565);
                }
            }
        }
    }
    Ok(())
}

#[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
fn spawn_input_thread() -> Receiver<evdev::InputEvent> {
    let mut touch_device: Option<Device> = None;
    for id in 0..10 {
        let path = format!("/dev/input/event{}", id);
        if let Ok(dev) = Device::open(&path) {
            if dev.supported_absolute_axes().map(|axes| axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X)).unwrap_or(false) {
                println!("Found touch device: {} ({})", dev.name().unwrap_or("?"), path);
                touch_device = Some(dev);
                break;
            }
        }
    }
    
    let (tx, rx) = channel();
    if let Some(mut dev) = touch_device {
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
    rx
}

