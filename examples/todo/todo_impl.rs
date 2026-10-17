use std::sync::LazyLock;

use xerune::i18n::{Catalog, Translations};
use xerune::{Model, XeruneMessage, XeruneTemplate};

/// The example's strings, English, German and Korean (`i18n/*.txt`; Korean
/// only where the examples have the Hangul font).
static STRINGS: LazyLock<Translations> = LazyLock::new(|| {
    let catalog = |lang, text| Catalog::parse(lang, text).expect("the todo example's strings");
    let strings = Translations::new(catalog("en", include_str!("i18n/en.txt"))).with(catalog("de", include_str!("i18n/de.txt")));
    // Korean needs the Hangul font, which ARM builds leave out (examples/support).
    #[cfg(any(feature = "hangul", not(target_arch = "arm")))]
    let strings = strings.with(catalog("ko", include_str!("i18n/ko.txt")));
    strings
});

#[derive(XeruneTemplate, serde::Serialize, serde::Deserialize)]
#[template(path = "examples/todo/templates/todo_list.html")]
pub struct TodoList {
    pub(crate) items: Vec<TodoItem>,
    pub(crate) active_item: usize,
    new_item_title: String,
    /// The language shown, a tag like `de` (English when there are no
    /// strings for it).
    #[serde(default)]
    lang: String,
}

/// In English, so tests and benchmarks see the same list everywhere.
impl Default for TodoList {
    fn default() -> Self {
        Self::new("en")
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
    /// A list in language `lang`, starting with a few tasks in it.
    pub fn new(lang: &str) -> Self {
        let tasks = [
            ("work", false),
            ("home", true),
            ("work", false),
            ("fun", false),
            ("work", true),
            ("home", false),
            ("work", false),
            ("fun", true),
            ("home", false),
            ("work", false),
        ];
        Self {
            items: tasks
                .iter()
                .enumerate()
                .map(|(i, &(tag, completed))| TodoItem {
                    title: STRINGS.t(lang, &format!("task_{}", i + 1)).to_string(),
                    tag: tag.into(),
                    completed,
                })
                .collect(),
            active_item: 0,
            new_item_title: String::new(),
            lang: lang.to_string(),
        }
    }

    /// The text for `key` in the list's language.
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        STRINGS.t(&self.lang, key)
    }

    /// The languages to switch between.
    pub fn langs(&self) -> Vec<&'static str> {
        STRINGS.langs().collect()
    }

    /// Whether `lang` is the one shown (`de-AT` shows `de`).
    pub fn lang_is(&self, lang: &str) -> bool {
        STRINGS.catalog(&self.lang).map_or(STRINGS.langs().next() == Some(lang), |c| c.lang() == lang)
    }

    /// The name of `lang`, in it: "Deutsch".
    pub fn lang_name(&self, lang: &str) -> &'static str {
        STRINGS.t(lang, "language")
    }

    /// A category's name, from its tag (`work`).
    pub fn tag_name(&self, tag: &str) -> String {
        STRINGS.t(&self.lang, &format!("tag_{tag}")).to_string()
    }

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
    /// The input's text from the on-screen keyboard, on touch-only devices.
    #[xerune(prefix = "todo_input:value:")]
    TodoValue(String),
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
    /// Shows the list in another language.
    #[xerune(prefix = "lang:")]
    Lang(String),
}

impl Model for TodoList {
    type Message = TodoMsg;

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context<Self::Message>) {
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
            TodoMsg::TodoValue(text) => self.new_item_title = text,
            TodoMsg::Lang(lang) => self.lang = lang,
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
