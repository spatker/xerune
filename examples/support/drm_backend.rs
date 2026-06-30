use xerune::{Model, InputEvent, Runtime, TextMeasurer};
#[cfg(not(feature = "fast-renderer"))]
use skia_renderer::TinySkiaRenderer;
#[cfg(feature = "fast-renderer")]
use fast_renderer::FastRenderer;
use fontdue::Font;
#[allow(unused_imports)]
use tiny_skia::Pixmap;
use std::time::Instant;
use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd};
use std::sync::mpsc::{channel, Receiver};
use std::thread;

use drm::control::Device as ControlDevice;
use drm::Device as BasicDevice;
use drm_fourcc::DrmFourcc;

pub struct Card(File);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl drm::Device for Card {}
impl drm::control::Device for Card {}

impl Card {
    pub fn open_dri_card() -> anyhow::Result<Self> {
        for i in 0..5 {
            let path = format!("/dev/dri/card{}", i);
            if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                println!("Opened DRM device: {}", path);
                return Ok(Card(file));
            }
        }
        Err(anyhow::anyhow!("No DRM card device found"))
    }
}

#[derive(Debug, Clone)]
pub struct TouchCalibration {
    pub x_min: f32,
    pub x_max: f32,
    pub y_min: f32,
    pub y_max: f32,
}

