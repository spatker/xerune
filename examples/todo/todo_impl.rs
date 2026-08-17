#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;


#[derive(XeruneTemplate, serde::Serialize, serde::Deserialize)]
#[template(path = "todo_list.html")]
pub struct TodoList {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

impl Default for TodoList {
    fn default() -> Self {
        let mut items = Vec::new();
        for i in 1..=20 {
            items.push(TodoItem {
                title: format!("Todo Item {}", i),
                completed: i % 3 == 0,
            });
        }
        Self {
            items,
            active_item: 0,
            new_item_title: String::new(),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub title: String,
    pub completed: bool,
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum TodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
    #[xerune(prefix = "todo_input:text:")]
    TodoInput(String),
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
}

impl Model for TodoList {
    type Message = TodoMsg;

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
            TodoMsg::TodoInput(text) => {
                for c in text.chars() {
                    if !c.is_control() {
                        self.new_item_title.push(c);
                    }
                }
            }
            TodoMsg::KeyDown(key) => {
                match key.as_str() {
                    "Backspace" => {
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
                    _ => {}
                }
            }
        }
    }
}

// Logic to run this natively
#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<TodoList, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let todo_list = TodoList::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let runtime = Runtime::new(todo_list, measurer);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Todo Example", 800, 600, runtime, render_frame, | _ | {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Todo Example", 800, 600, runtime, render_frame, | _ | {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Todo Example", 800, 600, runtime, render_frame, | _ | {})?;
    }

    Ok(())
}
