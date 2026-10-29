//! Loading a WPT file: references, fuzzy metadata, stylesheets, and whether
//! the test can run without a browser.

use std::{fs, path::{Component, Path, PathBuf}, sync::LazyLock};
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use regex::Regex;
use crate::comparison::Fuzzy;

#[derive(Debug)]
pub enum Problem {
    /// The test needs something this runner deliberately doesn't do (scripts,
    /// animations, ...). It is skipped, not counted.
    Unsupported(String),
    /// The test or a resource is broken or missing.
    Error(String),
}

#[derive(Clone, Debug)]
pub struct Reference {
    /// Path relative to the WPT root.
    pub path: PathBuf,
    pub must_match: bool,
    pub fuzzy: Fuzzy,
}

pub struct Document {
    pub dom: RcDom,
    /// Absolute path.
    pub path: PathBuf,
    source: String,
    pub references: Vec<Reference>,
}

pub fn attr(node: &Handle, key: &str) -> Option<String> {
    if let NodeData::Element { attrs, .. } = &node.data {
        attrs.borrow().iter().find(|a| a.name.local.as_ref() == key).map(|a| a.value.to_string())
    } else {
        None
    }
}

fn walk(node: &Handle, f: &mut impl FnMut(&Handle) -> Result<(), Problem>) -> Result<(), Problem> {
    f(node)?;
    for child in node.children.borrow().iter() {
        walk(child, f)?;
    }
    Ok(())
}

fn read(path: &Path) -> Result<String, Problem> {
    fs::read_to_string(path).map_err(|e| Problem::Error(format!("{}: {e}", path.display())))
}

/// Resolves a URL in `base` (a file) to a path under `root`.
pub fn resolve(base: &Path, href: &str, root: &Path) -> Result<PathBuf, Problem> {
    let href = href.trim();
    if href.is_empty() || href.starts_with("//") || href.contains([':', '?', '#', '\\']) {
        return Err(Problem::Unsupported(format!("unsupported resource URL: {href}")));
    }
    let mut decoded = Vec::new();
    let mut bytes = href.as_bytes().iter().copied();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hi = bytes.next().and_then(|b| (b as char).to_digit(16));
            let lo = bytes.next().and_then(|b| (b as char).to_digit(16));
            decoded.push(match (hi, lo) {
                (Some(hi), Some(lo)) => (hi * 16 + lo) as u8,
                _ => return Err(Problem::Error(format!("invalid URL escape: {href}"))),
            });
        } else {
            decoded.push(byte);
        }
    }
    let decoded = String::from_utf8(decoded).map_err(|_| Problem::Error("non-UTF-8 URL".into()))?;
    let joined = if href.starts_with('/') {
        root.join(decoded.trim_start_matches('/'))
    } else {
        base.parent().unwrap_or(root).join(decoded)
    };
    let mut path = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::ParentDir => { path.pop(); }
            Component::CurDir => {}
            other => path.push(other),
        }
    }
    if !path.starts_with(root) {
        return Err(Problem::Error(format!("resource outside WPT root: {href}")));
    }
    Ok(path)
}

