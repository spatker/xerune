use askama::Template;

#[derive(Template)]
#[template(path = "todo_list.html")]

struct TodoList<'a> {
    items: Vec<TodoItem<'a>>,
}

struct TodoItem<'a> {
    title: &'a str,
    completed: bool,
}

fn main() {
    let todo_list = TodoList { items: vec![TodoItem { title: "Buy milk", completed: false }, TodoItem { title: "Buy eggs", completed: true }] };
    let html = todo_list.render().unwrap();
    println!("{}", html);
    
}