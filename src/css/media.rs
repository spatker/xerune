//! Minimal `@media` query support.
//!
//! The upstream `simplecss` parser silently skips all at-rules, so this module
//! provides two integration paths:
//!
//! - **Compiled templates (hot path)**: `xerune_derive` parses `@media` blocks
//!   at compile time and wraps the declarations inside each block with a
//!   [`matches_features`] guard built from static [`Feature`] slices. The
//!   runtime cost is two atomic reads plus a tiny slice scan.
//! - **Runtime string stylesheets (dynamic path)**: [`expand_for_viewport`]
//!   rewrites a stylesheet, inlining the contents of matching `@media` blocks
//!   (source order preserved) before `simplecss` parsing.
//!
//! Supported features (embedded subset): `min-width`, `max-width`,
//! `min-height`, `max-height` (+ `device-*` aliases), `orientation`.
//! Media types: `all`/`screen` pass, `print` never matches, `not` negates.
//! Unknown or malformed features never match, per the CSS spec.

use crate::alloc_prelude::*;
use crate::screen;

/// A single parsed media feature condition, e.g. `(min-width: 600px)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Feature {
    /// `(min-width: N)` — viewport width >= N.
    MinWidth(f32),
    /// `(max-width: N)` — viewport width <= N.
    MaxWidth(f32),
    /// `(min-height: N)` — viewport height >= N.
    MinHeight(f32),
    /// `(max-height: N)` — viewport height <= N.
    MaxHeight(f32),
    /// `(orientation: landscape)` — width > height.
    Landscape,
    /// `(orientation: portrait)` — width <= height.
    Portrait,
}

impl Feature {
    fn eval(&self, width: f32, height: f32) -> bool {
        match *self {
            Feature::MinWidth(v) => width >= v,
            Feature::MaxWidth(v) => width <= v,
            Feature::MinHeight(v) => height >= v,
            Feature::MaxHeight(v) => height <= v,
            Feature::Landscape => width > height,
            Feature::Portrait => width <= height,
        }
    }
}

/// One `and`-joined condition list, optionally negated (`not (min-width: X)`).
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// True when the group came from a `not` prefix.
    pub negated: bool,
    /// AND-ed features. An empty list means the group only carries a media type.
    pub features: Vec<Feature>,
}

/// A parsed media query: comma-separated (OR) groups.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// OR-ed groups. An empty `groups` list never matches.
    pub groups: Vec<Group>,
}

impl Query {
    /// Returns true when the query matches the given viewport size.
    pub fn eval(&self, width: f32, height: f32) -> bool {
        self.groups.iter().any(|g| {
            let inner = g.features.iter().all(|f| f.eval(width, height));
            if g.negated { !inner } else { inner }
        })
    }
}

/// Evaluates OR-ed groups of AND-ed features against the current global
/// viewport. This is the entry point the derive macro emits for rules that
/// were declared inside `@media` blocks.
pub fn matches_features(groups: &[&[Feature]]) -> bool {
    let (width, height) = screen::size();
    groups
        .iter()
        .any(|group| group.iter().all(|f| f.eval(width, height)))
}

/// A const-constructible, optionally negated group of AND-ed features.
/// The derive macro emits whole media queries as `'static` literals of this
/// type, so matching never allocates or parses at runtime.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FeatureGroup {
    /// True when this comma-group carried a `not` prefix.
    pub negated: bool,
    /// AND-ed features; an empty slice means "media type only" (always matches).
    pub features: &'static [Feature],
}

/// Evaluates static [`FeatureGroup`]s against the current global viewport.
/// Emitted by the derive macro for rules declared inside `@media` blocks.
pub fn matches_query(groups: &[FeatureGroup]) -> bool {
    let (width, height) = screen::size();
    groups.iter().any(|g| {
        let inner = g.features.iter().all(|f| f.eval(width, height));
        if g.negated { !inner } else { inner }
    })
}

/// Convenience: parse a query string and evaluate it against the current
/// global viewport. Parses on each call — prefer the derive path on rebuild
/// hot paths, or cache the parsed [`Query`] yourself.
pub fn matches(query: &str) -> bool {
    match parse_query(query) {
        Some(q) => q.eval(screen::width(), screen::height()),
        None => false,
    }
}

