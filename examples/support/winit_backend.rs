use winit::event::{Event, WindowEvent, ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use std::rc::Rc;
use std::time::Instant;
use std::num::NonZeroU32;
use tiny_skia::{Pixmap, Color};
use xerune::{Model, InputEvent, Runtime, TextMeasurer};
use skia_renderer::TinySkiaRenderer;
use fontdue::Font;

pub fn run_app<M: Model + 'static, TM: TextMeasurer + 'static>(
    title: &str,
    width: u32,
    height: u32,
    mut runtime: Runtime<M, TM>,
    fonts: &'static [Font],
    tick_interval: Option<std::time::Duration>,
) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
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
    
    let mut last_render_time: Option<f32> = None;
    let mut next_tick = Instant::now();

    event_loop.run(move |event, target| {
         // handle control flow based on tick_interval
         match tick_interval {
            Some(interval) if interval.is_zero() => {
                 target.set_control_flow(ControlFlow::Poll);
            }
            Some(_interval) => {
                if target.control_flow() != ControlFlow::WaitUntil(next_tick) {
                     target.set_control_flow(ControlFlow::WaitUntil(next_tick));
                }
            }
            None => {
                target.set_control_flow(ControlFlow::Wait);
            }
        }

        match event {
             winit::event::Event::NewEvents(winit::event::StartCause::ResumeTimeReached { .. }) => {
                 if let Some(interval) = tick_interval {
                     if !interval.is_zero() {
                         if runtime.handle_event(InputEvent::Tick { render_time_ms: last_render_time }) {
                            window_clone.request_redraw();
                         }
                         next_tick = Instant::now() + interval;
                         target.set_control_flow(ControlFlow::WaitUntil(next_tick));
                     }
                 }
             },
            Event::AboutToWait => {
                 // Only tick on AboutToWait if we are polling (interval == 0)
                 if let Some(interval) = tick_interval {
                     if interval.is_zero() {
                        if runtime.handle_event(InputEvent::Tick { render_time_ms: last_render_time }) {
                            window_clone.request_redraw();
                        }
                     }
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

                        if let Some(mut pixmap) = Pixmap::new(width, height) {
                            pixmap.fill(Color::from_rgba8(34, 34, 34, 255)); 

                            let mut renderer = TinySkiaRenderer::new(&mut pixmap, fonts);
                            let start_render = Instant::now();
                            runtime.render(&mut renderer);
                            last_render_time = Some(start_render.elapsed().as_secs_f32() * 1000.0);

                            let data = pixmap.data();
                            for (i, chunk) in data.chunks_exact(4).enumerate() {
                                let r = chunk[0] as u32;
                                let g = chunk[1] as u32;
                                let b = chunk[2] as u32;
                                // 0RGB format for softbuffer
                                buffer[i] = (r << 16) | (g << 8) | b;
                            }
                            
                            buffer.present().unwrap();
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
                    _ => {}
                }
            },
            _ => {}
        }
    })?;
    Ok(())
}
