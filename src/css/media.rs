//! Minimal `@media` query support.
//!
//! The stylesheet parser (`xerune_css::sheet`) records which `@media`
//! blocks each rule is in, and both style paths evaluate them when a view is
//! built:
//!
//! - **Compiled templates**: `xerune_derive` wraps the declarations of each
//!   block's rules in a [`matches_query`] guard built from static
//!   [`FeatureGroup`]s. The runtime cost is two atomic reads plus a tiny
//!   slice scan.
//! - **Runtime stylesheets**: the parsed sheet is cached, and each build
//!   evaluates its blocks against the viewport once
//!   (`StyleSheet::media_matches`).
//!
//! `xerune_css::media` lists the supported features.

use crate::screen;

pub use xerune_css::media::{parse_feature, parse_query, Feature, Group, Query};

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
