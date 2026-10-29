//! `--report`: ranks the failing tests' causes, so engine work goes where
//! it flips the most tests. Xerune itself says what it doesn't support
//! (`xerune::css::apply_declaration`, `xerune_css`'s selector parser), and
//! the comparison says how far a failure is from its reference.

use std::{collections::{BTreeMap, BTreeSet}, fmt::Write, path::{Path, PathBuf}};
use xerune::css::Unsupported;
use xerune_css::selector::{PseudoElement, Selector};
use crate::Status;

/// What the report needs to know about one test.
#[derive(Debug, Default)]
pub struct Detail {
    /// The CSS Xerune leaves out, as keys that group alike declarations:
    /// `cursor`, `display: table-cell`, `width: calc()`, `selector
    /// :first-letter`, `@font-face`.
    pub unsupported: BTreeSet<String>,
    /// Every property the test declares.
    pub properties: BTreeSet<String>,
    /// Pixels that differ from the closest `match` reference.
    pub pixels: Option<u32>,
}

impl Detail {
    /// `css` is the document's stylesheet; `inline` its `style` attributes.
    pub fn of(css: &str, inline: &[String]) -> Self {
        let mut detail = Detail::default();
        let css = strip_comments(css);
        let (preludes, blocks) = split_rules(&css);
        for block in blocks.iter().map(String::as_str).chain(inline.iter().map(String::as_str)) {
            for d in xerune_css::sheet::declarations(block) {
                detail.declaration(&d.name, &d.value);
            }
        }
        for prelude in preludes {
            detail.prelude(prelude.trim());
        }
        for queries in xerune_css::sheet::StyleSheet::parse(&css).media {
            for query in queries.iter().filter(|q| q.unsupported) {
                detail.unsupported.insert(format!("@media {}", normalize(&query.text)));
            }
        }
        detail
    }

    fn declaration(&mut self, name: &str, value: &str) {
        if name.starts_with("--") {
            self.unsupported.insert("custom properties".into());
            return;
        }
        self.properties.insert(name.to_owned());
        // Every family is Ahem here, in Xerune and in Chromium.
        if name == "font-family" {
            return;
        }
        let mut style = xerune::ContainerStyle::default();
        let mut layout = taffy::style::Style::default();
        match xerune::css::apply_declaration(name, value, &mut style, &mut layout) {
            Ok(()) => {}
            Err(Unsupported::Property) => { self.unsupported.insert(name.to_owned()); }
            Err(Unsupported::Value) => {
                let value = normalize(value);
                self.unsupported.insert(if is_css_wide(&value) { value } else { format!("{name}: {value}") });
            }
        }
    }

    /// A rule's selector list, or an at-rule's prelude.
    fn prelude(&mut self, prelude: &str) {
        if let Some(at_rule) = prelude.strip_prefix('@') {
            let name: String = at_rule.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
            // `@font-face` too: every family is Ahem here.
            if !matches!(name.to_ascii_lowercase().as_str(), "media" | "starting-style" | "charset" | "import" | "font-face") {
                self.unsupported.insert(format!("@{}", name.to_ascii_lowercase()));
            }
            return;
        }
        if prelude.is_empty() || xerune_css::selector::parse_selector_list(prelude).is_some_and(|list| {
            list.iter().all(|s| s.pseudo_element() != Some(PseudoElement::Unsupported))
        }) {
            return;
        }
        // Blame the pseudo-classes and -elements Xerune can't use.
        let mut blamed = false;
        for pseudo in pseudos(prelude) {
            let usable = Selector::parse(&format!("*{pseudo}"))
                .is_some_and(|s| s.pseudo_element() != Some(PseudoElement::Unsupported));
            if !usable {
                let name = pseudo.split('(').next().unwrap().to_ascii_lowercase();
                self.unsupported.insert(format!("selector {name}{}", if pseudo.contains('(') { "()" } else { "" }));
                blamed = true;
            }
        }
        if !blamed {
            self.unsupported.insert("selector syntax".into());
        }
    }
}

