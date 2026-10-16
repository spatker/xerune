//! Pseudo-elements: `::before` / `::after` and the parts of form controls.
//!
//! `simplecss` can't parse `::`, so their rules are rewritten into rules for
//! the element with prefixed declarations (see [`ControlPart`]).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use crate::text::{find_outside_comments, split_top_level, strip_comments};

/// A pseudo-element: `::before` / `::after`, or a part of a form control,
/// styled with the selectors pages use for Chromium and Firefox
/// (`::-webkit-slider-thumb`, `::-moz-range-thumb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlPart {
    /// `<progress>`'s `::-webkit-progress-bar`: the track.
    ProgressBar,
    /// `<progress>`'s `::-webkit-progress-value` / `::-moz-progress-bar`.
    ProgressValue,
    /// A range's `::-webkit-slider-runnable-track` / `::-moz-range-track`.
    SliderTrack,
    /// A range's `::-webkit-slider-thumb` / `::-moz-range-thumb`.
    SliderThumb,
    /// A range's `::-moz-range-progress`: the track up to the thumb.
    SliderFill,
    /// A text input's `::placeholder`.
    Placeholder,
    /// `::before`: a box generated as the element's first child.
    Before,
    /// `::after`: a box generated as the element's last child.
    After,
}

impl ControlPart {
    const PARTS: [(ControlPart, &'static str, &'static [&'static str]); 8] = [
        (ControlPart::ProgressBar, "-xerune-bar-", &["-webkit-progress-bar"]),
        (ControlPart::ProgressValue, "-xerune-value-", &["-webkit-progress-value", "-moz-progress-bar"]),
        (ControlPart::SliderTrack, "-xerune-track-", &["-webkit-slider-runnable-track", "-moz-range-track"]),
        (ControlPart::SliderThumb, "-xerune-thumb-", &["-webkit-slider-thumb", "-moz-range-thumb"]),
        (ControlPart::SliderFill, "-xerune-fill-", &["-moz-range-progress"]),
        (ControlPart::Placeholder, "-xerune-placeholder-", &["placeholder", "-webkit-input-placeholder", "-moz-placeholder"]),
        (ControlPart::Before, "-xerune-before-", &["before"]),
        (ControlPart::After, "-xerune-after-", &["after"]),
    ];

    /// The part a pseudo-element name (without `::`) selects.
    pub fn from_pseudo_element(name: &str) -> Option<Self> {
        Self::PARTS.iter().find(|(_, _, names)| names.iter().any(|n| n.eq_ignore_ascii_case(name))).map(|(part, _, _)| *part)
    }

    /// Part rules reach the control as declarations with this prefix
    /// (`-xerune-thumb-background`), since simplecss can't parse `::`.
    pub fn declaration_prefix(self) -> &'static str {
        Self::PARTS.iter().find(|(part, _, _)| *part == self).map_or("", |(_, prefix, _)| prefix)
    }

    /// Whether this is a part of a form control, not a generated box.
    pub fn of_form_control(self) -> bool {
        !matches!(self, ControlPart::Before | ControlPart::After)
    }

    /// The part and property of a prefixed declaration name.
    pub fn from_declaration(name: &str) -> Option<(Self, &str)> {
        if !name.starts_with("-xerune-") {
            return None;
        }
        Self::PARTS.iter().find_map(|(part, prefix, _)| name.strip_prefix(prefix).map(|prop| (*part, prop)))
    }
}

/// `prelude` with CSS 2's `:before` / `:after` spelled `::before` / `::after`.
fn legacy_pseudo_elements(prelude: String) -> String {
    let mut out = String::with_capacity(prelude.len() + 2);
    let mut rest = prelude.as_str();
    while let Some(pos) = rest.find(':') {
        out.push_str(&rest[..=pos]);
        let after = &rest[pos + 1..];
        let name_end = after.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(after.len());
        let name = &after[..name_end];
        if !out[..out.len() - 1].ends_with(':') && (name.eq_ignore_ascii_case("before") || name.eq_ignore_ascii_case("after")) {
            out.push(':');
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// Rules for `::before` / `::after` and form control parts
/// (`progress::-webkit-progress-value { background: red }`), which simplecss
/// can't parse, become rules for the element with prefixed declarations
/// (`progress { -xerune-value-background: red }`, see
/// [`ControlPart`]). Selectors of other
/// pseudo-elements, or with anything after the part, are dropped. Used for both
/// compiled templates and runtime stylesheets.
pub fn rewrite_part_rules(css: &str) -> Cow<'_, str> {
    if !css.contains("::") && !css.contains(":before") && !css.contains(":after") {
        return Cow::Borrowed(css);
    }
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(open) = find_outside_comments(rest, b"{") {
        // The block's end, past nested blocks.
        let mut depth = 1;
        let mut end = open + 1;
        while depth > 0 {
            match find_outside_comments(&rest[end..], b"{}") {
                Some(offset) => {
                    end += offset;
                    depth = if rest.as_bytes()[end] == b'{' { depth + 1 } else { depth - 1 };
                    end += 1;
                }
                None => {
                    end = rest.len();
                    break;
                }
            }
        }
        let prelude = legacy_pseudo_elements(strip_comments(&rest[..open]));
        if prelude.trim_start().starts_with('@') || !prelude.contains("::") {
            out.push_str(&rest[..end]);
        } else {
            let body = rest[open + 1..end].strip_suffix('}').unwrap_or(&rest[open + 1..end]);
            let mut plain = Vec::new();
            let mut parts: Vec<(ControlPart, Vec<&str>)> = Vec::new();
            for selector in split_top_level(&prelude, b',') {
                let selector = selector.trim();
                let Some((host, pseudo)) = selector.split_once("::") else {
                    plain.push(selector);
                    continue;
                };
                let name_end = pseudo.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or(pseudo.len());
                let (Some(part), true) = (ControlPart::from_pseudo_element(&pseudo[..name_end]), pseudo[name_end..].trim().is_empty()) else {
                    continue;
                };
                let host = if host.trim().is_empty() { "*" } else { host.trim() };
                match parts.iter_mut().find(|(p, _)| *p == part) {
                    Some((_, hosts)) => hosts.push(host),
                    None => parts.push((part, alloc::vec![host])),
                }
            }
            if !plain.is_empty() {
                out.push_str(&plain.join(", "));
                out.push_str(" {");
                out.push_str(body);
                out.push('}');
            }
            for (part, hosts) in parts {
                out.push_str(&hosts.join(", "));
                out.push_str(" {");
                for declaration in split_top_level(body, b';') {
                    if let Some((name, value)) = strip_comments(declaration).split_once(':') {
                        out.push_str(part.declaration_prefix());
                        out.push_str(name.trim());
                        out.push(':');
                        out.push_str(value);
                        out.push(';');
                    }
                }
                out.push('}');
            }
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    Cow::Owned(out)
}
