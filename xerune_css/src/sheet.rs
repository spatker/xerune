//! Stylesheets: style rules and their declarations.
//!
//! `@media` blocks are read, so that each rule knows the queries it is
//! under; other at-rules (`@keyframes` is read separately) and nested
//! rules are skipped.

use alloc::borrow::Cow;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::media::Query;
use crate::selector::{parse_selector_list, Selector};
use crate::text::find_outside_comments;

/// A declaration, `name: value [!important]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Declaration {
    /// The property, ASCII-lowercased (custom properties keep their case).
    pub name: String,
    /// The value as written, without comments and `!important`.
    pub value: String,
    /// Whether the value ended in `!important`.
    pub important: bool,
}

/// A style rule for one selector of a selector list; the rules of a list
/// share their declarations.
#[derive(Debug, Clone)]
pub struct Rule {
    /// What the rule applies to.
    pub selector: Selector,
    /// The declarations, in source order.
    pub declarations: Rc<[Declaration]>,
    /// The `@media` blocks the rule is in, as an index into
    /// [`StyleSheet::media`]; `None` outside any.
    pub media: Option<usize>,
    /// Inside `@starting-style`: the rule only applies to the style an
    /// element starts from when it appears, for transitions.
    pub starting: bool,
}

/// A parsed stylesheet.
#[derive(Debug, Clone, Default)]
pub struct StyleSheet {
    /// The rules in cascade order: by specificity, then source order.
    pub rules: Vec<Rule>,
    /// Whether any declaration is `!important`.
    pub has_important: bool,
    /// Whether any selector uses `:hover`.
    pub uses_hover: bool,
    /// Whether any selector uses `:active`.
    pub uses_active: bool,
    /// The selectors of the rules dropped because they are invalid (or
    /// use something xerune doesn't support), for warnings.
    pub dropped: Vec<String>,
    /// Each `@media` block's queries: its own and those of the blocks it is
    /// nested in, which must all match for its rules to apply.
    pub media: Vec<Vec<Query>>,
}

impl StyleSheet {
    /// Parses `css`. Rules with an invalid selector list, at-rules other
    /// than `@media` and `@starting-style`, and rules without declarations
    /// are dropped.
    pub fn parse(css: &str) -> Self {
        let mut sheet = StyleSheet::default();
        sheet.parse_block(css, &mut Vec::new(), false);
        // Stable, so equal specificities keep source order.
        sheet.rules.sort_by_key(|rule| rule.selector.specificity());
        let rules = &sheet.rules;
        let uses = |state| rules.iter().any(|rule| rule.selector.uses_state(state));
        sheet.uses_hover = uses(crate::selector::State::Hover);
        sheet.uses_active = uses(crate::selector::State::Active);
        sheet.has_important = rules.iter().any(|rule| rule.declarations.iter().any(|d| d.important));
        sheet
    }

    /// Which of [`Self::media`] match a `width` x `height` viewport.
    pub fn media_matches(&self, width: f32, height: f32) -> Vec<bool> {
        self.media.iter().map(|queries| queries.iter().all(|q| q.eval(width, height))).collect()
    }

    /// Reads the rules of `css`, which is inside the `@media` blocks whose
    /// queries are `queries`, and inside `@starting-style` if `starting`.
    fn parse_block(&mut self, css: &str, queries: &mut Vec<Query>, starting: bool) {
        let media = (!queries.is_empty()).then(|| {
            self.media.push(queries.clone());
            self.media.len() - 1
        });
        let mut rest = css;
        loop {
            rest = skip_cdo_cdc(rest);
            if rest.is_empty() {
                break;
            }
            let Some(open) = prelude_end(rest) else {
                break;
            };
            let prelude = crate::text::strip_comments(&rest[..open]);
            let prelude = prelude.trim();
            let at_rule = prelude.strip_prefix('@').is_some_and(crate::selector::starts_ident);
            if rest.as_bytes()[open] == b';' {
                // `@import ...;`; a stray `;` in a selector drops the rule
                // up to the end of its block.
                if !at_rule {
                    let brace = find_outside_comments(rest, b"{").unwrap_or(rest.len());
                    self.dropped.push(String::from(crate::text::strip_comments(&rest[..brace]).trim()));
                }
                rest = if at_rule { &rest[open + 1..] } else { &rest[block_end(rest, open)..] };
                continue;
            }
            let end = block_end(rest, open);
            let body = rest[open + 1..end].strip_suffix('}').unwrap_or(&rest[open + 1..end]);
            if at_rule {
                let name_end = prelude[1..].find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_')).map_or(prelude.len(), |i| i + 1);
                let name = &prelude[1..name_end];
                if name.eq_ignore_ascii_case("media") {
                    queries.push(crate::media::parse_query(&prelude[name_end..]));
                    self.parse_block(body, queries, starting);
                    queries.pop();
                } else if name.eq_ignore_ascii_case("starting-style") {
                    self.parse_block(body, queries, true);
                }
            } else {
                match parse_selector_list(prelude) {
                    Some(selectors) => {
                        let declarations: Rc<[Declaration]> = parse_declarations(body).into();
                        if !declarations.is_empty() {
                            self.rules.extend(selectors.into_iter().map(|selector| Rule { selector, declarations: declarations.clone(), media, starting }));
                        }
                    }
                    None => self.dropped.push(String::from(prelude)),
                }
            }
            rest = &rest[end..];
        }
    }
}