fn spawn_input_thread() -> (Receiver<evdev::InputEvent>, Option<TouchCalibration>) {
    let mut touch_device: Option<evdev::Device> = None;
    for id in 0..10 {
        let path = format!("/dev/input/event{}", id);
        if let Ok(dev) = evdev::Device::open(&path) {
            if dev.supported_absolute_axes().map(|axes| axes.contains(evdev::AbsoluteAxisType::ABS_MT_POSITION_X)).unwrap_or(false) {
                println!("Found touch device: {} ({})", dev.name().unwrap_or("?"), path);
                touch_device = Some(dev);
                break;
            }
        }
    }
    
    let mut calibration = None;
    if let Some(ref dev) = touch_device {
        if let Ok(abs_state) = dev.get_abs_state() {
            let x_info = &abs_state[evdev::AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize];
            let y_info = &abs_state[evdev::AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize];
            let (mut xm, mut xM) = (x_info.minimum as f32, x_info.maximum as f32);
            let (mut ym, mut yM) = (y_info.minimum as f32, y_info.maximum as f32);
            
            if xM - xm <= 0.0 {
                let x_info_fallback = &abs_state[evdev::AbsoluteAxisType::ABS_X.0 as usize];
                xm = x_info_fallback.minimum as f32;
                xM = x_info_fallback.maximum as f32;
            }
            if yM - ym <= 0.0 {
                let y_info_fallback = &abs_state[evdev::AbsoluteAxisType::ABS_Y.0 as usize];
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
    (rx, calibration)
}

pub fn run_app<M: Model + xerune::ui::TemplateLayout + 'static, TM: TextMeasurer + 'static>(
    _title: &str,
    _width: u32,
    _height: u32,
    mut runtime: Runtime<M, TM>,
    fonts: &'static [Font],
    setup: impl FnOnce(std::sync::mpsc::Sender<String>),
) -> anyhow::Result<()> {
    println!("Initializing DRM/KMS Backend...");
    let card = Card::open_dri_card()?;
    
    // Acquire DRM Master capability
    let _ = card.acquire_master_lock(); // Ignore failure if already master
    
    let resources = card.resource_handles()
        .map_err(|e| anyhow::anyhow!("Failed to get DRM resources: {:?}", e))?;
        
    let mut active_connector = None;
    for conn_handle in resources.connectors() {
        if let Ok(conn) = card.get_connector(*conn_handle, false) {
            if conn.state() == drm::control::connector::State::Connected {
                active_connector = Some(conn);
                break;
            }
        }
    }
    
    let connector = active_connector.ok_or_else(|| anyhow::anyhow!("No connected connector found"))?;
    let mode = connector.modes().get(0).copied()
        .ok_or_else(|| anyhow::anyhow!("No modes found on connector"))?;
        
    let (disp_w, disp_h) = mode.size();
    let (w, h) = (disp_w as u32, disp_h as u32);
    println!("DRM Display: {}x{} (mode: {})", w, h, mode.name().to_string_lossy());
    
    let encoder_handle = connector.current_encoder().unwrap_or_else(|| {
        connector.encoders().get(0).copied().expect("No encoders found")
    });
    let encoder = card.get_encoder(encoder_handle)?;
    
    let crtc_handle = encoder.crtc().unwrap_or_else(|| {
        resources.crtcs().get(0).copied().expect("No CRTCs found")
    });
    
    // Allocate two dumb buffers for double buffering!
    let fmt = DrmFourcc::Xrgb8888;
    let mut db1 = card.create_dumb_buffer((w, h), fmt, 32)
        .map_err(|e| anyhow::anyhow!("Failed to create dumb buffer 1: {:?}", e))?;
    let mut db2 = card.create_dumb_buffer((w, h), fmt, 32)
        .map_err(|e| anyhow::anyhow!("Failed to create dumb buffer 2: {:?}", e))?;
        
    let fb1 = card.add_framebuffer(&db1, 24, 32)
        .map_err(|e| anyhow::anyhow!("Failed to add framebuffer 1: {:?}", e))?;
    let fb2 = card.add_framebuffer(&db2, 24, 32)
        .map_err(|e| anyhow::anyhow!("Failed to add framebuffer 2: {:?}", e))?;
        
    let mut map1 = card.map_dumb_buffer(&mut db1)
        .map_err(|e| anyhow::anyhow!("Failed to map dumb buffer 1: {:?}", e))?;
    let mut map2 = card.map_dumb_buffer(&mut db2)
        .map_err(|e| anyhow::anyhow!("Failed to map dumb buffer 2: {:?}", e))?;
        
    // Initial modeset
    card.set_crtc(crtc_handle, Some(fb1), (0, 0), &[connector.handle()], Some(mode))
        .map_err(|e| anyhow::anyhow!("Failed to perform initial modeset: {:?}", e))?;
        
    let (rx_input, calibration) = spawn_input_thread();
    
    runtime.set_size(w as f32, h as f32);
    
    let (msg_tx, msg_rx) = std::sync::mpsc::channel::<String>();
    setup(msg_tx);
    
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
    let mut current_fb = fb1;
    
    loop {
        let frame_start = Instant::now();
        let mut dirty = force_redraw;
        force_redraw = false;

        // Poll Input
        while let Ok(ev) = rx_input.try_recv() {
            match ev.kind() {
                evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_X) | evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_MT_POSITION_X) => {
                    let raw_val = ev.value() as f32;
                    if let Some(ref cal) = calibration {
                        touch_x = ((raw_val - cal.x_min) / (cal.x_max - cal.x_min) * w as f32).clamp(0.0, w as f32 - 1.0);
                    } else {
                        touch_x = raw_val;
                    }
                    mouse_x = touch_x;
                    dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                },
                evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_Y) | evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_MT_POSITION_Y) => {
                    let raw_val = ev.value() as f32;
                    if let Some(ref cal) = calibration {
                        touch_y = ((raw_val - cal.y_min) / (cal.y_max - cal.y_min) * h as f32).clamp(0.0, h as f32 - 1.0);
                    } else {
                        touch_y = raw_val;
                    }
                    mouse_y = touch_y;
                    dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                },
                evdev::InputEventKind::Key(evdev::Key::BTN_LEFT) | evdev::InputEventKind::Key(evdev::Key::BTN_TOUCH) => {
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
            if messages.len() > 300 { break; }
        }
        if !messages.is_empty() {
            dirty |= runtime.handle_messages(messages);
        }

        // Update
        let tick_res = runtime.tick();
        dirty |= tick_res.needs_redraw;
        
        // Draw
        if dirty {
            // Determine draw target (the back buffer)
            let (target_fb, draw_slice) = if current_fb == fb1 {
                (fb2, map2.as_mut())
            } else {
                (fb1, map1.as_mut())
            };
            
            #[cfg(not(feature = "fast-renderer"))]
            {
                if let Some(fb_pixmap) = tiny_skia::PixmapMut::from_bytes(draw_slice, w, h) {
                     let mut renderer = TinySkiaRenderer::new(fb_pixmap, fonts, &mut image_cache, &mut gradient_cache, &mut glyph_cache);
                     renderer.swap_rb = true; // Xrgb8888 is BGRA in memory
                     runtime.render(&mut renderer);
                }
            }

            #[cfg(feature = "fast-renderer")]
            {
                let draw_slice_u32 = unsafe {
                    std::slice::from_raw_parts_mut(
                        draw_slice.as_mut_ptr() as *mut u32,
                        draw_slice.len() / 4,
                    )
                };
                let mut renderer = FastRenderer::new(draw_slice_u32, w, h, fonts, &mut image_cache, &mut glyph_cache);
                renderer.swap_rb = false; // Xrgb8888 matches FastRenderer default
                runtime.render(&mut renderer);
            }
            
            // Perform hardware page flip!
            loop {
                match card.page_flip(crtc_handle, target_fb, drm::control::PageFlipFlags::empty(), None) {
                    Ok(_) => {
                        current_fb = target_fb;
                        break;
                    }
                    Err(e) => {
                        // If system is busy (previous flip pending), retry after a tiny sleep
                        let err_raw = std::io::Error::from(e);
                        if err_raw.kind() == std::io::ErrorKind::WouldBlock || err_raw.raw_os_error() == Some(libc::EBUSY) {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        } else {
                            return Err(anyhow::anyhow!("Failed to page flip: {:?}", err_raw));
                        }
                    }
                }
            }
        }
        
        // Frame limiting and dynamic sleeping
        let elapsed = frame_start.elapsed();
        let mut sleep_duration = tick_res.next_tick_in.saturating_sub(elapsed);
        if dirty {
            let target_duration = std::time::Duration::from_nanos((1_000_000_000.0 / runtime.target_fps as f64) as u64);
            let min_sleep = target_duration.saturating_sub(elapsed);
            if min_sleep > sleep_duration {
                sleep_duration = min_sleep;
            }
        }
        if !sleep_duration.is_zero() {
            std::thread::sleep(sleep_duration);
        }
    }
}
