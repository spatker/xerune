use xerune::{Model, XeruneMessage, XeruneTemplate};

#[derive(XeruneTemplate, serde::Serialize, serde::Deserialize)]
#[template(path = "examples/todo/templates/todo_list.html")]
pub struct TodoList {
    pub(crate) items: Vec<TodoItem>,
    pub(crate) active_item: usize,
    new_item_title: String,
}

impl Default for TodoList {
    fn default() -> Self {
        let tasks = [
            ("Flash the new firmware to the Pi", "work", false),
            ("Water the balcony tomatoes", "home", true),
            ("Profile the conic gradient path on ARM", "work", false),
            ("Book climbing gym for Saturday", "fun", false),
            ("Reply to the display vendor", "work", true),
            ("Buy coffee beans", "home", false),
            ("Record a demo video of the showcase", "work", false),
            ("Try the new ramen place", "fun", true),
            ("Clean up the 3D printer bed", "home", false),
            ("Write release notes", "work", false),
        ];
        Self {
            items: tasks
                .iter()
                .map(|&(title, tag, completed)| TodoItem { title: title.into(), tag: tag.into(), completed })
                .collect(),
            active_item: 0,
            new_item_title: String::new(),
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub title: String,
    /// Category chip: `work`, `home` or `fun`.
    pub tag: String,
    pub completed: bool,
}

impl TodoList {
    pub fn done_count(&self) -> usize {
        self.items.iter().filter(|i| i.completed).count()
    }

    pub fn open_count(&self) -> usize {
        self.items.len() - self.done_count()
    }

    pub fn done_pct(&self) -> usize {
        if self.items.is_empty() { 100 } else { self.done_count() * 100 / self.items.len() }
    }
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
                        tag: "work".into(),
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
                            context.scroll_into_view(&format!("item-{}", self.active_item));
                        }
                    }
                    "ArrowDown" => {
                        if self.items.len() > 0 && self.active_item + 1 < self.items.len() {
                            self.active_item += 1;
                            context.scroll_into_view(&format!("item-{}", self.active_item));
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