/// Parses a media query text such as `screen and (min-width: 600px), (orientation: landscape)`.
/// Returns `None` when the query is malformed or uses unsupported syntax
/// (per spec, an unparseable query never matches).
pub fn parse_query(text: &str) -> Option<Query> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }

    let mut groups = Vec::new();
    for part in split_top_level(text, ',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        let (negated, body) = match strip_prefix_ci(part, "not") {
            Some(rest) if matches!(rest.chars().next(), Some(' ') | Some('(') | Some('\t')) => {
                (true, rest.trim_start())
            }
            _ => (false, part),
        };

        let mut features = Vec::new();

        // Split into `and`-joined conditions; the first may be a bare media type.
        for cond in split_and(body) {
            let cond = cond.trim();
            if cond.is_empty() {
                return None;
            }
            if let Some(inner) = cond.strip_prefix('(') {
                let inner = inner.strip_suffix(')')?;
                features.push(parse_feature(inner)?);
            } else {
                let lower = cond.to_ascii_lowercase();
                match lower.as_str() {
                    "all" | "screen" => {}
                    "print" => {
                        // Unsatisfiable group, but other comma groups still apply.
                        features.push(Feature::MinWidth(f32::INFINITY));
                    }
                    _ => return None, // unknown media type
                }
            }
        }

        groups.push(Group { negated, features });
    }

    if groups.is_empty() {
        None
    } else {
        Some(Query { groups })
    }
}

/// Parses the inside of a `(...)` condition. Returns `None` for unknown
/// features (`prefers-color-scheme`, `aspect-ratio`, …) — they never match.
pub fn parse_feature(inner: &str) -> Option<Feature> {
    let (name, value) = inner.split_once(':')?;
    let name = name.trim().to_ascii_lowercase();
    let value = value.trim();

    match name.as_str() {
        "orientation" => match value.to_ascii_lowercase().as_str() {
            "landscape" => Some(Feature::Landscape),
            "portrait" => Some(Feature::Portrait),
            _ => None,
        },
        "min-width" | "device-min-width" => Some(Feature::MinWidth(parse_px_value(value)?)),
        "max-width" | "device-max-width" => Some(Feature::MaxWidth(parse_px_value(value)?)),
        "min-height" | "device-min-height" => Some(Feature::MinHeight(parse_px_value(value)?)),
        "max-height" | "device-max-height" => Some(Feature::MaxHeight(parse_px_value(value)?)),
        _ => None,
    }
}

fn parse_px_value(val: &str) -> Option<f32> {
    if let Some(stripped) = val.strip_suffix("px") {
        stripped.trim().parse::<f32>().ok()
    } else if val.ends_with('%') || val.ends_with("em") || val.ends_with("rem") {
        None
    } else {
        val.parse::<f32>().ok()
    }
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes()) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// Splits on a top-level separator character, ignoring separators nested
/// inside parentheses/brackets.
fn split_top_level(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

/// Splits a query body on top-level ` and ` separators (case-insensitive).
fn split_and(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b'a' | b'A' if depth == 0 && is_and_keyword(bytes, i) => {
                parts.push(&s[start..i]);
                i += 4; // "and "
                start = i;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    parts.push(&s[start..]);
    parts
}

/// Detects `and ` at byte position `i`, case-insensitive, preceded by whitespace.
fn is_and_keyword(bytes: &[u8], i: usize) -> bool {
    const AND: &[u8] = b"and ";
    if i + AND.len() > bytes.len() {
        return false;
    }
    if !bytes[i..i + AND.len()].eq_ignore_ascii_case(AND) {
        return false;
    }
    i == 0 || matches!(bytes[i - 1], b' ' | b'\t' | b'\n' | b'\r')
}

/// A segment of a stylesheet: plain rules or a `@media { ... }` block.
#[derive(Debug)]
pub enum Segment<'a> {
    /// Ordinary CSS text (other at-rules like `@keyframes` stay untouched).
    Base(&'a str),
    /// `@media <query> { <inner> }`.
    Media {
        /// Raw query text, e.g. `screen and (min-width: 600px)`.
        query: &'a str,
        /// Block body (may itself contain nested `@media` blocks).
        inner: &'a str,
    },
}

/// Splits stylesheet text into base/media segments at the top level.
/// `/* ... */` comments are skipped so a `@media` inside a comment is ignored.
/// The `Base` segments preserve comments; `simplecss` strips them downstream.
pub fn split_segments(css: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let bytes = css.as_bytes();
    let mut base_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        // Skip comments.
        if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = css[i + 2..].find("*/") {
                i += 2 + end + 2;
            } else {
                i = bytes.len();
            }
            continue;
        }

        if bytes[i] == b'@' && is_media_at_rule(css, bytes, i) {
            if i > base_start {
                let seg = &css[base_start..i];
                if !seg.trim().is_empty() {
                    out.push(Segment::Base(seg));
                }
            }

            let open = match css[i..].find('{') {
                Some(off) => i + off,
                None => break, // unterminated — leave the rest as base
            };
            let query = css[i + 6..open].trim();

            let close = match find_matching_brace(css, open) {
                Some(pos) => pos,
                None => break,
            };
            let inner = &css[open + 1..close];

            out.push(Segment::Media { query, inner });

            i = close + 1;
            base_start = i;
            continue;
        }
        i += 1;
    }

    if base_start < css.len() {
        let seg = &css[base_start..];
        if !seg.trim().is_empty() {
            out.push(Segment::Base(seg));
        }
    }
    out
}

