use winit::event::{Event, WindowEvent, ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::{ControlFlow, EventLoopProxy};
use winit::window::WindowBuilder;
use std::rc::Rc;
use std::num::NonZeroU32;
use crate::{Model, InputEvent, Runtime, TextMeasurer};
use super::{Backend, EventProxy, BackendError, SendError};

/// The touch id the left mouse button presses as.
const MOUSE_TOUCH_ID: u64 = u64::MAX;

/// Proxy struct to dispatch user messages from winit events into the MVU event loop.
#[derive(Clone)]
pub struct WinitProxy {
    proxy: EventLoopProxy<String>,
}

impl EventProxy for WinitProxy {
    fn send_message(&self, message: String) -> Result<(), SendError> {
        self.proxy.send_event(message)
            .map_err(|e| SendError(e.to_string()))
    }
}

/// Backend implementation for winit window environments.
pub struct WinitBackend;

impl WinitBackend {
    /// Create a new WinitBackend.
    pub fn new() -> Self {
        Self
    }
}

impl Backend for WinitBackend {
    type Proxy = WinitProxy;

    fn run<M, TM, R>(
        self,
        title: &str,
        width: u32,
        height: u32,
        mut runtime: Runtime<M, TM>,
        mut renderer: R,
        setup: impl FnOnce(Self::Proxy) + 'static,
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
        let event_loop = winit::event_loop::EventLoopBuilder::<String>::with_user_event().build()
            .map_err(|e| BackendError::Init(e.to_string()))?;
        let proxy = event_loop.create_proxy();
        {
            // Background image loads wake the loop; `poll()` then applies them.
            let wake_proxy = std::sync::Mutex::new(proxy.clone());
            runtime.set_image_waker(move || {
                if let Ok(p) = wake_proxy.lock() {
                    let _ = p.send_event(crate::runtime::WAKE_MESSAGE.to_string());
                }
            });
        }
        setup(WinitProxy { proxy });
        
        let window = Rc::new(WindowBuilder::new()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(width as f64, height as f64))
            .build(&event_loop)
            .map_err(|e| BackendError::Init(e.to_string()))?);

        let context = softbuffer::Context::new(&window)
            .map_err(|e| BackendError::Init(e.to_string()))?;
        let mut surface = softbuffer::Surface::new(&context, &window)
            .map_err(|e| BackendError::Init(e.to_string()))?;

        runtime.set_size(width as f32, height as f32);

        let window_clone = window.clone();
        let mut mouse_x = 0.0;
        let mut mouse_y = 0.0;
        // The left button held: the mouse acts as a touch (drags ranges,
        // scrolls, clicks on release), like a touch screen on the board.
        let mut mouse_down = false;
        // When to poll the runtime next; `None`: only after input.
        let mut next_trigger = Some(std::time::Instant::now());
        let mut app_buffer: Vec<u32> = Vec::new();

        event_loop.run(move |event, target| {
            match event {
                Event::AboutToWait => {
                    if next_trigger.is_none_or(|at| std::time::Instant::now() >= at) {
                        let poll = runtime.poll();
                        if poll.redraw {
                            window_clone.request_redraw();
                        }
                        next_trigger = poll.wake.deadline();
                    }
                    target.set_control_flow(match next_trigger {
                        Some(at) => ControlFlow::WaitUntil(at),
                        None => ControlFlow::Wait,
                    });
                }
                Event::UserEvent(msg) => {
                    if msg == crate::runtime::WAKE_MESSAGE {
                        // Poll in the upcoming AboutToWait.
                        next_trigger = Some(std::time::Instant::now());
                    } else if runtime.handle_event(InputEvent::Message(msg)) {
                        next_trigger = Some(std::time::Instant::now());
                        window_clone.request_redraw();
                    }
                },
                Event::WindowEvent { window_id, event } if window_id == window_clone.id() => {
                    match event {
                        WindowEvent::Resized(_) => {
                            window_clone.request_redraw();
                        },
                        WindowEvent::ScaleFactorChanged { .. } => {
                            window_clone.request_redraw();
                        },
                        WindowEvent::RedrawRequested => {
                            let size = window_clone.inner_size();
                            let width = size.width;
                            let height = size.height;
                            
                            if width == 0 || height == 0 { return; }

                            if let Err(e) = surface.resize(
                                NonZeroU32::new(width).unwrap(),
                                NonZeroU32::new(height).unwrap(),
                            ) {
                                eprintln!("Resize error: {}", e);
                                return;
                            }

                            let buffer_len = (width * height) as usize;
                            if app_buffer.len() != buffer_len {
                                app_buffer.resize(buffer_len, 0);
                                app_buffer.fill(0);
                            }

                            let mut buffer = match surface.buffer_mut() {
                                Ok(b) => b,
                                Err(e) => {
                                    eprintln!("Buffer error: {}", e);
                                    return;
                                }
                            };
                            
                            runtime.set_size(width as f32, height as f32);

                            let mut target = crate::Surface::new(crate::Pixels::Xrgb8888(&mut app_buffer), width, height);
                            runtime.render(&mut renderer, &mut target);

                            buffer.copy_from_slice(&app_buffer);
                            if let Err(e) = buffer.present() {
                                eprintln!("Present error: {}", e);
                            }
                        },
                        WindowEvent::CloseRequested => {
                            target.exit();
                        },
                        WindowEvent::CursorMoved { position, .. } => {
                            mouse_x = position.x as f32;
                            mouse_y = position.y as f32;
                            let event = if mouse_down {
                                InputEvent::TouchMove { id: MOUSE_TOUCH_ID, x: mouse_x, y: mouse_y }
                            } else {
                                InputEvent::Hover { x: mouse_x, y: mouse_y }
                            };
                            if runtime.handle_event(event) {
                                next_trigger = Some(std::time::Instant::now());
                                window_clone.request_redraw();
                            }
                        },
                        WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                            let event = match state {
                                ElementState::Pressed if !mouse_down => Some(InputEvent::TouchStart { id: MOUSE_TOUCH_ID, x: mouse_x, y: mouse_y }),
                                ElementState::Released if mouse_down => Some(InputEvent::TouchEnd { id: MOUSE_TOUCH_ID, x: mouse_x, y: mouse_y }),
                                _ => None,
                            };
                            mouse_down = state == ElementState::Pressed;
                            if event.is_some_and(|event| runtime.handle_event(event)) {
                                next_trigger = Some(std::time::Instant::now());
                                window_clone.request_redraw();
                            }
                        },
                        WindowEvent::MouseWheel { delta, .. } => {
                            let (dx, dy) = match delta {
                                MouseScrollDelta::LineDelta(x, y) => (x * 20.0, y * 20.0),
                                MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                            };
                            if runtime.handle_event(InputEvent::Scroll { x: mouse_x, y: mouse_y, delta_x: dx, delta_y: dy }) {
                                next_trigger = Some(std::time::Instant::now());
                                window_clone.request_redraw();
                            }
                        },
                        WindowEvent::Touch(touch) => {
                            let id = touch.id;
                            let x = touch.location.x as f32;
                            let y = touch.location.y as f32;
                            let input_event = match touch.phase {
                                winit::event::TouchPhase::Started => InputEvent::TouchStart { id, x, y },
                                winit::event::TouchPhase::Moved => InputEvent::TouchMove { id, x, y },
                                winit::event::TouchPhase::Ended => InputEvent::TouchEnd { id, x, y },
                                winit::event::TouchPhase::Cancelled => InputEvent::TouchCancel { id, x, y },
                            };
                            if runtime.handle_event(input_event) {
                                next_trigger = Some(std::time::Instant::now());
                                window_clone.request_redraw();
                            }
                        },
                        WindowEvent::KeyboardInput { event: kb_event, .. } => {
                            let mut redraw = false;
                            if kb_event.state == ElementState::Pressed {
                                if let Some(text) = &kb_event.text {
                                    if !text.is_empty() {
                                        let text_event = InputEvent::TextInput { id: String::new(), text: text.to_string() };
                                        if runtime.handle_event(text_event) {
                                            redraw = true;
                                        }
                                    }
                                }
                            }

                            if let winit::keyboard::PhysicalKey::Code(keycode) = kb_event.physical_key {
                                let key_name = format!("{:?}", keycode);
                                let input_event = if kb_event.state == ElementState::Pressed {
                                    InputEvent::KeyDown(key_name)
                                } else {
                                    InputEvent::KeyUp(key_name)
                                };
                                if runtime.handle_event(input_event) {
                                    redraw = true;
                                }
                            }

                            if redraw {
                                next_trigger = Some(std::time::Instant::now());
                                window_clone.request_redraw();
                            }
                        },
                        _ => {}
                    }
                },
                _ => {}
            }
        }).map_err(|e| BackendError::Run(e.to_string()))
    }
}
