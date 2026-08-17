use std::sync::mpsc::{channel, Receiver};
use std::thread;
use evdev::{Device, AbsoluteAxisType, InputEventKind, Key};

use crate::{Model, InputEvent, Runtime, TextMeasurer};
use super::{InputSource, SurfaceInfo};
use crate::backend::BackendError;
use crate::backend::render_utils::map_touch_to_logical;

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

/// Evdev hardware input source for Linux `/dev/input/event*` devices.
pub struct EvdevInputSource {
    rx_input: Receiver<evdev::InputEvent>,
    calibration: Option<TouchCalibration>,
    buffered_event: Option<evdev::InputEvent>,
    touch_x: f32,
    touch_y: f32,
    mouse_x: f32,
    mouse_y: f32,
    touch_down: bool,
}

impl EvdevInputSource {
    /// Open input devices and spawn the evdev event fetch threads.
    pub fn new() -> Self {
        let (rx_input, calibration) = spawn_input_thread();
        Self {
            rx_input,
            calibration,
            buffered_event: None,
            touch_x: 0.0,
            touch_y: 0.0,
            mouse_x: 0.0,
            mouse_y: 0.0,
            touch_down: false,
        }
    }
}

impl InputSource for EvdevInputSource {
    fn dispatch_events<M, TM>(
        &mut self,
        runtime: &mut Runtime<M, TM>,
        surface: &SurfaceInfo,
    ) -> Result<bool, BackendError>
    where
        M: Model + crate::ui::TemplateLayout,
        TM: TextMeasurer,
    {
        let mut dirty = false;
        let disp_w = surface.disp_w as f32;
        let disp_h = surface.disp_h as f32;
        let rotation = surface.rotation;

        let mut touch_pos_changed = false;

        while let Some(ev) = self.buffered_event.take().or_else(|| self.rx_input.try_recv().ok()) {
            match ev.kind() {
                InputEventKind::AbsAxis(AbsoluteAxisType::ABS_X) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_X) => {
                    let raw_val = ev.value() as f32;
                    if let Some(ref cal) = self.calibration {
                        self.touch_x = ((raw_val - cal.x_min) / (cal.x_max - cal.x_min) * disp_w).clamp(0.0, disp_w - 1.0);
                    } else {
                        self.touch_x = raw_val;
                    }
                    touch_pos_changed = true;
                },
                InputEventKind::AbsAxis(AbsoluteAxisType::ABS_Y) | InputEventKind::AbsAxis(AbsoluteAxisType::ABS_MT_POSITION_Y) => {
                    let raw_val = ev.value() as f32;
                    if let Some(ref cal) = self.calibration {
                        self.touch_y = ((raw_val - cal.y_min) / (cal.y_max - cal.y_min) * disp_h).clamp(0.0, disp_h - 1.0);
                    } else {
                        self.touch_y = raw_val;
                    }
                    touch_pos_changed = true;
                },
                InputEventKind::Key(Key::BTN_TOUCH) => {
                    if touch_pos_changed {
                        let (mx, my) = map_touch_to_logical(self.touch_x, self.touch_y, disp_w, disp_h, rotation);
                        self.mouse_x = mx;
                        self.mouse_y = my;
                        if self.touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: self.mouse_x, y: self.mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: self.mouse_x, y: self.mouse_y });
                        }
                        touch_pos_changed = false;
                    }

                    if ev.value() == 1 {
                        self.touch_down = true;
                        dirty |= runtime.handle_event(InputEvent::TouchStart { id: 0, x: self.mouse_x, y: self.mouse_y });
                    } else {
                        self.touch_down = false;
                        dirty |= runtime.handle_event(InputEvent::TouchEnd { id: 0, x: self.mouse_x, y: self.mouse_y });
                    }
                },
                InputEventKind::Key(Key::BTN_LEFT) => {
                    if touch_pos_changed {
                        let (mx, my) = map_touch_to_logical(self.touch_x, self.touch_y, disp_w, disp_h, rotation);
                        self.mouse_x = mx;
                        self.mouse_y = my;
                        if self.touch_down {
                            dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: self.mouse_x, y: self.mouse_y });
                        } else {
                            dirty |= runtime.handle_event(InputEvent::Hover { x: self.mouse_x, y: self.mouse_y });
                        }
                        touch_pos_changed = false;
                    }

                    if ev.value() == 1 {
                        dirty |= runtime.handle_event(InputEvent::Click { x: self.mouse_x, y: self.mouse_y });
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

                    log::debug!("[XERUNE INPUT] Key: {:?}, name: {}, val: {}", key, key_name, ev.value());
                    if ev.value() == 1 || ev.value() == 2 {
                        dirty |= runtime.handle_event(InputEvent::KeyDown(key_name));
                    } else if ev.value() == 0 {
                        dirty |= runtime.handle_event(InputEvent::KeyUp(key_name));
                    }
                },
                _ => {}
            }
        }

        if touch_pos_changed {
            let (mx, my) = map_touch_to_logical(self.touch_x, self.touch_y, disp_w, disp_h, rotation);
            self.mouse_x = mx;
            self.mouse_y = my;
            if self.touch_down {
                dirty |= runtime.handle_event(InputEvent::TouchMove { id: 0, x: self.mouse_x, y: self.mouse_y });
            } else {
                dirty |= runtime.handle_event(InputEvent::Hover { x: self.mouse_x, y: self.mouse_y });
            }
        }

        Ok(dirty)
    }

    fn wait_for_event(&mut self, timeout: Option<std::time::Duration>) {
        if let Some(sleep_dur) = timeout {
            if let Ok(ev) = self.rx_input.recv_timeout(sleep_dur) {
                self.buffered_event = Some(ev);
            }
        } else {
            if let Ok(ev) = self.rx_input.recv() {
                self.buffered_event = Some(ev);
            }
        }
    }
}