impl Document {
    pub fn load(path: &Path, root: &Path) -> Result<Self, Problem> {
        let source = read(path)?;
        let dom = parse_document(RcDom::default(), Default::default()).one(source.as_str());
        let mut links = Vec::new();
        let mut fuzzy = Vec::new();
        walk(&dom.document, &mut |node| {
            if let NodeData::Element { name, .. } = &node.data {
                if name.local.as_ref() == "link" {
                    for rel in attr(node, "rel").unwrap_or_default().split_ascii_whitespace() {
                        if rel.eq_ignore_ascii_case("match") || rel.eq_ignore_ascii_case("mismatch") {
                            let href = attr(node, "href").ok_or_else(|| Problem::Error("reference without href".into()))?;
                            links.push((resolve(path, &href, root)?, rel.eq_ignore_ascii_case("match")));
                        }
                    }
                }
                if name.local.as_ref() == "meta" && attr(node, "name").as_deref() == Some("fuzzy") {
                    let content = attr(node, "content").unwrap_or_default();
                    // `ref.html:1;2` scopes the tolerance to one reference.
                    let (key, value) = match content.rsplit_once(':') {
                        Some((key, value)) => (Some(resolve(path, key, root)?), value),
                        None => (None, content.as_str()),
                    };
                    if fuzzy.iter().any(|(existing, _)| *existing == key) {
                        return Err(Problem::Error("duplicate fuzzy metadata".into()));
                    }
                    fuzzy.push((key, Fuzzy::parse(value).map_err(Problem::Error)?));
                }
            }
            Ok(())
        })?;
        let default = fuzzy.iter().find(|(key, _)| key.is_none()).map(|(_, f)| *f).unwrap_or_default();
        for (key, _) in &fuzzy {
            if key.as_ref().is_some_and(|key| !links.iter().any(|(path, _)| path == key)) {
                return Err(Problem::Error("fuzzy metadata names an unknown reference".into()));
            }
        }
        let references = links.into_iter().map(|(reference, must_match)| Reference {
            fuzzy: fuzzy.iter().find(|(key, _)| key.as_ref() == Some(&reference)).map_or(default, |(_, f)| *f),
            path: reference.strip_prefix(root).unwrap().to_owned(),
            must_match,
        }).collect();
        Ok(Self { dom, path: path.to_owned(), source, references })
    }

    /// Fails with `Unsupported` if rendering the file needs more than static
    /// HTML and CSS.
    pub fn check_supported(&self) -> Result<(), Problem> {
        static SELF_CLOSING: LazyLock<Regex> = LazyLock::new(|| Regex::new(
            r"(?i)<(?:div|span|p|a|b|i|em|strong|li|ul|ol|td|tr|table|style|script|title|object|iframe|canvas|h[1-6])\b[^<>]*/>"
        ).unwrap());
        if matches!(self.path.extension().and_then(|s| s.to_str()), Some("xht" | "xhtml"))
            && SELF_CLOSING.is_match(&self.source)
        {
            // The HTML parser would leave these elements open.
            return Err(Problem::Unsupported("XHTML self-closing element".into()));
        }
        walk(&self.dom.document, &mut |node| {
            if let NodeData::Element { name, attrs, .. } = &node.data {
                let tag = name.local.as_ref();
                if tag == "script" {
                    return Err(Problem::Unsupported("script".into()));
                }
                if matches!(tag, "iframe" | "object" | "embed" | "video" | "audio" | "canvas" | "svg" | "math" | "base" | "template") {
                    return Err(Problem::Unsupported(format!("<{tag}>")));
                }
                for a in attrs.borrow().iter() {
                    let key = a.name.local.as_ref();
                    if key.starts_with("on") || matches!(key, "srcset" | "autofocus" | "contenteditable" | "popover") {
                        return Err(Problem::Unsupported(format!("{key} attribute")));
                    }
                }
                if attr(node, "class").unwrap_or_default().split_ascii_whitespace()
                    .any(|c| c == "reftest-wait" || c == "test-wait")
                {
                    return Err(Problem::Unsupported("reftest-wait".into()));
                }
                if tag == "meta" {
                    if attr(node, "http-equiv").is_some_and(|s| s.eq_ignore_ascii_case("refresh")) {
                        return Err(Problem::Unsupported("meta refresh".into()));
                    }
                    if attr(node, "name").as_deref() == Some("flags") {
                        let content = attr(node, "content").unwrap_or_default();
                        if let Some(flag) = content.split_ascii_whitespace().find(|s| matches!(*s, "interact" | "animated" | "paged" | "scroll" | "userstyle")) {
                            return Err(Problem::Unsupported(format!("{flag} flag")));
                        }
                    }
                    if matches!(attr(node, "name").as_deref(), Some("reftest-pages" | "viewport")) {
                        return Err(Problem::Unsupported("paged or viewport test".into()));
                    }
                }
            }
            Ok(())
        })
    }

