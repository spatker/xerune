use askama::Template;
use fontdue::Font;
use xerune::{Runtime, Model};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

#[derive(Template)]
#[template(path = "todo_list.html")]
struct TodoList {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

#[derive(Clone)]
struct TodoItem {
    title: String,
    completed: bool,
}

#[derive(Debug, Clone)]
enum TodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
    TextInput(String, String),
    KeyDown(String),
}

impl std::str::FromStr for TodoMsg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(index_str) = s.strip_prefix("toggle:") {
             if let Ok(index) = index_str.parse::<usize>() {
                 return Ok(TodoMsg::Toggle(index));
             }
        }
        if let Some(index_str) = s.strip_prefix("remove:") {
             if let Ok(index) = index_str.parse::<usize>() {
                 return Ok(TodoMsg::Remove(index));
             }
        }
        if s == "add" {
            return Ok(TodoMsg::Add);
        }
        if let Some(text_payload) = s.strip_prefix("todo_input:text:") {
            return Ok(TodoMsg::TextInput("todo_input".to_string(), text_payload.to_string()));
        }
        if let Some(key) = s.strip_prefix("keydown:") {
            return Ok(TodoMsg::KeyDown(key.to_string()));
        }
        Err(())
    }
}

impl Model for TodoList {
    type Message = TodoMsg;

    fn view(&self) -> String {
        self.render().unwrap()
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
        match msg {
            TodoMsg::Toggle(index) => {
                if index < self.items.len() {
                    self.items[index].completed = !self.items[index].completed;
                    self.active_item = index;
                }
            }
            TodoMsg::Remove(index) => {
                if index < self.items.len() {
                    self.items.remove(index);
                    if self.active_item >= self.items.len() && !self.items.is_empty() {
                        self.active_item = self.items.len() - 1;
                    }
                }
            }
            TodoMsg::Add => {
                if !self.new_item_title.trim().is_empty() {
                    self.items.insert(0, TodoItem {
                        title: self.new_item_title.trim().to_string(),
                        completed: false,
                    });
                    self.new_item_title.clear();
                }
            }
            TodoMsg::TextInput(id, text) => {
                if id == "todo_input" {
                    // Ignore control characters
                    for c in text.chars() {
                        if !c.is_control() {
                            self.new_item_title.push(c);
                        }
                    }
                }
            }
            TodoMsg::KeyDown(key) => {
                match key.as_str() {
                    "Backspace" => {
                        // pop a char from new_item_title if it has focus
                        self.new_item_title.pop();
                    }
                    "ArrowUp" => {
                        if self.active_item > 0 {
                            self.active_item -= 1;
                            context.scroll_into_view(&format!("toggle:{}", self.active_item));
                        }
                    }
                    "ArrowDown" => {
                        if self.items.len() > 0 && self.active_item + 1 < self.items.len() {
                            self.active_item += 1;
                            context.scroll_into_view(&format!("toggle:{}", self.active_item));
                        }
                    }
                    "Enter" => {
                        if !self.new_item_title.is_empty() {
                            self.update(TodoMsg::Add, context);
                        } else if self.active_item < self.items.len() {
                            self.items[self.active_item].completed = !self.items[self.active_item].completed;
                        }
                    }
                    "Space" => {
                        // don't toggle if we're typing a space into the input box
                        // (we handle space in TextInput instead for the input field, but we assume
                        // space toggles item otherwise. For now, let's keep it simple.)
                    }
                    _ => {}
                }
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular, roboto_bold];
    // Leak fonts to satisfy static lifetime for winit event loop
    let fonts_ref: &'static [Font] = Box::leak(Box::new(fonts));

    let mut items = Vec::new();
    for i in 1..=20 {
        items.push(TodoItem {
            title: format!("Todo Item {}", i),
            completed: i % 3 == 0,
        });
    }

    let todo_list = TodoList { items, active_item: 0, new_item_title: String::new() };

    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    let runtime = Runtime::new(todo_list, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app("Xerune Todo Example", 800, 600, runtime, fonts_ref, | _ | {})
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
        support::linux_backend::run_app("Xerune Todo Example", 800, 600, runtime, fonts_ref, | _ | {})
    }
}