/// `:name`, `::name` and `:name(...)` in a selector list, outside
/// attribute selectors.
fn pseudos(selectors: &str) -> Vec<&str> {
    let bytes = selectors.as_bytes();
    let (mut out, mut i, mut brackets) = (Vec::new(), 0, 0);
    while i < bytes.len() {
        match bytes[i] {
            b'[' => brackets += 1,
            b']' => brackets -= 1,
            b'\\' => i += 1,
            b':' if brackets == 0 => {
                let start = i;
                i += 1 + usize::from(bytes.get(i + 1) == Some(&b':'));
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-' || bytes[i] == b'_') {
                    i += 1;
                }
                if bytes.get(i) == Some(&b'(') {
                    let mut depth = 0;
                    while i < bytes.len() {
                        depth += i32::from(bytes[i] == b'(') - i32::from(bytes[i] == b')');
                        i += 1;
                        if depth == 0 {
                            break;
                        }
                    }
                }
                out.push(&selectors[start..i]);
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    out
}

fn is_css_wide(value: &str) -> bool {
    matches!(value, "inherit" | "initial" | "unset" | "revert" | "revert-layer")
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out + rest
}

/// The preludes and the declaration blocks of `css`, which has no
/// comments: `@media x { a { b } }` gives `@media x` and `a`, and `b`.
/// Blocks of at-rules (`@font-face { src: .. }`) are left out.
fn split_rules(css: &str) -> (Vec<&str>, Vec<String>) {
    let (mut preludes, mut blocks) = (Vec::new(), Vec::new());
    let mut open: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (i, c) in css.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => {
                // `@import x;` and other statements end before the prelude.
                let prelude = css[start..i].rsplit(';').next().unwrap();
                preludes.push(prelude);
                open.push(prelude);
                start = i + 1;
            }
            (None, '}') => {
                let at_rule = open.pop().is_some_and(|p| p.trim_start().starts_with('@'));
                if !at_rule && !css[start..i].trim().is_empty() {
                    blocks.push(css[start..i].to_owned());
                }
                start = i + 1;
            }
            _ => {}
        }
    }
    (preludes, blocks)
}

