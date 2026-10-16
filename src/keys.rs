//! What the keys of a device mean to the runtime's own UI (the on-screen
//! keyboard). Backends name keys differently (winit `ArrowUp`, evdev `Up`
//! or `KEY_ENTER`), and a device may have only a few buttons, e.g. the
//! media keys of a music player, so a [`KeyMap`] says which key does what.
//! Models still get every key as `keydown:<name>` and map them themselves.

use crate::alloc_prelude::*;

/// What a key does in the runtime's own UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyAction {
    /// Move the focus up.
    Up,
    /// Move the focus down.
    Down,
    /// Move the focus left.
    Left,
    /// Move the focus right.
    Right,
    /// Move the focus to the next element, in reading order, for devices
    /// with only two buttons to move with.
    Next,
    /// Move the focus to the previous element.
    Prev,
    /// Press the focused element.
    Activate,
    /// Leave without a result (the keyboard's Cancel).
    Cancel,
    /// Delete the character before the caret.
    Backspace,
}

/// Which key names (as in [`crate::InputEvent::KeyDown`]) do which
/// [`KeyAction`]. The default knows the names of the built-in backends; a
/// device adds its own with [`Self::bind`].
#[derive(Debug, Clone)]
pub struct KeyMap {
    bindings: Vec<(String, KeyAction)>,
}

impl KeyMap {
    /// A map without keys.
    pub fn empty() -> Self {
        Self { bindings: Vec::new() }
    }

    /// Makes `key` do `action`, instead of what it did before.
    pub fn bind(mut self, key: &str, action: KeyAction) -> Self {
        self.unbind_key(key);
        self.bindings.push((key.to_string(), action));
        self
    }

    /// Makes `key` do nothing.
    pub fn unbind(mut self, key: &str) -> Self {
        self.unbind_key(key);
        self
    }

    fn unbind_key(&mut self, key: &str) {
        self.bindings.retain(|(k, _)| k != key);
    }

    /// What `key` does, if anything.
    pub fn action(&self, key: &str) -> Option<KeyAction> {
        self.bindings.iter().find(|(k, _)| k == key).map(|&(_, action)| action)
    }
}

impl Default for KeyMap {
    /// Arrows move, Enter presses, Escape cancels, Backspace deletes, and
    /// Tab and the media keys' next / previous move in reading order. Names
    /// from winit and the evdev backend.
    fn default() -> Self {
        use KeyAction::*;
        let bindings: &[(&str, KeyAction)] = &[
            ("ArrowUp", Up), ("Up", Up), ("KEY_UP", Up),
            ("ArrowDown", Down), ("Down", Down), ("KEY_DOWN", Down),
            ("ArrowLeft", Left), ("Left", Left), ("KEY_LEFT", Left),
            ("ArrowRight", Right), ("Right", Right), ("KEY_RIGHT", Right),
            ("Tab", Next), ("Next", Next),
            ("Prev", Prev),
            ("Enter", Activate), ("NumpadEnter", Activate), ("KEY_ENTER", Activate), ("KEY_KPENTER", Activate), ("KEY_OK", Activate), ("KEY_SELECT", Activate), ("PlayPause", Activate),
            ("Escape", Cancel), ("KEY_ESC", Cancel), ("Back", Cancel), ("KEY_BACK", Cancel),
            ("Backspace", Backspace), ("KEY_BACKSPACE", Backspace),
        ];
        Self { bindings: bindings.iter().map(|&(key, action)| (key.to_string(), action)).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_knows_winit_and_evdev_names() {
        let map = KeyMap::default();
        assert_eq!(map.action("ArrowUp"), Some(KeyAction::Up));
        assert_eq!(map.action("Up"), Some(KeyAction::Up));
        assert_eq!(map.action("KEY_ENTER"), Some(KeyAction::Activate));
        assert_eq!(map.action("Next"), Some(KeyAction::Next));
        assert_eq!(map.action("KeyA"), None);
    }

    #[test]
    fn bind_replaces_and_unbind_removes() {
        let map = KeyMap::default().bind("Next", KeyAction::Right).unbind("Escape");
        assert_eq!(map.action("Next"), Some(KeyAction::Right));
        assert_eq!(map.action("Escape"), None);
        assert_eq!(KeyMap::empty().bind("KEY_F1", KeyAction::Cancel).action("KEY_F1"), Some(KeyAction::Cancel));
    }
}
