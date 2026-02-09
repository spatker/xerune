use askama::Template;
use taffy::prelude::*;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Import from the library and renderer
use rmtui::{Ui, TextStyle};
use skia_renderer::{TinySkiaRenderer, TinySkiaMeasurer};

#[derive(Template)]
#[template(path = "showcase.html")]
struct ShowcaseTemplate;

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new()
        .with_title("RMTUI Showcase")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 800.0))
        .build(&event_loop)
        .unwrap());

    let context = softbuffer::Context::new(&window).unwrap();
    let mut surface = softbuffer::Surface::new(&context, &window).unwrap();

    // Load fonts
    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular];

    // Create UI from template
    let template = ShowcaseTemplate;
    let html = template.render().unwrap();
    let measurer = TinySkiaMeasurer { fonts: &fonts };
    let mut ui = Ui::new(&html, &measurer, TextStyle::default()).unwrap();
    
    // Initial compute
    ui.compute_layout(Size::MAX_CONTENT).unwrap();

    event_loop.run(|event, target| {
        target.set_control_flow(ControlFlow::Wait);

        match event {
            Event::WindowEvent { window_id, event: WindowEvent::RedrawRequested } if window_id == window.id() => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };
                
                 if width == 0 || height == 0 { return; }

                surface.resize(
                    NonZeroU32::new(width).unwrap(),
                    NonZeroU32::new(height).unwrap(),
                ).unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                
                // Re-compute layout with window size constraint
                ui.compute_layout(Size {
                    width: length(width as f32),
                    height: length(height as f32),
                }).unwrap();

                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::WHITE);

                let mut renderer = TinySkiaRenderer { pixmap: &mut pixmap, fonts: &fonts };
                ui.render(&mut renderer);

                let data = pixmap.data();
                for (i, chunk) in data.chunks_exact(4).enumerate() {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    buffer[i] = (r << 16) | (g << 8) | b;
                }
                
                buffer.present().unwrap();
            },
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested } if window_id == window.id() => {
                 target.exit();
            },
            // No interactivity handling needed for this static showcase
            _ => {}
        }
    }).unwrap();
}
