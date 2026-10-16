//! Translated strings: one [`Catalog`] per language, looked up by key.
//!
//! A catalog is a text file of `key = value` lines (`#` starts a comment),
//! usually embedded with `include_str!`:
//!
//! ```text
//! # de.txt
//! add_task = Aufgabe hinzufügen
//! ```
//!
//! The model owns a [`Translations`] and templates call it through a model
//! method, `{{ self.t("add_task") }}`. Switching the language is an ordinary
//! update: [`Translations::set_lang`], then the view is rebuilt.

use crate::alloc_prelude::*;

/// The strings of one language, by key.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    lang: String,
    entries: HashMap<String, String>,
}

impl Catalog {
    /// Reads the `key = value` lines of `source` as language `lang` (a tag
    /// like `de` or `en-US`). Fails on a line without `=`, naming it.
    pub fn parse(lang: &str, source: &str) -> Result<Self, String> {
        let mut entries = HashMap::new();
        for (i, line) in source.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("{lang}: line {}: expected `key = value`", i + 1))?;
            entries.insert(key.trim().to_string(), value.trim().to_string());
        }
        Ok(Self { lang: lang.to_string(), entries })
    }

    /// The language tag this catalog was read as.
    pub fn lang(&self) -> &str {
        &self.lang
    }

    /// The text for `key`, if this catalog has it.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }
}

/// The catalogs of an app and the language in use. The first catalog is the
/// fallback for keys another language lacks.
#[derive(Debug, Clone)]
pub struct Translations {
    catalogs: Vec<Catalog>,
    current: usize,
}

impl Translations {
    /// Translations with `fallback` as the only, current language.
    pub fn new(fallback: Catalog) -> Self {
        Self { catalogs: vec![fallback], current: 0 }
    }

    /// Adds another language.
    pub fn with(mut self, catalog: Catalog) -> Self {
        self.catalogs.push(catalog);
        self
    }

    /// Switches to `lang`. Returns `false`, keeping the current language,
    /// when there is no catalog for it.
    pub fn set_lang(&mut self, lang: &str) -> bool {
        match self.catalogs.iter().position(|c| c.lang == lang) {
            Some(i) => {
                self.current = i;
                true
            }
            None => false,
        }
    }

    /// The language in use.
    pub fn lang(&self) -> &str {
        &self.catalogs[self.current].lang
    }

    /// The available languages, the fallback first.
    pub fn langs(&self) -> impl Iterator<Item = &str> {
        self.catalogs.iter().map(Catalog::lang)
    }

    /// The text for `key` in the current language, else in the fallback,
    /// else `key` itself (so a missing string shows up on screen).
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(text) = self.catalogs[self.current].get(key) {
            return text;
        }
        log::warn!("no `{key}` in the {} strings", self.lang());
        self.catalogs[0].get(key).unwrap_or(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translations() -> Translations {
        let en = Catalog::parse("en", "# English\nadd = Add task\nopen = open\n").unwrap();
        let de = Catalog::parse("de", "add = Aufgabe hinzufügen\n\n").unwrap();
        Translations::new(en).with(de)
    }

    #[test]
    fn looks_up_the_current_language() {
        let mut tr = translations();
        assert_eq!(tr.t("add"), "Add task");
        assert!(tr.set_lang("de"));
        assert_eq!(tr.lang(), "de");
        assert_eq!(tr.t("add"), "Aufgabe hinzufügen");
    }

    #[test]
    fn falls_back_to_the_first_language_then_the_key() {
        let mut tr = translations();
        tr.set_lang("de");
        assert_eq!(tr.t("open"), "open");
        assert_eq!(tr.t("missing"), "missing");
    }

    #[test]
    fn keeps_the_language_for_an_unknown_tag() {
        let mut tr = translations();
        assert!(!tr.set_lang("fr"));
        assert_eq!(tr.lang(), "en");
        assert_eq!(tr.langs().collect::<Vec<_>>(), ["en", "de"]);
    }

    #[test]
    fn rejects_a_line_without_equals() {
        assert_eq!(Catalog::parse("de", "a = b\noops").unwrap_err(), "de: line 2: expected `key = value`");
    }
}
