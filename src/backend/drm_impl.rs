use crate::{Model, InputEvent, Runtime, TextMeasurer};
use super::{Backend, BackendError, MpscProxy};
use std::time::Instant;
use std::fs::File;
use std::os::fd::{AsFd, BorrowedFd, AsRawFd};
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
    pub fn open_dri_card() -> Result<Self, BackendError> {
        for i in 0..5 {
            let path = format!("/dev/dri/card{}", i);
            if let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
                println!("Opened DRM device: {}", path);
                return Ok(Card(file));
            }
        }
        Err(BackendError::Init("No DRM card device found".to_string()))
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
            let axes = dev.supported_absolute_axes().unwrap_or_default();
            if axes.contains(evdev::AbsoluteAxisType::ABS_MT_POSITION_X) || axes.contains(evdev::AbsoluteAxisType::ABS_X) {
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

fn blit_rotated(
    local_buffer: &[u32],
    draw_slice: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    disp_w: u32,
    disp_h: u32,
    rotation: u32,
) {
    match rotation {
        90 => {
            for py in 0..disp_h as usize {
                let dst_offset = py * disp_w as usize;
                for px in 0..disp_w as usize {
                    let x = py;
                    let y = disp_w as usize - 1 - px;
                    let src_idx = y * logical_w as usize + x;
                    draw_slice[dst_offset + px] = local_buffer[src_idx];
                }
            }
        }
        180 => {
            for py in 0..disp_h as usize {
                let dst_offset = py * disp_w as usize;
                let src_y = logical_h as usize - 1 - py;
                let src_row_offset = src_y * logical_w as usize;
                for px in 0..disp_w as usize {
                    let src_x = logical_w as usize - 1 - px;
                    draw_slice[dst_offset + px] = local_buffer[src_row_offset + src_x];
                }
            }
        }
        270 => {
            for py in 0..disp_h as usize {
                let dst_offset = py * disp_w as usize;
                for px in 0..disp_w as usize {
                    let x = logical_w as usize - 1 - py;
                    let y = px;
                    let src_idx = y * logical_w as usize + x;
                    draw_slice[dst_offset + px] = local_buffer[src_idx];
                }
            }
        }
        _ => {
            draw_slice.copy_from_slice(local_buffer);
        }
    }
}

fn map_touch_to_logical(touch_x: f32, touch_y: f32, disp_w: f32, disp_h: f32, rotation: u32) -> (f32, f32) {
    match rotation {
        90 => (touch_y, disp_w - 1.0 - touch_x),
        180 => (disp_w - 1.0 - touch_x, disp_h - 1.0 - touch_y),
        270 => (disp_h - 1.0 - touch_y, touch_x),
        _ => (touch_x, touch_y),
    }
}

fn wait_for_page_flip(card: &Card) -> Result<(), BackendError> {
    let fd = card.as_fd().as_raw_fd();
    let mut poll_fd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let ret = unsafe { libc::poll(&mut poll_fd, 1, -1) };
        if ret < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(BackendError::Run(format!("Poll error: {}", err)));
        }
        if ret > 0 {
            break;
        }
    }
    
    let events = card.receive_events()
        .map_err(|e| BackendError::Run(format!("Failed to receive DRM events: {:?}", e)))?;
    for event in events {
        match event {
            drm::control::Event::PageFlip(_) => {
                return Ok(());
            }
            _ => {}
        }
    }
    Ok(())
}

pub struct DrmBackend;

impl DrmBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Backend for DrmBackend {
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
        println!("Initializing DRM/KMS Backend...");
        let card = Card::open_dri_card()?;
        
        // Acquire DRM Master capability
        let _ = card.acquire_master_lock(); // Ignore failure if already master
        
        let resources = card.resource_handles()
            .map_err(|e| BackendError::Init(format!("Failed to get DRM resources: {:?}", e)))?;
            
        let mut active_connector = None;
        for conn_handle in resources.connectors() {
            if let Ok(conn) = card.get_connector(*conn_handle, false) {
                if conn.state() == drm::control::connector::State::Connected {
                    active_connector = Some(conn);
                    break;
                }
            }
        }
        
        let connector = active_connector.ok_or_else(|| BackendError::Init("No connected connector found".to_string()))?;
        let mode = connector.modes().get(0).copied()
            .ok_or_else(|| BackendError::Init("No modes found on connector".to_string()))?;
            
        let (disp_w, disp_h) = mode.size();
        
        // Parse rotation option
        let rotation = std::env::var("XERUNE_ROTATION")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        let rotate = rotation == 90 || rotation == 270;
        let (w, h) = if rotate { (disp_h as u32, disp_w as u32) } else { (disp_w as u32, disp_h as u32) };
        println!("DRM Display: {}x{} (mode: {}, rotation: {}), logical size: {}x{}", disp_w, disp_h, mode.name().to_string_lossy(), rotation, w, h);
        
        let encoder_handle = connector.current_encoder().unwrap_or_else(|| {
            connector.encoders().get(0).copied().expect("No encoders found")
        });
        let encoder = card.get_encoder(encoder_handle)
            .map_err(|e| BackendError::Init(format!("Failed to get encoder: {:?}", e)))?;
        