/// The offset of the `{` or `;` that ends the prelude at the start of
/// `css`. A `{` inside `[ ]` or `( )` is part of the prelude.
fn prelude_end(css: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut at = 0;
    loop {
        at += find_outside_comments(&css[at..], b"{;[]()")?;
        match css.as_bytes()[at] {
            b'[' | b'(' => depth += 1,
            b']' | b')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => return Some(at),
            _ => {}
        }
        at += 1;
    }
}

/// `css` past whitespace and the `<!--` / `-->` an HTML `<style>` may hold.
fn skip_cdo_cdc(mut css: &str) -> &str {
    loop {
        let trimmed = css.trim_start();
        css = match trimmed.strip_prefix("<!--").or_else(|| trimmed.strip_prefix("-->")) {
            Some(rest) => rest,
            None => return trimmed,
        };
    }
}

/// The offset just past the block opened at `open` (its `}`, past nested
/// blocks), or the end of `css` when it isn't closed.
fn block_end(css: &str, open: usize) -> usize {
    let mut depth = 0;
    let mut at = open;
    while let Some(offset) = find_outside_comments(&css[at..], b"{}") {
        at += offset;
        if css.as_bytes()[at] == b'{' {
            depth += 1;
        } else {
            depth -= 1;
            if depth == 0 {
                return at + 1;
            }
        }
        at += 1;
    }
    css.len()
}

/// A declaration borrowed from the text it was read from, where it could
/// be (a lowercase name, no comments).
#[derive(Debug, Clone, PartialEq)]
pub struct DeclarationRef<'a> {
    /// The property, ASCII-lowercased (custom properties keep their case).
    pub name: Cow<'a, str>,
    /// The value as written, without comments and `!important`.
    pub value: Cow<'a, str>,
    /// Whether the value ended in `!important`.
    pub important: bool,
}

/// The declarations of a block's contents (or a `style` attribute), in
/// order. Invalid ones are skipped; so are nested rules.
pub fn declarations(block: &str) -> impl Iterator<Item = DeclarationRef<'_>> {
    let mut rest = block;
    core::iter::from_fn(move || loop {
        if rest.trim().is_empty() {
            return None;
        }
        let (text, next) = next_declaration(rest);
        rest = next;
        if let Some(declaration) = parse_declaration(text) {
            return Some(declaration);
        }
    })
}

/// [`declarations`], owned.
pub fn parse_declarations(block: &str) -> Vec<Declaration> {
    declarations(block)
        .map(|d| Declaration { name: d.name.into_owned(), value: d.value.into_owned(), important: d.important })
        .collect()
}

/// The text of the first declaration in `block` and what follows it: up to
/// a top-level `;`, or past a nested `{ }` block (then the text is empty).
fn next_declaration(block: &str) -> (&str, &str) {
    let mut depth = 0usize;
    let mut at = 0;
    while let Some(offset) = find_outside_comments(&block[at..], b";{}()[]") {
        at += offset;
        match block.as_bytes()[at] {
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.saturating_sub(1),
            b';' if depth == 0 => return (&block[..at], &block[at + 1..]),
            b'{' if depth == 0 => return ("", &block[block_end(block, at)..]),
            _ => {}
        }
        at += 1;
    }
    (block, "")
}