fn spawn_input_thread() -> (Receiver<evdev::InputEvent>, Option<TouchCalibration>) {
    let mut calibration = None;
    let mut open_devices = Vec::new();

    for id in 0..32 {
        let path = format!("/dev/input/event{}", id);
        if let Ok(dev) = Device::open(&path) {
            log::info!("Opened input device: {} ({})", dev.name().unwrap_or("?"), path);
            
            if calibration.is_none() {
                let axes = dev.supported_absolute_axes().unwrap_or_default();
                if axes.contains(AbsoluteAxisType::ABS_MT_POSITION_X) || axes.contains(AbsoluteAxisType::ABS_X) {
                    if let Ok(abs_state) = dev.get_abs_state() {
                        let x_info = &abs_state[AbsoluteAxisType::ABS_MT_POSITION_X.0 as usize];
                        let y_info = &abs_state[AbsoluteAxisType::ABS_MT_POSITION_Y.0 as usize];
                        let (mut xm, mut x_m) = (x_info.minimum as f32, x_info.maximum as f32);
                        let (mut ym, mut y_m) = (y_info.minimum as f32, y_info.maximum as f32);
                        
                        if x_m - xm <= 0.0 {
                            let x_info_fallback = &abs_state[AbsoluteAxisType::ABS_X.0 as usize];
                            xm = x_info_fallback.minimum as f32;
                            x_m = x_info_fallback.maximum as f32;
                        }
                        if y_m - ym <= 0.0 {
                            let y_info_fallback = &abs_state[AbsoluteAxisType::ABS_Y.0 as usize];
                            ym = y_info_fallback.minimum as f32;
                            y_m = y_info_fallback.maximum as f32;
                        }
                        
                        if x_m - xm > 0.0 && y_m - ym > 0.0 {
                            calibration = Some(TouchCalibration {
                                x_min: xm,
                                x_max: x_m,
                                y_min: ym,
                                y_max: y_m,
                            });
                            log::info!("Touch screen calibration: X=[{}, {}], Y=[{}, {}]", xm, x_m, ym, y_m);
                        }
                    }
                }
            }
            open_devices.push(dev);
        }
    }
    
    let (tx, rx) = channel();
    if !open_devices.is_empty() {
        use std::os::fd::AsRawFd;
        thread::spawn(move || {
            let mut pollfds: Vec<libc::pollfd> = open_devices
                .iter()
                .map(|dev| libc::pollfd {
                    fd: dev.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                })
                .collect();

            loop {
                for pfd in &mut pollfds {
                    pfd.revents = 0;
                }
                let ret = unsafe { libc::poll(pollfds.as_mut_ptr(), pollfds.len() as libc::nfds_t, -1) };
                if ret < 0 {
                    let err = std::io::Error::last_os_error();
                    if err.kind() == std::io::ErrorKind::Interrupted {
                        continue;
                    }
                    log::error!("[XERUNE EVDEV] Poll error in input thread: {}", err);
                    thread::sleep(std::time::Duration::from_millis(100));
                    continue;
                }
                if ret > 0 {
                    for (idx, pfd) in pollfds.iter().enumerate() {
                        if pfd.revents & (libc::POLLIN | libc::POLLERR | libc::POLLHUP) != 0 {
                            match open_devices[idx].fetch_events() {
                                Ok(events) => {
                                    for ev in events {
                                        if tx.send(ev).is_err() {
                                            return;
                                        }
                                    }
                                }
                                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                                Err(e) => {
                                    log::warn!("[XERUNE EVDEV] Error fetching events from device: {:?}", e);
                                }
                            }
                        }
                    }
                }
            }
        });
    }
    (rx, calibration)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evdev_key_debug_format() {
        assert_eq!(format!("{:?}", Key::KEY_UP), "KEY_UP");
        assert_eq!(format!("{:?}", Key::KEY_DOWN), "KEY_DOWN");
        assert_eq!(format!("{:?}", Key::KEY_PLAYPAUSE), "KEY_PLAYPAUSE");
    }
}
