//! The on-screen keyboard: text entry on devices without a keyboard.
//!
//! A model asks for text with [`crate::Context::edit_text`]; the runtime
//! then shows the keyboard over the whole screen, in place of the model's
//! view, until the user confirms or cancels, and sends the model the
//! message made from the result. Keys of a hardware keyboard type into it
//! too, and the [`crate::KeyMap`] decides which keys confirm, cancel,
//! delete and move the caret.

mod edit;

pub use edit::TextEdit;

use crate::alloc_prelude::*;
use crate::keys::KeyAction;
use crate::{XeruneMessage, XeruneTemplate};

/// Makes the model's message from the keyboard's result.
pub(crate) type OnText<M> = Box<dyn FnOnce(Option<String>) -> M + Send>;

/// What a model asks the keyboard for: a title saying what to type, the
/// text to start from, and limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextRequest {
    title: String,
    text: String,
    password: bool,
    min_len: usize,
    max_len: Option<usize>,
}

impl TextRequest {
    /// A request titled `title`, e.g. "Wi-Fi password", starting empty.
    pub fn new(title: &str) -> Self {
        Self { title: title.to_string(), ..Self::default() }
    }

    /// Starts from `text` instead of nothing.
    pub fn text(mut self, text: &str) -> Self {
        self.text = text.to_string();
        self
    }

    /// Shows the text as dots, with a key to reveal it.
    pub fn password(mut self) -> Self {
        self.password = true;
        self
    }

    /// OK does nothing until there are at least `min_len` characters (a
    /// WPA passphrase has 8 to 63).
    pub fn min_len(mut self, min_len: usize) -> Self {
        self.min_len = min_len;
        self
    }

    /// Takes at most `max_len` characters.
    pub fn max_len(mut self, max_len: usize) -> Self {
        self.max_len = Some(max_len);
        self
    }
}

/// The keys of a page, by row; the third row also has the page's switch
/// key and Del, and every page has the bottom row.
const LETTERS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const SYMBOLS: [&str; 3] = ["1234567890", "-/:;()$&@\"", ".,?!'"];
const MORE_SYMBOLS: [&str; 3] = ["[]{}#%^*+=", "_\\|~<>`", ".,?!'"];

/// The keyboard's pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Letters,
    Symbols,
    MoreSymbols,
}

/// Shift: off, for the next letter, or locked until pressed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shift {
    Off,
    Once,
    Locked,
}

/// A key's message, from the keyboard's template. They start with `kb:`,
/// so snapshots can send them (`XERUNE_SNAPSHOT_MESSAGES="edit;kb:type:a"`)
/// without clashing with the model's.
#[derive(Debug, Clone, XeruneMessage)]
pub(crate) enum KeyboardMsg {
    /// Types a character.
    #[xerune(prefix = "kb:type:")]
    Type(String),
    #[xerune(rename = "kb:shift")]
    Shift,
    /// Letters or the first symbol page.
    #[xerune(rename = "kb:symbols")]
    Symbols,
    /// The first or second symbol page.
    #[xerune(rename = "kb:more_symbols")]
    MoreSymbols,
    #[xerune(rename = "kb:space")]
    Space,
    #[xerune(rename = "kb:backspace")]
    Backspace,
    #[xerune(rename = "kb:left")]
    Left,
    #[xerune(rename = "kb:right")]
    Right,
    #[xerune(rename = "kb:reveal")]
    Reveal,
    #[xerune(rename = "kb:ok")]
    Ok,
    #[xerune(rename = "kb:cancel")]
    Cancel,
}

/// Whether `message` is one of the keyboard's (`kb:...`), which
/// [`crate::Runtime::handle_messages`] sends to it while it is open.
#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm", feature = "browser")))]
pub(crate) fn is_keyboard_message(message: &str) -> bool {
    <KeyboardMsg as core::str::FromStr>::from_str(message).is_ok()
}

/// The keyboard's state and view, built by the runtime in place of the
/// model's view while text is asked for.
#[derive(XeruneTemplate)]
#[template(path = "src/keyboard/keyboard.html")]
pub(crate) struct Keyboard {
    title: String,
    edit: TextEdit,
    password: bool,
    revealed: bool,
    min_len: usize,
    max_len: Option<usize>,
    page: Page,
    shift: Shift,
}

impl Keyboard {
    pub(crate) fn new(request: TextRequest) -> Self {
        let mut edit = TextEdit::new(&request.text);
        if let Some(max_len) = request.max_len {
            edit = edit.with_max_len(max_len);
        }
        Self {
            title: request.title,
            edit,
            password: request.password,
            revealed: false,
            min_len: request.min_len,
            max_len: request.max_len,
            page: Page::Letters,
            shift: Shift::Off,
        }
    }

    /// Handles a key's message. Returns the result once the keyboard is
    /// done: the text on OK, `None` on Cancel.
    pub(crate) fn press(&mut self, msg: KeyboardMsg) -> Option<Option<String>> {
        match msg {
            KeyboardMsg::Type(text) => {
                self.edit.insert(&text);
                if self.shift == Shift::Once {
                    self.shift = Shift::Off;
                }
            }
            KeyboardMsg::Shift => {
                self.shift = match self.shift {
                    Shift::Off => Shift::Once,
                    Shift::Once => Shift::Locked,
                    Shift::Locked => Shift::Off,
                };
            }
            KeyboardMsg::Symbols => {
                self.page = if self.page == Page::Letters { Page::Symbols } else { Page::Letters };
            }
            KeyboardMsg::MoreSymbols => {
                self.page = if self.page == Page::Symbols { Page::MoreSymbols } else { Page::Symbols };
            }
            KeyboardMsg::Space => {
                self.edit.insert(" ");
            }
            KeyboardMsg::Backspace => {
                self.edit.backspace();
            }
            KeyboardMsg::Left => {
                self.edit.move_left();
            }
            KeyboardMsg::Right => {
                self.edit.move_right();
            }
            KeyboardMsg::Reveal => {
                self.revealed = !self.revealed;
            }
            KeyboardMsg::Ok => {
                if self.can_confirm() {
                    return Some(Some(self.edit.text().to_string()));
                }
            }
            KeyboardMsg::Cancel => return Some(None),
        }
        None
    }

