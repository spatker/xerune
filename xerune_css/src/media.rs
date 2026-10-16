//! `@media` queries: splitting a stylesheet into its `@media` blocks and
//! parsing their queries.
//!
//! Supported features (embedded subset): `min-width`, `max-width`,
//! `min-height`, `max-height` (+ `device-*` aliases), `orientation`.
//! Media types: `all`/`screen` match, other types (`print`) don't, `not`
//! negates. As in CSS, a comma-separated part with an unknown feature or bad
//! syntax never matches, while the other parts still apply, and an empty
//! query matches everything.

use alloc::string::String;
use alloc::vec::Vec;

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

/// A condition no viewport meets: what a part that can't match becomes.
const NEVER: Feature = Feature::MinWidth(f32::INFINITY);

impl Feature {
    /// Whether a `width` x `height` viewport meets this condition.
    pub fn eval(&self, width: f32, height: f32) -> bool {
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

impl Group {
    /// Whether a `width` x `height` viewport matches this group.
    pub fn eval(&self, width: f32, height: f32) -> bool {
        self.features.iter().all(|f| f.eval(width, height)) != self.negated
    }
}

/// A parsed media query: comma-separated (OR) groups.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    /// OR-ed groups. An empty `groups` list never matches.
    pub groups: Vec<Group>,
    /// Some part used an unknown feature, media type or bad syntax, so it
    /// never matches.
    pub unsupported: bool,
    /// The query as written, for messages.
    pub text: String,
}

impl Query {
    /// Returns true when the query matches the given viewport size.
    pub fn eval(&self, width: f32, height: f32) -> bool {
        self.groups.iter().any(|g| g.eval(width, height))
    }
}

/// Parses a media query text such as `screen and (min-width: 600px), (orientation: landscape)`.
pub fn parse_query(text: &str) -> Query {
    let text = text.trim();
    let mut query = Query { groups: Vec::new(), unsupported: false, text: String::from(text) };
    if text.is_empty() {
        query.groups.push(Group { negated: false, features: Vec::new() });
        return query;
    }
    for part in split_top_level(text, ',') {
        let part = part.trim();
        let (negated, body) = match strip_prefix_ci(part, "not") {
            Some(rest) if matches!(rest.chars().next(), Some(' ' | '(' | '\t')) => (true, rest.trim_start()),
            _ => (false, part),
        };
        match parse_group(body) {
            Some(features) => query.groups.push(Group { negated, features }),
            None => {
                // Bad syntax or an unknown feature: this part is `not all`.
                query.unsupported = true;
                query.groups.push(Group { negated: false, features: alloc::vec![NEVER] });
            }
        }
    }
    query
}

/// The features of one comma-separated part (without `not`); `None` if it
/// can't be parsed or uses an unknown feature.
fn parse_group(body: &str) -> Option<Vec<Feature>> {
    let mut features = Vec::new();
    // `and`-joined conditions; the first may be a bare media type.
    for cond in split_and(body) {
        let cond = cond.trim();
        if let Some(inner) = cond.strip_prefix('(') {
            features.push(parse_feature(inner.strip_suffix(')')?)?);
        } else if cond.eq_ignore_ascii_case("all") || cond.eq_ignore_ascii_case("screen") {
        } else if cond.is_empty() || !cond.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
            return None;
        } else {
            // Another media type (`print`, `tv`): valid, but not this device.
            features.push(NEVER);
        }
    }
    Some(features)
}

/// Parses the inside of a `(...)` condition. Returns `None` for unknown
/// features (`prefers-color-scheme`, `aspect-ratio`, …) — they never match.
pub fn parse_feature(inner: &str) -> Option<Feature> {
    let (name, value) = inner.split_once(':')?;
    let name = name.trim();
    let value = value.trim();
    let is = |n: &str| name.eq_ignore_ascii_case(n);
    if is("orientation") {
        return if value.eq_ignore_ascii_case("landscape") {
            Some(Feature::Landscape)
        } else if value.eq_ignore_ascii_case("portrait") {
            Some(Feature::Portrait)
        } else {
            None
        };
    }
    let px = parse_px_value(value);
    if is("min-width") || is("device-min-width") {
        Some(Feature::MinWidth(px?))
    } else if is("max-width") || is("device-max-width") {
        Some(Feature::MaxWidth(px?))
    } else if is("min-height") || is("device-min-height") {
        Some(Feature::MinHeight(px?))
    } else if is("max-height") || is("device-max-height") {
        Some(Feature::MaxHeight(px?))
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_queries() {
        let q = parse_query("(min-width: 600px)");
        assert!(q.eval(800.0, 400.0));
        assert!(!q.eval(599.0, 400.0));

        let q = parse_query("screen and (min-width: 600px) and (max-width: 1024px)");
        assert!(q.eval(600.0, 400.0));
        assert!(q.eval(1024.0, 400.0));
        assert!(!q.eval(1025.0, 400.0));

        let q = parse_query("(min-width: 100px), (min-width: 500px)");
        assert!(q.eval(120.0, 0.0));
        assert!(q.eval(600.0, 0.0));
        assert!(!q.eval(50.0, 0.0));
    }

    #[test]
    fn orientation_types_and_negation() {
        let q = parse_query("(orientation: landscape)");
        assert!(q.eval(400.0, 200.0));
        assert!(!q.eval(200.0, 400.0));

        assert!(parse_query("all").eval(0.0, 0.0));
        assert!(!parse_query("print").eval(800.0, 600.0));
        assert!(parse_query("not print").eval(800.0, 600.0));
        assert!(parse_query("NOT print").eval(800.0, 600.0));

        let q = parse_query("not (min-width: 600px)");
        assert!(q.eval(500.0, 400.0));
        assert!(!q.eval(600.0, 400.0));
    }

    #[test]
    fn unsupported_parts_never_match() {
        for text in ["(prefers-color-scheme: dark)", "(min-width: 10px", "(min-width: 10px) and", "not (hover: hover)"] {
            let q = parse_query(text);
            assert!(q.unsupported && !q.eval(800.0, 600.0), "{text}");
        }
        // The other parts of the list still apply.
        let q = parse_query("(hover: hover), (min-width: 100px)");
        assert!(q.unsupported && q.eval(800.0, 600.0));
        // An empty query matches everything.
        assert!(parse_query("").eval(800.0, 600.0));
    }
}