/// `value` with what varies between alike declarations replaced: numbers
/// by `<n>` (keeping the unit), function arguments, hex colors and strings.
/// Parentheses that aren't a function's (`@media (width)`) stay.
pub fn normalize(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    let bytes = value.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let starts_number = c.is_ascii_digit()
            || (c == b'.' && bytes.get(i + 1).is_some_and(u8::is_ascii_digit))
            || (matches!(c, b'-' | b'+') && bytes.get(i + 1).is_some_and(|&n| n.is_ascii_digit() || n == b'.'));
        let in_word = i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-');
        if starts_number && !in_word {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            out.push_str("<n>");
        } else if c == b'#' {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                i += 1;
            }
            out.push_str("#hex");
        } else if c == b'"' || c == b'\'' {
            i += 1 + value[i + 1..].find(c as char).map_or(value.len() - i - 1, |end| end + 1);
            out.push_str("\"..\"");
        } else if c == b'(' && i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'-') {
            let mut depth = 0;
            while i < bytes.len() {
                depth += i32::from(bytes[i] == b'(') - i32::from(bytes[i] == b')');
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            out.push_str("()");
        } else if c.is_ascii_whitespace() {
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push(' ');
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out
}

#[derive(Default)]
struct Count {
    fail: usize,
    /// Failing tests in which this is the only unsupported thing.
    alone: usize,
    pass: usize,
}

/// The report, with at most `limit` rows per table.
pub fn render(results: &BTreeMap<PathBuf, Status>, details: &BTreeMap<PathBuf, Detail>, limit: usize) -> String {
    let mut out = String::new();
    let tests: Vec<(&Path, bool, &Detail)> = details.iter().filter_map(|(path, detail)| match results.get(path)? {
        Status::Pass | Status::PassAntialiasing => Some((path.as_path(), true, detail)),
        Status::Fail(_) => Some((path.as_path(), false, detail)),
        Status::Skip(_) => None,
    }).collect();
    let failing = tests.iter().filter(|(_, pass, _)| !pass).count();

    // What Xerune leaves out.
    let mut features: BTreeMap<&str, Count> = BTreeMap::new();
    let mut words: BTreeMap<&str, Count> = BTreeMap::new();
    for (_, pass, detail) in &tests {
        let mut test_words = BTreeSet::new();
        for feature in &detail.unsupported {
            count(features.entry(feature.as_str()), *pass, detail.unsupported.len() == 1);
            // `width: calc()` and `margin: calc()` are both `calc()`.
            let Some((_, value)) = feature.split_once(": ") else { continue };
            test_words.extend(value.split([' ', ',', '/']).filter(|w| {
                (w.starts_with("<n>") && w.len() > 3) || w.ends_with("()")
            }));
        }
        for word in test_words {
            count(words.entry(word), *pass, false);
        }
    }
    let unsupported_failing = tests.iter().filter(|(_, pass, d)| !pass && !d.unsupported.is_empty()).count();
    writeln!(out, "\n{unsupported_failing} of {failing} failing tests use CSS Xerune leaves out.").unwrap();
    writeln!(out, "\nUnsupported CSS, by the failing tests it is the only gap in (alone):").unwrap();
    writeln!(out, "{:>6}{:>6}{:>6}  {}", "alone", "fail", "pass", "property: value, selector or at-rule").unwrap();
    let mut rows: Vec<_> = features.iter().filter(|(_, c)| c.fail > 0).collect();
    rows.sort_by_key(|(name, c)| (std::cmp::Reverse((c.alone, c.fail)), *name));
    for (name, c) in rows.iter().take(limit) {
        writeln!(out, "{:>6}{:>6}{:>6}  {name}", c.alone, c.fail, c.pass).unwrap();
    }
    writeln!(out, "\nUnits and functions in the unsupported values, across properties:").unwrap();
    writeln!(out, "{:>6}{:>6}  {}", "fail", "pass", "unit or function").unwrap();
    let mut rows: Vec<_> = words.iter().filter(|(_, c)| c.fail > 0).collect();
    rows.sort_by_key(|(name, c)| (std::cmp::Reverse(c.fail), *name));
    for (name, c) in rows.iter().take(limit) {
        writeln!(out, "{:>6}{:>6}  {name}", c.fail, c.pass).unwrap();
    }

    // Bugs in what Xerune supports.
    let supported: Vec<_> = tests.iter().filter(|(_, _, d)| d.unsupported.is_empty()).collect();
    let mut properties: BTreeMap<&str, Count> = BTreeMap::new();
    for (_, pass, detail) in &supported {
        for property in &detail.properties {
            count(properties.entry(property.as_str()), *pass, false);
        }
    }
    writeln!(out, "\n{} failing tests use only supported CSS. Their properties, by failing tests:", failing - unsupported_failing).unwrap();
    writeln!(out, "{:>6}{:>6}{:>7}  {}", "fail", "pass", "pass%", "property").unwrap();
    let mut rows: Vec<_> = properties.iter().filter(|(_, c)| c.fail > 0).collect();
    rows.sort_by_key(|(name, c)| (std::cmp::Reverse(c.fail), *name));
    for (name, c) in rows.iter().take(limit) {
        let rate = 100.0 * c.pass as f64 / (c.pass + c.fail) as f64;
        writeln!(out, "{:>6}{:>6}{:>6.0}%  {name}", c.fail, c.pass, rate).unwrap();
    }
    writeln!(out, "\nNear misses: failing tests with only supported CSS, by differing pixels:").unwrap();
    let mut near: Vec<_> = supported.iter().filter(|(_, pass, d)| !pass && d.pixels.is_some()).collect();
    near.sort_by_key(|(path, _, d)| (d.pixels, *path));
    for (path, _, detail) in near.iter().take(limit) {
        writeln!(out, "{:>7}  {}", detail.pixels.unwrap(), path.display()).unwrap();
    }
    out
}

fn count(entry: std::collections::btree_map::Entry<'_, &str, Count>, pass: bool, alone: bool) {
    let c = entry.or_default();
    if pass {
        c.pass += 1;
    } else {
        c.fail += 1;
        c.alone += usize::from(alone);
    }
}