/// One `name: value` declaration; `None` when it has no name or value.
fn parse_declaration(text: &str) -> Option<DeclarationRef<'_>> {
    if text.contains("/*") {
        let text = crate::text::strip_comments(text);
        let d = parse_declaration(&text)?;
        return Some(DeclarationRef { name: Cow::Owned(d.name.into_owned()), value: Cow::Owned(d.value.into_owned()), important: d.important });
    }
    let (name, value) = text.split_once(':')?;
    let name = name.trim();
    let valid_name = !name.is_empty()
        && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii());
    if !valid_name {
        return None;
    }
    let mut value = value.trim();
    let mut important = false;
    if let Some(bang) = value.rfind('!') {
        if value[bang + 1..].trim().eq_ignore_ascii_case("important") {
            important = true;
            value = value[..bang].trim_end();
        }
    }
    if value.is_empty() {
        return None;
    }
    let name = if name.starts_with("--") || !name.bytes().any(|b| b.is_ascii_uppercase()) {
        Cow::Borrowed(name)
    } else {
        Cow::Owned(name.to_ascii_lowercase())
    };
    Some(DeclarationRef { name, value: Cow::Borrowed(value), important })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn pairs(declarations: &[Declaration]) -> Vec<(&str, &str, bool)> {
        declarations.iter().map(|d| (d.name.as_str(), d.value.as_str(), d.important)).collect()
    }

    #[test]
    fn declarations_keep_whole_values() {
        let declarations = parse_declarations(
            "font: 14px/1.4 sans; COLOR: red !important; background: url(data:a;b) ; grid-area: 1 / 2; --X: 1; bad; : x; y: ;",
        );
        assert_eq!(pairs(&declarations), [
            ("font", "14px/1.4 sans", false),
            ("color", "red", true),
            ("background", "url(data:a;b)", false),
            ("grid-area", "1 / 2", false),
            ("--X", "1", false),
        ]);
        assert_eq!(pairs(&parse_declarations("a: 'x;y' /* ; */; b: 1 ! IMPORTANT")), [("a", "'x;y'", false), ("b", "1", true)]);
    }

    #[test]
    fn nested_rules_and_other_at_rules_are_skipped() {
        let sheet = StyleSheet::parse(
            "@import 'x.css'; <!-- p { a: 1; & b { c: 2 } d: 3 } --> @supports (x: y) { q { e: 4 } } /* r { } */ s { f: 5 }",
        );
        let rules: Vec<_> = sheet.rules.iter().map(|r| pairs(&r.declarations)).collect();
        assert_eq!(rules, vec![vec![("a", "1", false), ("d", "3", false)], vec![("f", "5", false)]]);
    }

    #[test]
    fn starting_style_blocks_mark_their_rules() {
        let sheet = StyleSheet::parse("a { x: 1 } @starting-style { b { x: 2 } @media (min-width: 1px) { c { x: 3 } } } d { x: 4 }");
        let starting: Vec<_> = sheet.rules.iter().map(|r| (r.declarations[0].value.as_str(), r.starting, r.media.is_some())).collect();
        assert_eq!(starting, [("1", false, false), ("2", true, false), ("3", true, true), ("4", false, false)]);
    }

    #[test]
    fn media_blocks_condition_their_rules() {
        let sheet = StyleSheet::parse(
            "a { x: 1 } /* @media (min-width: 1px) { z { x: 0 } } */ @MEDIA (min-width: 500px) { b { x: 2 } @media (orientation: portrait) { c { x: 3 } } d { x: 4 } } e { x: 5 }",
        );
        let media: Vec<_> = sheet.rules.iter().map(|r| (r.declarations[0].value.as_str(), r.media)).collect();
        assert_eq!(media, [("1", None), ("2", Some(0)), ("3", Some(1)), ("4", Some(0)), ("5", None)]);
        assert_eq!(sheet.media[1].len(), 2, "nested blocks keep both queries");
        assert_eq!(sheet.media_matches(800.0, 480.0), [true, false]);
        assert_eq!(sheet.media_matches(600.0, 900.0), [true, true]);
        assert_eq!(sheet.media_matches(400.0, 900.0), [false, false]);
    }

    #[test]
    fn rules_are_in_cascade_order() {
        let sheet = StyleSheet::parse("#a { x: 1 } .b, c { x: 2 } d { x: 3 } e {} f::before { x: 4 }");
        let order: Vec<_> = sheet.rules.iter().map(|r| (r.declarations[0].value.as_str(), r.selector.specificity())).collect();
        assert_eq!(order, [("2", 1), ("3", 1), ("4", 2), ("2", 1 << 10), ("1", 1 << 20)]);
    }

    #[test]
    fn invalid_selector_lists_drop_the_rule() {
        // Not at-rules: `@ x` and `@1x` are part of the selector. A `{` in
        // `[ ]` belongs to the selector too.
        let sheet = StyleSheet::parse("a, b:bogus { x: 1 } c; d { x: 2 } @ i; g { x: 5 } @1i; h { x: 6 } [ i { x: 7 } ] j { x: 8 } e { x: 3 } f { x: 4");
        let values: Vec<_> = sheet.rules.iter().map(|r| r.declarations[0].value.as_str()).collect();
        assert_eq!(values, ["3", "4"]);
        assert_eq!(sheet.dropped, ["a, b:bogus", "c; d", "@ i; g", "@1i; h", "[ i { x: 7 } ] j"]);
    }
}
