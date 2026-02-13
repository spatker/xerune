use askama::Template;
use taffy::prelude::*;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent, ElementState, MouseButton};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Import from the library
use xerune::{Runtime, Model, InputEvent};
use skia_renderer::{TinySkiaRenderer, TinySkiaMeasurer};

#[derive(Template)]
#[template(path = "todo_list.html")]
struct TodoList<'a> {
    items: Vec<TodoItem<'a>>,
}

#[derive(Clone)]
struct TodoItem<'a> {
    title: &'a str,
    completed: bool,
}

impl<'a> Model for TodoList<'a> {
    fn view(&self) -> String {
        self.render().unwrap()
    }

    fn update(&mut self, msg: &str) {
         if let Some(index_str) = msg.strip_prefix("toggle:") {
             if let Ok(index) = index_str.parse::<usize>() {
                if index < self.items.len() {
                    self.items[index].completed = !self.items[index].completed;
                }
             }
         }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new()
        .with_title("RMTUI Todo Example")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap());

    // Leak window to satisfy static lifetime for softbuffer surface in the event loop
    let leaked_window: &'static Rc<winit::window::Window> = Box::leak(Box::new(window.clone()));
    
    let context = softbuffer::Context::new(leaked_window).unwrap();
    let leaked_context = Box::leak(Box::new(context));
    
    let mut surface = softbuffer::Surface::new(leaked_context, leaked_window).unwrap();

    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular, roboto_bold];
    // Leak fonts to satisfy static lifetime for winit event loop
    let fonts: &'static [Font] = Box::leak(Box::new(fonts));

    let todo_list = TodoList {
        items: vec![
            TodoItem {
                title: "Refactor to library",
                completed: false,
            },
            TodoItem {
                title: "Initial PoC",
                completed: true,
            },
        ],
    };

    let measurer = TinySkiaMeasurer { fonts };
    let mut runtime = Runtime::new(todo_list, measurer);
    
    // Initial compute
    runtime.compute_layout(Size::MAX_CONTENT);

    let mut cursor_pos = None;

    event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent { window_id, event: WindowEvent::RedrawRequested } if window_id == leaked_window.id() => {
                let (width, height) = {
                    let size = leaked_window.inner_size();
                    (size.width, size.height)
                };
                
                 if width == 0 || height == 0 { return; }

                surface.resize(
                    NonZeroU32::new(width).unwrap(),
                    NonZeroU32::new(height).unwrap(),
                ).unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                
                // Re-compute layout with window size constraint
                runtime.set_size(width as f32, height as f32);

                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::WHITE);

                let mut renderer = TinySkiaRenderer { pixmap: &mut pixmap, fonts };
                runtime.render(&mut renderer);

                let data = pixmap.data();
                for (i, chunk) in data.chunks_exact(4).enumerate() {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    buffer[i] = (r << 16) | (g << 8) | b;
                }
                
                buffer.present().unwrap();
            },
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested } if window_id == leaked_window.id() => {
                 target.exit();
            },
            Event::WindowEvent { window_id, event: WindowEvent::CursorMoved { position, .. } } if window_id == leaked_window.id() => {
                cursor_pos = Some((position.x, position.y));
            },
            Event::WindowEvent { window_id, event: WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } } if window_id == leaked_window.id() => {
                if let Some((cx, cy)) = cursor_pos {
                    if runtime.handle_event(InputEvent::Click { x: cx as f32, y: cy as f32 }) {
                        leaked_window.request_redraw();
                    }
                }
            },
            _ => {}
        }
    }).unwrap();
}

