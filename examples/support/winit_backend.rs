use winit::event::{Event, WindowEvent, ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::{ControlFlow};
use winit::window::WindowBuilder;
use std::rc::Rc;

use std::num::NonZeroU32;
use xerune::{Model, InputEvent, Runtime, TextMeasurer};

#[cfg(not(feature = "fast-renderer"))]
use skia_renderer::TinySkiaRenderer;
#[cfg(not(feature = "fast-renderer"))]
use tiny_skia::Color;
#[cfg(feature = "fast-renderer")]
use fast_renderer::FastRenderer;

use fontdue::Font;

pub fn run_app<M: Model + xerune::ui::TemplateLayout + 'static, TM: TextMeasurer + 'static>(
    title: &str,
    width: u32,
    height: u32,
    mut runtime: Runtime<M, TM>,
    fonts: &'static [Font],
    setup: impl FnOnce(winit::event_loop::EventLoopProxy<String>),
) -> anyhow::Result<()> {
    let event_loop = winit::event_loop::EventLoopBuilder::<String>::with_user_event().build()?;
    let proxy = event_loop.create_proxy();
    setup(proxy.clone());
    let window = Rc::new(WindowBuilder::new()
        .with_title(title)
        .with_inner_size(winit::dpi::LogicalSize::new(width as f64, height as f64))
        .build(&event_loop)?);

    let context = softbuffer::Context::new(&window).map_err(|e| anyhow::anyhow!("Context error: {}", e))?;
    let mut surface = softbuffer::Surface::new(&context, &window).map_err(|e| anyhow::anyhow!("Surface error: {}", e))?;

    runtime.set_size(width as f32, height as f32);

    let window_clone = window.clone();
    let mut mouse_x = 0.0;
    let mut mouse_y = 0.0;
    


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

    #[cfg(not(feature = "fast-renderer"))]
    let mut app_pixmap: Option<tiny_skia::Pixmap> = None;
    #[cfg(feature = "fast-renderer")]
    let mut app_buffer: Option<Vec<u32>> = None;

    event_loop.run(move |event, target| {
        match event {
            Event::AboutToWait => {
                let res = runtime.tick();
                if res.needs_redraw {
                    window_clone.request_redraw();
                }
                let next_trigger = std::time::Instant::now() + res.next_tick_in;
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

                        #[cfg(not(feature = "fast-renderer"))]
                        {
                            if app_pixmap.is_none() || app_pixmap.as_ref().unwrap().width() != width || app_pixmap.as_ref().unwrap().height() != height {
                                app_pixmap = tiny_skia::Pixmap::new(width, height);
                                if let Some(p) = app_pixmap.as_mut() {
                                    p.fill(Color::from_rgba8(34, 34, 34, 255));
                                }
                            }

                            if let Some(pixmap) = app_pixmap.as_mut() {
                                let mut renderer = TinySkiaRenderer::new(pixmap.as_mut(), fonts, &mut image_cache, &mut gradient_cache, &mut glyph_cache);
                                runtime.render(&mut renderer);

                                let data = pixmap.data();
                                for (i, chunk) in data.chunks_exact(4).enumerate() {
                                    let r = chunk[0] as u32;
                                    let g = chunk[1] as u32;
                                    let b = chunk[2] as u32;
                                    buffer[i] = (r << 16) | (g << 8) | b;
                                }
                                buffer.present().unwrap();
                            }
                        }

                        #[cfg(feature = "fast-renderer")]
                        {
                            let buffer_len = (width * height) as usize;
                            if app_buffer.is_none() || app_buffer.as_ref().unwrap().len() != buffer_len {
                                app_buffer = Some(vec![0xFF222222; buffer_len]);
                            }

                            if let Some(ref mut app_buf) = app_buffer {
                                let mut renderer = FastRenderer::new(app_buf, width, height, fonts, &mut image_cache, &mut glyph_cache);
                                runtime.render(&mut renderer);

                                buffer.copy_from_slice(app_buf);
                                buffer.present().unwrap();
                            }
                        }
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
                    WindowEvent::KeyboardInput { event: kb_event, .. } => {
                        // For winit 0.29
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
    })?;
    Ok(())
}
