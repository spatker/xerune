//! The on-screen keyboard: text entry on devices without a keyboard.
//!
//! A model asks for text with [`crate::Context::edit_text`]; the runtime
//! then shows the keyboard over the whole screen, in place of the model's
//! view, until the user confirms or cancels, and sends the model the
//! message made from the result. Keys of a hardware keyboard type into it
//! too, and the [`crate::KeyMap`] decides which keys confirm, cancel,
//! delete and move the caret.

mod edit;
mod hangul;

pub use edit::TextEdit;

use crate::i18n::{Catalog, Translations};
use crate::keys::KeyAction;
use crate::{XeruneMessage, XeruneTemplate};

/// Makes the model's message, if any, from the keyboard's result.
pub(crate) type OnText<M> = Box<dyn FnOnce(Option<String>) -> Option<M> + Send>;

/// What a model asks the keyboard for: a title saying what to type, the
/// text to start from, and limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextRequest {
    title: String,
    text: String,
    password: bool,
    min_len: usize,
    max_len: Option<usize>,
    lang: Option<String>,
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

    /// Lays the keys out and labels them for language `lang` (`de`, `en-US`).
    /// The keyboard has English, German and Korean (two-set, composing
    /// jamo into syllables); it is English for others.
    /// A text input's keyboard takes the input's `lang` (or an ancestor's).
    pub fn lang(mut self, lang: &str) -> Self {
        self.lang = Some(lang.to_string());
        self
    }
}

/// The keyboard's languages: their letter rows (`row1` to `row3`, and
/// `shift_row1` to `shift_row3` where Shift doesn't just capitalize), labels,
/// and `compose = hangul` where typed jamo join into syllables. English,
/// the first, has every label.
fn translations() -> Translations {
    let catalog = |lang, source| Catalog::parse(lang, source).expect("the keyboard's own strings");
    Translations::new(catalog("en", include_str!("en.txt")))
        .with(catalog("de", include_str!("de.txt")))
        .with(catalog("ko", include_str!("ko.txt")))
}

/// The keys of the symbol pages, by row (the letters are the language's);
/// the third row also has the page's switch key and Del, and every page
/// has the bottom row.
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

/// The index of the key in `row` whose middle is nearest `x`.
fn nearest(row: &[(KeyboardMsg, f32)], x: f32) -> usize {
    (0..row.len()).min_by(|&a, &b| (row[a].1 - x).abs().total_cmp(&(row[b].1 - x).abs())).unwrap_or(0)
}

/// `c` in upper case, if that is one letter: ß stays (its capital, SS,
/// would be two).
fn upper_case(c: char) -> String {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u.to_string(),
        _ => c.to_string(),
    }
}

