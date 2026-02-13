use askama::Template;
use fontdue::Font;
use xerune::{Runtime, Model};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

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

fn main() -> anyhow::Result<()> {
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
            title: Box::leak(format!("Todo Item {}", i).into_boxed_str()),
            completed: i % 3 == 0,
        });
    }

    let todo_list = TodoList { items };

    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    let runtime = Runtime::new(todo_list, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app("RMTUI Todo Example", 800, 600, runtime, fonts_ref, None)
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
        support::linux_backend::run_app("RMTUI Todo Example", 800, 600, runtime, fonts_ref, None)
    }
}