        let crtc_handle = encoder.crtc().unwrap_or_else(|| {
            resources.crtcs().get(0).copied().expect("No CRTCs found")
        });
        
        // Allocate two dumb buffers for double buffering
        let fmt = DrmFourcc::Argb8888;
        let mut db1 = card.create_dumb_buffer((disp_w as u32, disp_h as u32), fmt, 32)
            .map_err(|e| BackendError::Init(format!("Failed to create dumb buffer 1: {:?}", e)))?;
        let mut db2 = card.create_dumb_buffer((disp_w as u32, disp_h as u32), fmt, 32)
            .map_err(|e| BackendError::Init(format!("Failed to create dumb buffer 2: {:?}", e)))?;
            
        let fb1 = card.add_framebuffer(&db1, 32, 32)
            .map_err(|e| BackendError::Init(format!("Failed to add framebuffer 1: {:?}", e)))?;
        let fb2 = card.add_framebuffer(&db2, 32, 32)
            .map_err(|e| BackendError::Init(format!("Failed to add framebuffer 2: {:?}", e)))?;
            
        let mut map1 = card.map_dumb_buffer(&mut db1)
            .map_err(|e| BackendError::Init(format!("Failed to map dumb buffer 1: {:?}", e)))?;
        let mut map2 = card.map_dumb_buffer(&mut db2)
            .map_err(|e| BackendError::Init(format!("Failed to map dumb buffer 2: {:?}", e)))?;
            
        // Initial modeset
        card.set_crtc(crtc_handle, Some(fb1), (0, 0), &[connector.handle()], Some(mode))
            .map_err(|e| BackendError::Init(format!("Failed to perform initial modeset: {:?}", e)))?;
            
        let (rx_input, calibration) = spawn_input_thread();
        
        runtime.set_size(w as f32, h as f32);
        
        let (msg_tx, msg_rx) = channel::<String>();
        setup(MpscProxy { sender: msg_tx });
        
        let mut mouse_x = 0.0;
        let mut mouse_y = 0.0;
        let mut touch_x = 0.0;
        let mut touch_y = 0.0;
        let mut touch_down = false;
        
        let mut force_redraw = true;
        let mut current_fb = fb1;
        
        let mut local_buffer = vec![0u32; (w * h) as usize];
        
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
                            touch_x = ((raw_val - cal.x_min) / (cal.x_max - cal.x_min) * disp_w as f32).clamp(0.0, disp_w as f32 - 1.0);
                        } else {
                            touch_x = raw_val;
                        }
                        let (mx, my) = map_touch_to_logical(touch_x, touch_y, disp_w as f32, disp_h as f32, rotation);
                        mouse_x = mx;
                        mouse_y = my;
                        
                        if touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                        }
                    },
                    evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_Y) | evdev::InputEventKind::AbsAxis(evdev::AbsoluteAxisType::ABS_MT_POSITION_Y) => {
                        let raw_val = ev.value() as f32;
                        if let Some(ref cal) = calibration {
                            touch_y = ((raw_val - cal.y_min) / (cal.y_max - cal.y_min) * disp_h as f32).clamp(0.0, disp_h as f32 - 1.0);
                        } else {
                            touch_y = raw_val;
                        }
                        let (mx, my) = map_touch_to_logical(touch_x, touch_y, disp_w as f32, disp_h as f32, rotation);
                        mouse_x = mx;
                        mouse_y = my;
                        
                        if touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y });
                        }
                    },
                    evdev::InputEventKind::Key(evdev::Key::BTN_TOUCH) => {
                        if ev.value() == 1 {
                            touch_down = true;
                            dirty |= runtime.handle_event(InputEvent::TouchStart { id: 0, x: mouse_x, y: mouse_y });
                        } else {
                            touch_down = false;
                            dirty |= runtime.handle_event(InputEvent::TouchEnd { id: 0, x: mouse_x, y: mouse_y });
                        }
                    },
                    evdev::InputEventKind::Key(evdev::Key::BTN_LEFT) => {
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
                
                local_buffer.fill(0xFF222222);
                render_fn(&mut runtime, &mut local_buffer, w, h);
                
                // Blit from local_buffer to physical dumb buffer (draw_slice) with rotation!
                let draw_slice_u32 = unsafe {
                    std::slice::from_raw_parts_mut(
                        draw_slice.as_mut_ptr() as *mut u32,
                        draw_slice.len() / 4,
                    )
                };
                blit_rotated(&local_buffer, draw_slice_u32, w, h, disp_w as u32, disp_h as u32, rotation);
                
                // Perform hardware page flip
                loop {
                    match card.page_flip(crtc_handle, target_fb, drm::control::PageFlipFlags::EVENT, None) {
                        Ok(_) => {
                            wait_for_page_flip(&card)?;
                            current_fb = target_fb;
                            break;
                        }
                        Err(e) => {
                            let err_raw = std::io::Error::from(e);
                            if err_raw.kind() == std::io::ErrorKind::WouldBlock || err_raw.raw_os_error() == Some(libc::EBUSY) {
                                thread::sleep(std::time::Duration::from_millis(1));
                            } else {
                                return Err(BackendError::Run(format!("Failed to page flip: {:?}", err_raw)));
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
                thread::sleep(sleep_duration);
            }
        }
    }
}
