use askama::Template;
use taffy::prelude::*;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom as rcdom;
use rcdom::{Handle, NodeData, RcDom};
use fontdue::Font;
use tiny_skia::*;

#[derive(Template)]
#[template(path = "todo_list.html")]

struct TodoList<'a> {
    items: Vec<TodoItem<'a>>,
}

struct TodoItem<'a> {
    title: &'a str,
    completed: bool,
}

fn walk(taffy: &mut TaffyTree, handle: &Handle, fonts: &[Font]) -> Option<NodeId> {
    let mut children = Vec::new();
    for child in handle.children.borrow().iter() {
        if let Some(id) = walk(taffy, child, fonts) {
            children.push(id);
        }
    }

    let style = Style::default();

    match handle.data {
        NodeData::Document => taffy.new_with_children(style, &children).ok(),

        NodeData::Element { .. } => taffy.new_with_children(style, &children).ok(),

        NodeData::Text { ref contents } => {
            let text = contents.borrow();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYUp);
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

                    if gx < min_x {
                        min_x = gx;
                    }
                    if gy < min_y {
                        min_y = gy;
                    }
                    if gx + gw > max_x {
                        max_x = gx + gw;
                    }
                    if gy + gh > max_y {
                        max_y = gy + gh;
                    }
                }

                let width = if max_x > min_x { max_x - min_x } else { 0.0 };
                let height = if max_y > min_y { max_y - min_y } else { 0.0 };

                let style = Style {
                    size: Size {
                        width: length(width),
                        height: length(height),
                    },
                    ..Style::default()
                };

                taffy.new_leaf(style).ok()
            }
        }

        _ => None,
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
    println!("{}", html);

    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();

    if !dom.errors.borrow().is_empty() {
        println!("\nParse errors:");
        for err in dom.errors.borrow().iter() {
            println!("    {err}");
        }
    }

    let font = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font, fontdue::FontSettings::default()).unwrap();
    let fonts = &[roboto_regular];

    let mut taffy = TaffyTree::new();
    let root = walk(&mut taffy, &dom.document, fonts).unwrap();
    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();
    let layout = taffy.layout(root).unwrap();
    println!("Layout: {:?}", layout);
}