/// Whether `message` is one of the keyboard's (`kb:...`), which
/// [`crate::Runtime::handle_messages`] sends to it while it is open.
#[cfg(not(target_arch = "wasm32"))]
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
    /// The key the D-pad is on, as (row, index) in [`Self::focus_rows`];
    /// none until a key moves it, and then Activate presses it.
    focus: Option<(usize, usize)>,
    /// Where across the keyboard moving up and down aims, so going through
    /// a shorter row comes back to the same column.
    column: Option<f32>,
    /// The language of the letters and labels (see [`translations`]).
    lang: String,
    tr: Translations,
    /// The jamo before the caret were typed here and may still join with
    /// the next one (Korean); moving or typing anything else ends it.
    composing: bool,
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
            focus: None,
            column: None,
            lang: request.lang.unwrap_or_default(),
            tr: translations(),
            composing: false,
        }
    }

    /// The label `key` in the keyboard's language.
    fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.tr.t(&self.lang, key)
    }

    /// `key` of the keyboard's language only, without falling back to
    /// English: what only some layouts have.
    fn own(&self, key: &str) -> Option<&str> {
        self.tr.catalog(&self.lang)?.get(key)
    }

    /// Types `text` from a key: a jamo joins the syllable before the caret
    /// while composing.
    fn type_key(&mut self, text: &str) {
        let mut chars = text.chars();
        let jamo = match (chars.next(), chars.next()) {
            (Some(c), None) if self.own("compose") == Some("hangul") && hangul::is_jamo(c) => Some(c),
            _ => None,
        };
        let Some(jamo) = jamo else {
            self.composing = false;
            self.edit.insert(text);
            return;
        };
        let prev = self.edit.before_caret().chars().next_back().filter(|_| self.composing);
        match prev.and_then(|prev| hangul::type_jamo(prev, jamo).map(|joined| (prev, joined))) {
            Some((prev, joined)) => self.replace_before_caret(prev, &joined),
            None => {
                self.edit.insert(text);
            }
        }
        self.composing = true;
    }

    /// Replaces `prev`, the character before the caret, with `text`, or
    /// keeps it when `text` doesn't fit in the length limit.
    fn replace_before_caret(&mut self, prev: char, text: &str) {
        self.edit.backspace();
        if !self.edit.insert(text) {
            self.edit.insert(prev.encode_utf8(&mut [0; 4]));
        }
    }

    /// How many keys wide the keyboard is: its longest row of letters or
    /// symbols (10, or 11 with German's umlauts).
    fn columns(&self) -> usize {
        ["row1", "row2", "row3"].iter().map(|row| self.t(row).chars().count()).chain([10]).max().unwrap_or(10)
    }

    /// The width of a letter or symbol key, as a percentage of a row.
    fn key_width(&self) -> String {
        format!("{:.2}", 92.0 / self.columns() as f32)
    }

    /// Handles a key's message. Returns the result once the keyboard is
    /// done: the text on OK, `None` on Cancel.
    pub(crate) fn press(&mut self, msg: KeyboardMsg) -> Option<Option<String>> {
        match msg {
            KeyboardMsg::Type(text) => {
                self.type_key(&text);
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
                self.composing = false;
                self.edit.insert(" ");
            }
            KeyboardMsg::Backspace => {
                // While composing, takes off the last jamo: 한 → 하 → ㅎ.
                let prev = self.edit.before_caret().chars().next_back().filter(|_| self.composing);
                match prev.and_then(|prev| hangul::remove_jamo(prev).map(|rest| (prev, rest))) {
                    Some((prev, rest)) => self.replace_before_caret(prev, rest.encode_utf8(&mut [0; 4])),
                    None => {
                        self.composing = false;
                        self.edit.backspace();
                    }
                }
            }
            KeyboardMsg::Left => {
                self.composing = false;
                self.edit.move_left();
            }
            KeyboardMsg::Right => {
                self.composing = false;
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
    /// what the key map says it does. The direction keys move the focus;
    /// Activate presses the focused key, or OK before anything is focused
    /// (Enter after typing on a hardware keyboard).
    pub(crate) fn key(&mut self, action: KeyAction) -> Option<Option<String>> {
        match action {
            KeyAction::Activate => match self.focus() {
                Some((row, index)) => {
                    let (msg, x) = self.focus_rows().swap_remove(row).swap_remove(index);
                    let result = self.press(msg);
                    // The page or Show may have changed the row: stay over the same place.
                    let rows = self.focus_rows();
                    self.focus = Some((row, nearest(&rows[row], x)));
                    result
                }
                None => self.press(KeyboardMsg::Ok),
            },
            KeyAction::Cancel => self.press(KeyboardMsg::Cancel),
            KeyAction::Backspace => self.press(KeyboardMsg::Backspace),
            KeyAction::Up | KeyAction::Down | KeyAction::Left | KeyAction::Right | KeyAction::Next | KeyAction::Prev => {
                self.move_focus(action);
                None
            }
        }
    }

    /// The focused key, kept within its row (a click may have changed
    /// the page under it).
    fn focus(&self) -> Option<(usize, usize)> {
        let (row, index) = self.focus?;
        let rows = self.focus_rows();
        let row = row.min(rows.len() - 1);
        Some((row, index.min(rows[row].len() - 1)))
    }

    /// Moves the focus: left and right around the row, up and down to the
    /// key nearest the column, next and previous in reading order. The
    /// first move only shows the focus, on Q.
    fn move_focus(&mut self, action: KeyAction) {
        let Some((mut row, mut index)) = self.focus() else {
            self.focus = Some((1, 0));
            return;
        };
        let rows = self.focus_rows();
        let len = rows[row].len();
        match action {
            KeyAction::Left => index = (index + len - 1) % len,
            KeyAction::Right => index = (index + 1) % len,
            KeyAction::Up | KeyAction::Down => {
                let x = *self.column.get_or_insert(rows[row][index].1);
                row = match action {
                    KeyAction::Up => row.saturating_sub(1),
                    _ => (row + 1).min(rows.len() - 1),
                };
                self.focus = Some((row, nearest(&rows[row], x)));
                return;
            }
            KeyAction::Next if index + 1 < len => index += 1,
            KeyAction::Next => (row, index) = ((row + 1) % rows.len(), 0),
            KeyAction::Prev if index > 0 => index -= 1,
            KeyAction::Prev => {
                row = (row + rows.len() - 1) % rows.len();
                index = rows[row].len() - 1;
            }
            _ => return,
        }
        self.column = None;
        self.focus = Some((row, index));
    }

    /// The keys the focus moves over, by row as laid out (the buttons by
    /// the text first), with their messages and where their middles are
    /// across the keyboard, in key widths: the keyboard is
    /// [`Self::columns`] wide.
    fn focus_rows(&self) -> Vec<Vec<(KeyboardMsg, f32)>> {
        let width = self.columns() as f32;
        let mut actions = Vec::new();
        if self.password {
            actions.push(KeyboardMsg::Reveal);
        }
        actions.extend([KeyboardMsg::Cancel, KeyboardMsg::Ok]);
        // To the right of the text in landscape, across under it in portrait.
        let (screen_width, height) = crate::screen::size();
        let n = actions.len();
        let actions = actions.into_iter().enumerate().map(|(i, msg)| {
            let x = if height > screen_width { width * (i as f32 + 0.5) / n as f32 } else { width - 1.4 * (n - i) as f32 + 0.7 };
            (msg, x)
        });
        let centered = |keys: Vec<String>| {
            let start = (width - keys.len() as f32) / 2.0;
            keys.into_iter().enumerate().map(move |(i, k)| (KeyboardMsg::Type(k), start + i as f32 + 0.5))
        };
        let third = self.row(2);
        // The switch key and Del share what the keys leave.
        let side = (width - third.len() as f32) / 2.0;
        let switch = if self.letters() { KeyboardMsg::Shift } else { KeyboardMsg::MoreSymbols };
        let third = core::iter::once((switch, side / 2.0)).chain(centered(third)).chain([(KeyboardMsg::Backspace, width - side / 2.0)]);
        let bottom = [(KeyboardMsg::Symbols, 0.75), (KeyboardMsg::Left, 2.0), (KeyboardMsg::Space, 5.75), (KeyboardMsg::Right, 9.5)];
        vec![
            actions.collect(),
            centered(self.row(0)).collect(),
            centered(self.row(1)).collect(),
            third.collect(),
            bottom.into_iter().map(|(msg, x)| (msg, x * width / 10.0)).collect(),
        ]
    }

    /// Whether the focus is on key `index` of `row` (of [`Self::focus_rows`]).
    fn focus_is(&self, row: usize, index: usize) -> bool {
        self.focus() == Some((row, index))
    }

    /// Whether the focus is on Show (0), Cancel (1) or OK (2).
    fn action_focused(&self, action: usize) -> bool {
        match (self.password, action) {
            (false, 0) => false,
            (false, action) => self.focus_is(0, action - 1),
            (true, action) => self.focus_is(0, action),
        }
    }

    /// Types text from a hardware keyboard.
    pub(crate) fn type_text(&mut self, text: &str) {
        self.composing = false;
        self.edit.insert(text);
    }

    fn can_confirm(&self) -> bool {
        self.edit.len() >= self.min_len
    }

    /// The characters of `row` on the current page.
    fn row(&self, row: usize) -> Vec<String> {
        let shifted = self.page == Page::Letters && self.shift != Shift::Off;
        // A layout's own shifted row (Korean: double consonants), or capitals.
        if let Some(keys) = self.own(["shift_row1", "shift_row2", "shift_row3"][row]).filter(|_| shifted) {
            return keys.chars().map(String::from).collect();
        }
        let keys = match self.page {
            Page::Letters => self.t(["row1", "row2", "row3"][row]),
            Page::Symbols => SYMBOLS[row],
            Page::MoreSymbols => MORE_SYMBOLS[row],
        };
        keys.chars().map(|c| if shifted { upper_case(c) } else { c.to_string() }).collect()
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
        let mut kb = keyboard(TextRequest::new("Name"));
        let mut typable: Vec<char> = SYMBOLS.iter().chain(&MORE_SYMBOLS).flat_map(|row| row.chars()).collect();
        for shift in [false, true] {
            if shift {
                kb.press(KeyboardMsg::Shift);
            }
            typable.extend((0..3).flat_map(|row| kb.row(row)).flat_map(|key| key.chars().collect::<Vec<_>>()));
        }
        typable.push(' ');
        for c in ' '..='~' {
            assert!(typable.contains(&c), "{c:?} has no key");
        }
    }

    #[test]
    fn d_pad_moves_over_the_keys_and_presses_them() {
        use KeyAction::*;
        crate::screen::set_viewport(800.0, 480.0);
        let mut kb = keyboard(TextRequest::new("Name"));
        // Before any move, Activate is OK.
        kb.move_focus(Right);
        assert_eq!(kb.focus(), Some((1, 0)), "the first move shows the focus on Q");
        kb.key(Left);
        assert_eq!(kb.focus(), Some((1, 9)), "around to P");
        kb.key(Activate);
        assert_eq!(kb.edit.text(), "p");
        // Down from P: L (the row is shorter), then Del, then the right caret key.
        kb.key(Down);
        assert_eq!(kb.focus(), Some((2, 8)));
        kb.key(Down);
        assert_eq!(kb.focus(), Some((3, 8)));
        kb.key(Activate);
        assert_eq!(kb.edit.text(), "");
        kb.key(Down);
        assert_eq!(kb.focus(), Some((4, 3)));
        // Back up keeps the column: Del, L, P, then OK.
        kb.key(Up);
        kb.key(Up);
        kb.key(Up);
        assert_eq!(kb.focus(), Some((1, 9)));
        kb.key(Up);
        assert!(kb.action_focused(2));
        // Next and Prev go in reading order, around the end.
        kb.key(Next);
        assert_eq!(kb.focus(), Some((1, 0)));
        kb.key(Prev);
        kb.key(Prev);
        assert!(kb.action_focused(1));
        assert_eq!(kb.key(Activate), Some(None));
    }

    #[test]
    fn focus_stays_in_place_when_the_page_changes() {
        use KeyAction::*;
        crate::screen::set_viewport(800.0, 480.0);
        let mut kb = keyboard(TextRequest::new("Name"));
        kb.key(Down);
        kb.key(Down);
        kb.key(Down);
        kb.key(Down);
        assert_eq!(kb.focus(), Some((4, 0)), "?123");
        kb.key(Activate);
        kb.key(Up);
        kb.key(Activate);
        assert_eq!(kb.focus(), Some((3, 0)));
        assert_eq!(kb.page, Page::MoreSymbols);
        // Del moves with the row's length.
        kb.key(Left);
        let (row, index) = kb.focus().unwrap();
        assert!(matches!(kb.focus_rows()[row][index].0, KeyboardMsg::Backspace));
    }

    #[test]
    fn german_has_qwertz_with_umlauts() {
        use KeyAction::*;
        crate::screen::set_viewport(800.0, 480.0);
        let mut kb = keyboard(TextRequest::new("Name").lang("de-DE"));
        assert_eq!(kb.row(0).concat(), "qwertzuiopü");
        assert_eq!(kb.t("cancel"), "Abbrechen");
        assert_eq!(kb.columns(), 11);
        kb.press(KeyboardMsg::Shift);
        assert_eq!(kb.row(2).concat(), "YXCVBNMß");
        // Down from Ü: Ä, under it.
        kb.key(Down);
        kb.key(Left);
        kb.key(Down);
        assert_eq!(kb.focus(), Some((2, 10)));
        kb.key(Activate);
        assert_eq!(kb.edit.text(), "Ä");
        let kb = keyboard(TextRequest::new("Name").lang("fr"));
        assert_eq!(kb.row(0).concat(), "qwertyuiop", "English for other languages");
    }

    #[test]
    fn korean_composes_syllables_as_typed() {
        let mut kb = keyboard(TextRequest::new("이름").lang("ko"));
        assert_eq!(kb.row(0).concat(), "ㅂㅈㄷㄱㅅㅛㅕㅑㅐㅔ");
        assert_eq!(kb.t("cancel"), "취소");
        let type_keys = |kb: &mut Keyboard, keys: &str| {
            for c in keys.chars() {
                kb.press(if c == ' ' { KeyboardMsg::Space } else { KeyboardMsg::Type(c.to_string()) });
            }
        };
        type_keys(&mut kb, "ㅎㅏㄴㄱㅜㄱ ㅁㅏㄹ");
        assert_eq!(kb.edit.text(), "한국 말");
        // Backspace takes the jamo off one by one while composing.
        kb.press(KeyboardMsg::Backspace);
        assert_eq!(kb.edit.text(), "한국 마");
        kb.press(KeyboardMsg::Backspace);
        kb.press(KeyboardMsg::Backspace);
        assert_eq!(kb.edit.text(), "한국 ");
        // After moving, a jamo doesn't join the syllable before the caret.
        kb.press(KeyboardMsg::Left);
        kb.press(KeyboardMsg::Right);
        kb.press(KeyboardMsg::Backspace);
        type_keys(&mut kb, "ㅏ");
        assert_eq!(kb.edit.text(), "한국ㅏ");
        // Shift types double consonants.
        kb.press(KeyboardMsg::Shift);
        assert_eq!(kb.row(0).concat(), "ㅃㅉㄸㄲㅆㅛㅕㅑㅒㅖ");
        assert_eq!(kb.row(1).concat(), "ㅁㄴㅇㄹㅎㅗㅓㅏㅣ");
        // English has no shifted rows of its own.
        let mut en = keyboard(TextRequest::new("Name"));
        en.press(KeyboardMsg::Shift);
        assert_eq!(en.row(0)[0], "Q");
        en.press(KeyboardMsg::Type("ㄱ".into()));
        en.press(KeyboardMsg::Type("ㅏ".into()));
        assert_eq!(en.edit.text(), "ㄱㅏ", "only Korean composes");
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