    /// Handles a key of a hardware keyboard or the device's buttons, by
    /// what the key map says it does.
    pub(crate) fn key(&mut self, action: KeyAction) -> Option<Option<String>> {
        let msg = match action {
            KeyAction::Activate => KeyboardMsg::Ok,
            KeyAction::Cancel => KeyboardMsg::Cancel,
            KeyAction::Backspace => KeyboardMsg::Backspace,
            KeyAction::Left => KeyboardMsg::Left,
            KeyAction::Right => KeyboardMsg::Right,
            KeyAction::Up | KeyAction::Down | KeyAction::Next | KeyAction::Prev => return None,
        };
        self.press(msg)
    }

    /// Types text from a hardware keyboard.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.edit.insert(text);
    }

    fn can_confirm(&self) -> bool {
        self.edit.len() >= self.min_len
    }

    /// The characters of `row` on the current page.
    fn row(&self, row: usize) -> Vec<String> {
        let keys = match self.page {
            Page::Letters => LETTERS[row],
            Page::Symbols => SYMBOLS[row],
            Page::MoreSymbols => MORE_SYMBOLS[row],
        };
        let upper = self.page == Page::Letters && self.shift != Shift::Off;
        keys.chars().map(|c| if upper { c.to_ascii_uppercase() } else { c }.to_string()).collect()
    }

    fn letters(&self) -> bool {
        self.page == Page::Letters
    }

    fn symbols(&self) -> bool {
        self.page == Page::Symbols
    }

    fn shift_on(&self) -> bool {
        self.shift != Shift::Off
    }

    fn shift_locked(&self) -> bool {
        self.shift == Shift::Locked
    }

    fn masked(&self) -> bool {
        self.password && !self.revealed
    }

    /// The text before and after the caret, as shown: dots for a hidden
    /// password.
    fn before_caret(&self) -> String {
        self.shown(self.edit.before_caret())
    }

    fn after_caret(&self) -> String {
        self.shown(self.edit.after_caret())
    }

    fn shown(&self, text: &str) -> String {
        if self.masked() { "\u{2022}".repeat(text.chars().count()) } else { text.to_string() }
    }

    /// The length against the limits, e.g. "5 / 8–63", or "" without limits.
    fn count(&self) -> String {
        let len = self.edit.len();
        match (self.min_len, self.max_len) {
            (0, None) => String::new(),
            (0, Some(max)) => format!("{len} / {max}"),
            (min, None) => format!("{len} / {min}+"),
            (min, Some(max)) => format!("{len} / {min}\u{2013}{max}"),
        }
    }

    fn too_short(&self) -> bool {
        !self.can_confirm()
    }
}

/// The template needs a model; the runtime calls [`Keyboard::press`],
/// which says when the keyboard is done.
impl crate::Model for Keyboard {
    type Message = KeyboardMsg;

    fn update(&mut self, msg: KeyboardMsg, _context: &mut crate::Context<KeyboardMsg>) {
        self.press(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keyboard(request: TextRequest) -> Keyboard {
        Keyboard::new(request)
    }

    #[test]
    fn shift_capitalizes_one_letter_or_locks() {
        let mut kb = keyboard(TextRequest::new("Name"));
        kb.press(KeyboardMsg::Shift);
        assert_eq!(kb.row(0)[0], "Q");
        kb.press(KeyboardMsg::Type("Q".into()));
        assert_eq!(kb.row(0)[0], "q");
        kb.press(KeyboardMsg::Shift);
        kb.press(KeyboardMsg::Shift);
        kb.press(KeyboardMsg::Type("A".into()));
        kb.press(KeyboardMsg::Type("B".into()));
        assert!(kb.shift_locked());
        assert_eq!(kb.edit.text(), "QAB");
    }

    #[test]
    fn symbol_pages_type_all_printable_ascii() {
        let mut typable: Vec<char> = LETTERS.iter().chain(&SYMBOLS).chain(&MORE_SYMBOLS).flat_map(|row| row.chars()).collect();
        typable.extend(LETTERS.iter().flat_map(|row| row.chars()).map(|c| c.to_ascii_uppercase()));
        typable.push(' ');
        for c in ' '..='~' {
            assert!(typable.contains(&c), "{c:?} has no key");
        }
    }

    #[test]
    fn ok_waits_for_min_len_and_cancel_gives_none() {
        let mut kb = keyboard(TextRequest::new("Password").password().min_len(3).max_len(5).text("ab"));
        assert_eq!(kb.before_caret(), "\u{2022}\u{2022}");
        assert_eq!(kb.count(), "2 / 3\u{2013}5");
        assert_eq!(kb.press(KeyboardMsg::Ok), None);
        kb.press(KeyboardMsg::Reveal);
        assert_eq!(kb.before_caret(), "ab");
        kb.type_text("cdefg");
        assert_eq!(kb.press(KeyboardMsg::Ok), Some(Some("abcde".to_string())));
        assert_eq!(kb.key(KeyAction::Cancel), Some(None));
    }
}
