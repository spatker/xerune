use askama::Template;
use taffy::prelude::*;
use taffy::Rect as TaffyRect;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom as rcdom;
use rcdom::{Handle, NodeData, RcDom};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint, Color, Paint, Rect};
use std::collections::HashMap;

#[derive(Template)]
#[template(path = "todo_list.html")]
struct TodoList<'a> {
    items: Vec<TodoItem<'a>>,
}

struct TodoItem<'a> {
    title: &'a str,
    completed: bool,
}

enum RenderData {
    Container,
    Text(String),
}

fn walk(
    taffy: &mut TaffyTree,
    handle: &Handle,
    fonts: &[Font],
    render_data: &mut HashMap<NodeId, RenderData>,
) -> Option<NodeId> {
    let mut children = Vec::new();
    for child in handle.children.borrow().iter() {
        if let Some(id) = walk(taffy, child, fonts, render_data) {
            children.push(id);
        }
    }

    // TODO parse style from attributes
    let style = Style::default();
    // let style = Style {
    //     padding: TaffyRect {
    //         left: length(8.0),
    //         right: length(8.0),
    //         top: length(8.0),
    //         bottom: length(8.0),
    //     },
    //     display: Display::Flex,
    //     flex_direction: FlexDirection::Row,
    //     ..Default::default()
    // };

    match handle.data {
        NodeData::Document => {
            let id = taffy.new_with_children(style, &children).ok()?;
            render_data.insert(id, RenderData::Container);
            Some(id)
        },

        NodeData::Element { .. } => {
            let id = taffy.new_with_children(style, &children).ok()?;
            render_data.insert(id, RenderData::Container);
            Some(id)
        },

        NodeData::Text { ref contents } => {
            let text = contents.borrow();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                // Use PositiveYDown to match screen coordinates (Y goes down)
                // Note: fontdue's CoordinateSystem::PositiveYDown treats the Y axis as increasing downwards.
                let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                layout.reset(&fontdue::layout::LayoutSettings {
                    ..fontdue::layout::LayoutSettings::default()
                });
                layout.append(fonts, &fontdue::layout::TextStyle::new(trimmed, 20.0, 0));

                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;

                for glyph in layout.glyphs() {
                    let gx = glyph.x;
                    let gy = glyph.y;
                    let gw = glyph.width as f32;
                    let gh = glyph.height as f32;

                    if gx < min_x { min_x = gx; }
                    if gy < min_y { min_y = gy; }
                    if gx + gw > max_x { max_x = gx + gw; }
                    if gy + gh > max_y { max_y = gy + gh; }
                }

                let width = if max_x > min_x { max_x - min_x } else { 20.0 }; // Default width if empty?
                let height = if max_y > min_y { max_y - min_y } else { 20.0 };

                let style = Style {
                    size: Size {
                        width: length(width),
                        height: length(height),
                    },
                    ..Style::default()
                };

                let id = taffy.new_leaf(style).ok()?;
                render_data.insert(id, RenderData::Text(trimmed.to_string()));
                Some(id)
            }
        }

        _ => None,
    }
}

fn render_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    pixmap: &mut Pixmap,
    offset_x: f32,
    offset_y: f32,
    fonts: &[Font],
) {
    let layout = taffy.layout(root).unwrap();
    let x = offset_x + layout.location.x;
    let y = offset_y + layout.location.y;

    if let Some(RenderData::Text(content)) = render_data.get(&root) {
        // TODO: Use the style for the color
        let mut paint = Paint::default();
        paint.set_color_rgba8(220, 140, 75, 180);
        paint.anti_alias = false;

        // pixmap.fill_rect(
        //     Rect::from_xywh(x as f32, y as f32, layout.size.width as f32, layout.size.height as f32).unwrap(),
        //     &paint,
        //     Transform::identity(),
        //     None,
        // );

        let mut text_layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        text_layout.reset(&fontdue::layout::LayoutSettings {
            ..fontdue::layout::LayoutSettings::default()
        });
        text_layout.append(fonts, &fontdue::layout::TextStyle::new(content, 20.0, 0));

        for glyph in text_layout.glyphs() {
            let (metrics, bitmap) = fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
            
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }

            let mut glyph_pixmap = Pixmap::new(metrics.width as u32, metrics.height as u32).unwrap();
            let data = glyph_pixmap.data_mut();
            
            for (i, alpha) in bitmap.iter().enumerate() {
                // Black text: R=0, G=0, B=0
                // Premultiplied Alpha: A=alpha, R=0*A, G=0*A, B=0*A
                // Since R,G,B are 0, premultiplication is trivial (0).
                data[i*4 + 0] = 0;
                data[i*4 + 1] = 0;
                data[i*4 + 2] = 0;
                data[i*4 + 3] = *alpha;
            }

            let gx = x + glyph.x;
            let gy = y + glyph.y;

            pixmap.draw_pixmap(
                gx as i32,
                gy as i32,
                glyph_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    if let Ok(children) = taffy.children(root) {
        for child in children {
            render_recursive(taffy, child, render_data, pixmap, x, y, fonts);
        }
    }
}

fn main() {
    let todo_list = TodoList {
        items: vec![
            TodoItem {
                title: "Buy milk",
                completed: false,
            },
            TodoItem {
                title: "Buy eggs",
                completed: true,
            },
        ],
    };
    let html = todo_list.render().unwrap();
    println!("{}", html); // Debug print
    
    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let fonts = &[roboto_regular];

    let mut taffy = TaffyTree::new();
    let mut render_data = HashMap::new();
    
    let root = walk(&mut taffy, &dom.document, fonts, &mut render_data).unwrap();
    
    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();
    
    // Create a pixmap for rendering (e.g., 800x600)
    let mut pixmap = Pixmap::new(800, 600).unwrap();
    pixmap.fill(Color::WHITE);

    render_recursive(&taffy, root, &render_data, &mut pixmap, 0.0, 0.0, fonts);

    pixmap.save_png("image.png").unwrap();
    println!("Rendered image.png");
}