/// True when bytes starting at `i` are `@media` followed by a separator.
fn is_media_at_rule(_css: &str, bytes: &[u8], i: usize) -> bool {
    const NAME: &[u8] = b"media";
    if i + 1 + NAME.len() > bytes.len() {
        return false;
    }
    if !bytes[i + 1..i + 1 + NAME.len()].eq_ignore_ascii_case(NAME) {
        return false;
    }
    // `@media` must be followed by whitespace or directly by the block/query.
    match bytes.get(i + 1 + NAME.len()) {
        None => true,
        Some(&c) => matches!(c, b' ' | b'\t' | b'\n' | b'\r' | b'{' | b'('),
    }
}

fn find_matching_brace(css: &str, open: usize) -> Option<usize> {
    let bytes = css.as_bytes();
    let mut depth = 0usize;
    let mut i = open;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Rewrites a stylesheet keeping only `@media` blocks whose query matches the
/// current global viewport, inlining their contents in source order.
/// Used by the runtime (dynamic-string) style path.
pub fn expand_for_viewport(css: &str) -> String {
    let (width, height) = screen::size();
    let mut out = String::with_capacity(css.len());
    expand_into(css, width, height, &mut out);
    out
}

fn expand_into(css: &str, width: f32, height: f32, out: &mut String) {
    for seg in split_segments(css) {
        match seg {
            Segment::Base(text) => out.push_str(text),
            Segment::Media { query, inner } => {
                let matched = match parse_query(query) {
                    Some(q) => q.eval(width, height),
                    None => false,
                };
                if matched {
                    // Nested @media inside a matched block still gets expanded.
                    expand_into(inner, width, height, out);
                    out.push('\n');
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_queries() {
        let q = parse_query("(min-width: 600px)").unwrap();
        assert!(q.eval(800.0, 400.0));
        assert!(!q.eval(599.0, 400.0));

        let q = parse_query("screen and (min-width: 600px) and (max-width: 1024px)").unwrap();
        assert!(q.eval(600.0, 400.0));
        assert!(q.eval(1024.0, 400.0));
        assert!(!q.eval(1025.0, 400.0));

        let q = parse_query("(min-width: 100px), (min-width: 500px)").unwrap();
        assert!(q.eval(120.0, 0.0));
        assert!(q.eval(600.0, 0.0));
        assert!(!q.eval(50.0, 0.0));
    }

    #[test]
    fn orientation_types_and_negation() {
        let q = parse_query("(orientation: landscape)").unwrap();
        assert!(q.eval(400.0, 200.0));
        assert!(!q.eval(200.0, 400.0));

        let q = parse_query("all").unwrap();
        assert!(q.eval(0.0, 0.0));

        let q = parse_query("print").unwrap();
        assert!(!q.eval(800.0, 600.0));

        let q = parse_query("not (min-width: 600px)").unwrap();
        assert!(q.eval(500.0, 400.0));
        assert!(!q.eval(600.0, 400.0));
    }

    #[test]
    fn unsupported_features_never_match() {
        assert!(parse_query("(prefers-color-scheme: dark)").is_none());
        assert!(parse_query("(min-width: 10px").is_none());
        assert!(parse_query("").is_none());
    }

    // NOTE: `expand_for_viewport` and anything reading `crate::screen` must be
    // exercised from a single test — the viewport is process-global and cargo
    // runs tests on parallel threads.
    #[test]
    fn splits_and_expands() {
        let css = "a { color: #ff0000; }\n@media (min-width: 100px) { a { color: #00ff00; } }\nb { color: #0000ff; }";

        crate::screen::set_viewport(200.0, 200.0);
        let expanded = expand_for_viewport(css);
        assert!(expanded.contains("#00ff00"));
        assert!(expanded.contains("#0000ff"));

        crate::screen::set_viewport(50.0, 50.0);
        let expanded = expand_for_viewport(css);
        assert!(!expanded.contains("#00ff00"));
        assert!(expanded.contains("#ff0000"));

        // Source order preserved for base rules around blocks.
        let red = expanded.find("#ff0000").unwrap();
        let blue = expanded.find("#0000ff").unwrap();
        assert!(red < blue);

        // A @media inside a comment must never be treated as a block:
        // the color appears once (inside the preserved comment text only).
        let commented = "/* @media (min-width: 1px) { z { color: #010101; } } */ x { color: #020202; }";
        let segs = split_segments(commented);
        assert!(matches!(&segs[..], [Segment::Base(_)]));
        let expanded = expand_for_viewport(commented);
        assert_eq!(expanded.matches("#010101").count(), 1);
    }
}
