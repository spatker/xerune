use askama::Template;
use taffy::prelude::*;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::RcDom;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent, ElementState, MouseButton};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Import from the library
use rmtui::{dom_to_taffy, render_recursive, hit_test, RenderData, Interaction, TextStyle};

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

fn build_layout(
    todo_list: &TodoList,
    fonts: &[Font],
) -> (TaffyTree, HashMap<NodeId, RenderData>, HashMap<NodeId, Interaction>, NodeId) {
    let html = todo_list.render().unwrap();
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    let mut taffy = TaffyTree::new();
    let mut render_data = HashMap::new();
    let mut interactions = HashMap::new();
    
    let root = dom_to_taffy(&mut taffy, &dom.document, fonts, &mut render_data, &mut interactions, TextStyle::default()).unwrap();
    
    (taffy, render_data, interactions, root)
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new()
        .with_title("RMTUI Todo Example")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap());

    let context = softbuffer::Context::new(&window).unwrap();
    let mut surface = softbuffer::Surface::new(&context, &window).unwrap();

    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular];

    let mut todo_list = TodoList {
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

    let (mut taffy, mut render_data, mut interactions, mut root) = build_layout(&todo_list, &fonts);
    
    // Initial compute
    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();

    let mut cursor_pos = None;

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
                taffy.compute_layout(root, Size {
                    width: length(width as f32),
                    height: length(height as f32),
                }).unwrap();

                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::WHITE);

                render_recursive(&taffy, root, &render_data, &mut pixmap, 0.0, 0.0, &fonts);

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
            Event::WindowEvent { window_id, event: WindowEvent::CursorMoved { position, .. } } if window_id == window.id() => {
                cursor_pos = Some((position.x, position.y));
            },
            Event::WindowEvent { window_id, event: WindowEvent::MouseInput { state: ElementState::Pressed, button: MouseButton::Left, .. } } if window_id == window.id() => {
                if let Some((cx, cy)) = cursor_pos {
                    if let Some(clicked_node) = hit_test(&taffy, root, cx as f32, cy as f32, 0.0, 0.0) {
                         let mut current = Some(clicked_node);
                         let mut action = None;
                         
                         while let Some(node) = current {
                             if let Some(act) = interactions.get(&node) {
                                 action = Some(act.clone());
                                 break;
                             }
                             current = taffy.parent(node);
                         }

                         if let Some(act) = action {
                             if let Some(index_str) = act.strip_prefix("toggle:") {
                                 if let Ok(index) = index_str.parse::<usize>() {
                                    if index < todo_list.items.len() {
                                        todo_list.items[index].completed = !todo_list.items[index].completed;
                                        
                                        // Rebuild layout
                                        let build_result = build_layout(&todo_list, &fonts);
                                        taffy = build_result.0;
                                        render_data = build_result.1;
                                        interactions = build_result.2;
                                        root = build_result.3;
                                        
                                        window.request_redraw();
                                    }
                                 }
                             }
                         }
                    }
                }
            },
            _ => {}
        }
    }).unwrap();
}
