use askama::Template;
use taffy::prelude::*;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use html5ever::ns;
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

fn walk(indent: usize, handle: &Handle) {
    let node = handle;
    for _ in 0..indent {
        print!(" ");
    }
    match node.data {
        NodeData::Document => println!("#Document"),

        NodeData::Doctype {
            ref name,
            ref public_id,
            ref system_id,
        } => println!("<!DOCTYPE {name} \"{public_id}\" \"{system_id}\">"),

        NodeData::Text { ref contents } => {
            println!("#text: {}", contents.borrow().escape_default())
        },

        NodeData::Comment { ref contents } => println!("<!-- {} -->", contents.escape_default()),

        NodeData::Element {
            ref name,
            ref attrs,
            ..
        } => {
            assert!(name.ns == ns!(html));
            print!("<{}", name.local);
            for attr in attrs.borrow().iter() {
                assert!(attr.name.ns == ns!());
                print!(" {}=\"{}\"", attr.name.local, attr.value);
            }
            println!(">");
        },

        NodeData::ProcessingInstruction { .. } => unreachable!(),
    }

    for child in node.children.borrow().iter() {
        walk(indent + 4, child);
    }
}

fn main() {
    let todo_list = TodoList { items: vec![TodoItem { title: "Buy milk", completed: false }, TodoItem { title: "Buy eggs", completed: true }] };
    let html = todo_list.render().unwrap();
    println!("{}", html);

    let dom = parse_document(RcDom::default(), Default::default())
        .from_utf8()
        .read_from(&mut html.as_bytes())
        .unwrap();
    walk(0, &dom.document);

    if !dom.errors.borrow().is_empty() {
        println!("\nParse errors:");
        for err in dom.errors.borrow().iter() {
            println!("    {err}");
        }
    }
}