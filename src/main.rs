use askama::Template;
use taffy::prelude::*;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;

use markup5ever_rcdom as rcdom;
use rcdom::{Handle, NodeData, RcDom};

#[derive(Template)]
#[template(path = "todo_list.html")]

struct TodoList<'a> {
    items: Vec<TodoItem<'a>>,
}

struct TodoItem<'a> {
    title: &'a str,
    completed: bool,
}

fn walk(taffy: &mut TaffyTree, handle: &Handle) -> Option<NodeId> {
    let mut children = Vec::new();
    for child in handle.children.borrow().iter() {
        if let Some(id) = walk(taffy, child) {
            children.push(id);
        }
    }

    let style = Style::default();

    match handle.data {
        NodeData::Document => taffy.new_with_children(style, &children).ok(),

        NodeData::Element { .. } => taffy.new_with_children(style, &children).ok(),

        NodeData::Text { ref contents } => {
            if contents.borrow().trim().is_empty() {
                None
            } else {
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

    let mut taffy = TaffyTree::new();
    let root = walk(&mut taffy, &dom.document).unwrap();
    taffy.compute_layout(root, Size::MAX_CONTENT).unwrap();
    let layout = taffy.layout(root).unwrap();
    println!("Layout: {:?}", layout);
}