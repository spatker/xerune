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
//! The app's catalogs are a [`Translations`], kept where the model reaches
//! them (a `static`, or a field). The language in use is the model's own
//! state, a tag like any other field, and templates look strings up through
//! a model method:
//!
//! ```ignore
//! fn t<'a>(&'a self, key: &'a str) -> &'a str {
//!     STRINGS.t(&self.lang, key)
//! }
//! ```
//!
//! `{{ self.t("add_task") }}`. Switching the language is an ordinary update
//! that sets the field.

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

/// The catalogs of an app. The first is the fallback, for languages
/// without a catalog and keys a catalog lacks.
#[derive(Debug, Clone)]
pub struct Translations {
    catalogs: Vec<Catalog>,
}

impl Translations {
    /// Translations with `fallback` as the only language.
    pub fn new(fallback: Catalog) -> Self {
        Self { catalogs: vec![fallback] }
    }

    /// Adds another language.
    pub fn with(mut self, catalog: Catalog) -> Self {
        self.catalogs.push(catalog);
        self
    }

    /// The available languages, the fallback first.
    pub fn langs(&self) -> impl Iterator<Item = &str> {
        self.catalogs.iter().map(Catalog::lang)
    }

    /// The catalog for `lang`, or for the language it is a variant of (`de`
    /// for `de-AT`); tags are compared ignoring case. `None` when there is
    /// none.
    pub fn catalog(&self, lang: &str) -> Option<&Catalog> {
        let primary = |tag: &str| tag.split('-').next().unwrap_or("").to_ascii_lowercase();
        self.catalogs.iter().find(|c| c.lang.eq_ignore_ascii_case(lang))
            .or_else(|| self.catalogs.iter().find(|c| primary(&c.lang) == primary(lang)))
    }

    /// The text for `key` in `lang` (see [`Self::catalog`]), else in the
    /// fallback, else `key` itself (so a missing string shows up on screen).
    pub fn t<'a>(&'a self, lang: &str, key: &'a str) -> &'a str {
        if let Some(text) = self.catalog(lang).and_then(|c| c.get(key)) {
            return text;
        }
        if self.catalog(lang).is_some() {
            log::warn!("no `{key}` in the {lang} strings");
        }
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
    fn looks_up_the_language() {
        let tr = translations();
        assert_eq!(tr.t("en", "add"), "Add task");
        assert_eq!(tr.t("de", "add"), "Aufgabe hinzufügen");
        assert_eq!(tr.t("DE-at", "add"), "Aufgabe hinzufügen", "a regional variant takes the language");
        assert_eq!(tr.catalog("de-AT").map(Catalog::lang), Some("de"));
    }

    #[test]
    fn falls_back_to_the_first_language_then_the_key() {
        let tr = translations();
        assert_eq!(tr.t("de", "open"), "open");
        assert_eq!(tr.t("fr", "add"), "Add task");
        assert_eq!(tr.t("", "add"), "Add task");
        assert_eq!(tr.t("de", "missing"), "missing");
        assert!(tr.catalog("fr").is_none());
        assert_eq!(tr.langs().collect::<Vec<_>>(), ["en", "de"]);
    }

    #[test]
    fn rejects_a_line_without_equals() {
        assert_eq!(Catalog::parse("de", "a = b\noops").unwrap_err(), "de: line 2: expected `key = value`");
    }
}