    /// The `style` attributes, in document order.
    pub fn inline_styles(&self) -> Vec<String> {
        let mut styles = Vec::new();
        let _ = walk(&self.dom.document, &mut |node| {
            styles.extend(attr(node, "style"));
            Ok(())
        });
        styles
    }

    /// The document's CSS in cascade order: `<style>` blocks and linked
    /// stylesheets, with `@import`s inlined.
    pub fn stylesheet(&self, root: &Path) -> Result<String, Problem> {
        let mut css = String::new();
        walk(&self.dom.document, &mut |node| {
            if let NodeData::Element { name, .. } = &node.data {
                let tag = name.local.as_ref();
                if let Some(style) = attr(node, "style") {
                    check_css(&style)?;
                }
                let rel = attr(node, "rel").unwrap_or_default().to_ascii_lowercase();
                let is_link = tag == "link" && rel.split_ascii_whitespace().any(|r| r == "stylesheet");
                if tag != "style" && !is_link {
                    return Ok(());
                }
                let media = attr(node, "media").unwrap_or_default();
                if !matches!(media.trim(), "" | "all" | "screen") || rel.contains("alternate") || attr(node, "disabled").is_some() {
                    return Err(Problem::Unsupported("conditional or alternate stylesheet".into()));
                }
                let (text, base) = if is_link {
                    let href = attr(node, "href").ok_or_else(|| Problem::Error("stylesheet without href".into()))?;
                    let path = resolve(&self.path, &href, root)?;
                    (read(&path)?, path)
                } else {
                    let mut text = String::new();
                    for child in node.children.borrow().iter() {
                        if let NodeData::Text { contents } = &child.data { text.push_str(&contents.borrow()); }
                    }
                    (text, self.path.clone())
                };
                css.push_str(&inline_imports(&text, &base, root, 0)?);
                css.push('\n');
            }
            Ok(())
        })?;
        Ok(css)
    }
}

/// Replaces `@import url(x);` with the contents of `x`, resolved against `base`.
fn inline_imports(css: &str, base: &Path, root: &Path, depth: usize) -> Result<String, Problem> {
    static IMPORT: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r#"(?i)@import\s+(?:url\(\s*)?["']?([^"')\s;]+)["']?\s*\)?\s*([^;]*);"#
    ).unwrap());
    // XHTML tests wrap style sheets in CDATA, which the HTML parser keeps as text.
    let css = css.replace("<![CDATA[", "").replace("]]>", "");
    check_css(&css)?;
    let mut out = String::new();
    let mut last = 0;
    for import in IMPORT.captures_iter(&css) {
        if depth > 4 {
            return Err(Problem::Error("@import nested too deeply".into()));
        }
        if !matches!(import[2].trim(), "" | "all" | "screen") {
            return Err(Problem::Unsupported("conditional @import".into()));
        }
        let whole = import.get(0).unwrap();
        out.push_str(&css[last..whole.start()]);
        let path = resolve(base, &import[1], root)?;
        out.push_str(&inline_imports(&read(&path)?, &path, root, depth + 1)?);
        last = whole.end();
    }
    out.push_str(&css[last..]);
    Ok(out)
}

fn check_css(css: &str) -> Result<(), Problem> {
    static COMMENTS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)/\*.*?\*/").unwrap());
    static TIMED: LazyLock<Regex> = LazyLock::new(|| Regex::new(
        r"(?i)@keyframes\b|@page\b|\b(?:animation(?:-[a-z-]+)?|transition(?:-[a-z-]+)?)\s*:"
    ).unwrap());
    if let Some(found) = TIMED.find(&COMMENTS.replace_all(css, "")) {
        return Err(Problem::Unsupported(format!("timed or paged CSS ({})", found.as_str().trim_end_matches(':').trim())));
    }
    Ok(())
}
