//! Minimal `@media` query support.
//!
//! The upstream `simplecss` parser silently skips all at-rules, so this module
//! provides two integration paths:
//!
//! - **Compiled templates (hot path)**: `xerune_derive` parses `@media` blocks
//!   at compile time and wraps the declarations inside each block with a
//!   [`matches_query`] guard built from static [`FeatureGroup`]s. The
//!   runtime cost is two atomic reads plus a tiny slice scan.
//! - **Runtime string stylesheets (dynamic path)**: [`expand_for_viewport`]
//!   rewrites a stylesheet, inlining the contents of matching `@media` blocks
//!   (source order preserved) before `simplecss` parsing.
//!
//! Both read queries with `xerune_css::media`, which lists the supported
//! features.

use crate::alloc_prelude::*;
use crate::screen;

pub use xerune_css::media::{parse_feature, parse_query, split_segments, Feature, Group, Query, Segment};

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
    groups.iter().any(|g| g.features.iter().all(|f| f.eval(width, height)) != g.negated)
}

/// Convenience: parse a query string and evaluate it against the current
/// global viewport. Parses on each call — prefer the derive path on rebuild
/// hot paths, or cache the parsed [`Query`] yourself.
pub fn matches(query: &str) -> bool {
    parse_query(query).eval(screen::width(), screen::height())
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
                if parse_query(query).eval(width, height) {
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
