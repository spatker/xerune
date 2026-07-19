use winit::event::{Event, WindowEvent, ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::{ControlFlow, EventLoopProxy};
use winit::window::WindowBuilder;
use std::rc::Rc;
use std::num::NonZeroU32;
use crate::{Model, InputEvent, Runtime, TextMeasurer};
use super::{Backend, EventProxy, BackendError, SendError};

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

pub struct WinitBackend;

impl WinitBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Backend for WinitBackend {
    type Proxy = WinitProxy;

    fn run<M, TM, F>(
        self,
        title: &str,
        width: u32,
        height: u32,
        mut runtime: Runtime<M, TM>,
        mut render_fn: F,
        setup: impl FnOnce(Self::Proxy) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) + 'static,
    {
        let event_loop = winit::event_loop::EventLoopBuilder::<String>::with_user_event().build()
            .map_err(|e| BackendError::Init(e.to_string()))?;
        let proxy = event_loop.create_proxy();
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
        let mut next_trigger = std::time::Instant::now();

        // Local buffer for drawing logic
        let mut app_buffer: Vec<u32> = Vec::new();

        event_loop.run(move |event, target| {
            match event {
                Event::AboutToWait => {
                    let now = std::time::Instant::now();
                    if now >= next_trigger {
                        let res = runtime.tick();
                        if res.needs_redraw {
                            window_clone.request_redraw();
                        }
                        next_trigger = std::time::Instant::now() + res.next_tick_in;
                    }
                    target.set_control_flow(ControlFlow::WaitUntil(next_trigger));
                }
                Event::UserEvent(msg) => {
                    if runtime.handle_event(InputEvent::Message(msg)) {
                        window_clone.request_redraw();
                    }
                },
                Event::WindowEvent { window_id, event } if window_id == window_clone.id() => {
                    match event {
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

                            let mut buffer = match surface.buffer_mut() {
                                Ok(b) => b,
                                Err(e) => {
                                    eprintln!("Buffer error: {}", e);
                                    return;
                                }
                            };
                            
                            runtime.set_size(width as f32, height as f32);

                            let buffer_len = (width * height) as usize;
                            if app_buffer.len() != buffer_len {
                                app_buffer.resize(buffer_len, 0xFF222222);
                            }

                            render_fn(&mut runtime, &mut app_buffer, width, height);

                            buffer.copy_from_slice(&app_buffer);
                            buffer.present().unwrap();
                        },
                        WindowEvent::CloseRequested => {
                            target.exit();
                        },
                        WindowEvent::CursorMoved { position, .. } => {
                            mouse_x = position.x as f32;
                            mouse_y = position.y as f32;
                            if runtime.handle_event(InputEvent::Hover { x: mouse_x, y: mouse_y }) {
                                window_clone.request_redraw();
                            }
                        },
                        WindowEvent::MouseInput { state, button: MouseButton::Left, .. } => {
                            if state == ElementState::Pressed {
                                 if runtime.handle_event(InputEvent::Click { x: mouse_x, y: mouse_y }) {
                                    window_clone.request_redraw();
                                 }
                            }
                        },
                        WindowEvent::MouseWheel { delta, .. } => {
                            let (dx, dy) = match delta {
                                MouseScrollDelta::LineDelta(x, y) => (x * 20.0, y * 20.0),
                                MouseScrollDelta::PixelDelta(pos) => (pos.x as f32, pos.y as f32),
                            };
                            if runtime.handle_event(InputEvent::Scroll { x: mouse_x, y: mouse_y, delta_x: dx, delta_y: dy }) {
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
