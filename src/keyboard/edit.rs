//! A line of text being edited, with a caret.


/// A single line of text and a caret in it. The caret moves and deletes by
/// `char`, which is a whole character for the Latin text the on-screen
/// keyboard types (no combining marks or emoji sequences).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEdit {
    text: String,
    /// Byte offset of the caret, on a `char` boundary.
    caret: usize,
    max_len: Option<usize>,
}

impl TextEdit {
    /// `text`, with the caret at its end.
    pub fn new(text: &str) -> Self {
        let mut edit = Self::default();
        edit.insert(text);
        edit
    }

    /// Takes at most `max_len` characters: [`Self::insert`] drops the rest.
    /// Text already longer is cut.
    pub fn with_max_len(mut self, max_len: usize) -> Self {
        self.max_len = Some(max_len);
        if let Some((cut, _)) = self.text.char_indices().nth(max_len) {
            self.text.truncate(cut);
            self.caret = self.caret.min(cut);
        }
        self
    }

    /// The text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The text before the caret.
    pub fn before_caret(&self) -> &str {
        &self.text[..self.caret]
    }

    /// The text after the caret.
    pub fn after_caret(&self) -> &str {
        &self.text[self.caret..]
    }

    /// The caret's position, in characters.
    pub fn caret(&self) -> usize {
        self.before_caret().chars().count()
    }

    /// Number of characters.
    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    /// Whether there is no text.
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Inserts `text` at the caret and moves the caret after it. Control
    /// characters (newlines, the `\r` of Enter, the `\u{8}` of Backspace)
    /// are dropped: the line is single, and keys edit through the methods.
    /// Returns whether anything was inserted.
    pub fn insert(&mut self, text: &str) -> bool {
        let mut room = self.max_len.map_or(usize::MAX, |max| max.saturating_sub(self.len()));
        let mut inserted = false;
        for c in text.chars().filter(|c| !c.is_control()) {
            if room == 0 {
                break;
            }
            room -= 1;
            self.text.insert(self.caret, c);
            self.caret += c.len_utf8();
            inserted = true;
        }
        inserted
    }

    /// Deletes the character before the caret. Returns whether there was one.
    pub fn backspace(&mut self) -> bool {
        match self.before_caret().chars().next_back() {
            Some(c) => {
                self.caret -= c.len_utf8();
                self.text.remove(self.caret);
                true
            }
            None => false,
        }
    }

    /// Moves the caret one character left. Returns whether it moved.
    pub fn move_left(&mut self) -> bool {
        match self.before_caret().chars().next_back() {
            Some(c) => {
                self.caret -= c.len_utf8();
                true
            }
            None => false,
        }
    }

    /// Moves the caret one character right. Returns whether it moved.
    pub fn move_right(&mut self) -> bool {
        match self.after_caret().chars().next() {
            Some(c) => {
                self.caret += c.len_utf8();
                true
            }
            None => false,
        }
    }

    /// Moves the caret to the start.
    pub fn home(&mut self) {
        self.caret = 0;
    }

    /// Moves the caret to the end.
    pub fn end(&mut self) {
        self.caret = self.text.len();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_and_deletes_at_the_caret() {
        let mut edit = TextEdit::new("wifi");
        assert_eq!(edit.caret(), 4);
        edit.move_left();
        edit.move_left();
        edit.insert("-é-");
        assert_eq!(edit.text(), "wi-é-fi");
        assert_eq!((edit.before_caret(), edit.after_caret()), ("wi-é-", "fi"));
        edit.backspace();
        edit.backspace();
        assert_eq!(edit.text(), "wi-fi");
        assert_eq!(edit.caret(), 3);
        edit.home();
        assert!(!edit.backspace());
        assert!(!edit.move_left());
        edit.end();
        assert!(!edit.move_right());
        assert_eq!(edit.caret(), 5);
    }

    #[test]
    fn drops_control_characters() {
        let mut edit = TextEdit::new("a\r\nb\u{8}\t");
        assert_eq!(edit.text(), "ab");
        assert!(!edit.insert("\r"));
    }

    #[test]
    fn keeps_to_max_len() {
        let mut edit = TextEdit::new("abcdef").with_max_len(4);
        assert_eq!(edit.text(), "abcd");
        assert!(!edit.insert("x"));
        edit.home();
        edit.move_right();
        edit.backspace();
        assert!(edit.insert("xyz"));
        assert_eq!(edit.text(), "xbcd");
        assert_eq!(edit.caret(), 1);
    }
}
