#![allow(missing_docs)]

pub(crate) mod parser;
pub(crate) mod animation;
pub(crate) mod gradient;
pub mod media;

pub use gradient::{parse_gradient, interpolate_gradient};
pub use parser::{parse_hex_color, parse_px, parse_dimension, parse_length_percentage, parse_length_percentage_auto, parse_box_shadow};
use parser::{parse_padding, parse_margin};
pub use animation::{Keyframe, KeyframesAnimation, parse_duration_sec, parse_animation_shorthand};
pub use animation::parse_keyframes;

use crate::{Color, ContainerStyle, Display, TextAlign, Direction, WritingMode, FlexDirection, FlexWrap, AlignContent, AlignItems, CssJustifyContent, BoxSizing, style::AnimationIterationCount};
use taffy::prelude::*;
use taffy::style::Style;
use crate::alloc_prelude::*;

/// Stands in for `/` in values that pass through `simplecss` (U+2044 FRACTION
/// SLASH, which never occurs in CSS).
const SLASH: char = '\u{2044}';

/// Rewrites a stylesheet into something `simplecss` can match against the
/// classes the DOM builder adds: `:nth-child(N)` becomes `.nth-child-N` and
/// `:last-child` becomes `.last-child`. `simplecss` cuts a value off at `/`
/// and loses the rest of the rule, so the `/` of `/<number>` (the line-height
/// of a `font` shorthand) becomes [`SLASH`].
#[cfg(feature = "std")]
pub(crate) fn rewrite_for_simplecss(css: &str) -> String {
    const NTH: &str = ":nth-child(";
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(pos) = rest.find(NTH) {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + NTH.len()..];
        let inner = after.trim_start();
        let digits = inner.len() - inner.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let tail = inner[digits..].trim_start();
        if digits > 0 && tail.starts_with(')') {
            out.push_str(".nth-child-");
            out.push_str(&inner[..digits]);
            rest = &tail[1..];
        } else {
            out.push_str(NTH);
            rest = after;
        }
    }
    out.push_str(rest);
    let css = out.replace(":last-child", ".last-child");

    let mut out = String::with_capacity(css.len());
    let mut rest = css.as_str();
    while let Some(pos) = rest.find('/') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos + 1..];
        let num = after.len() - after.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.').len();
        out.push(if num == 0 { '/' } else { SLASH });
        rest = after;
    }
    out.push_str(rest);
    out
}

struct FastLayoutStyle {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    bg_color: Color,
}

fn parse_layout_style_fast(s: &str) -> Option<FastLayoutStyle> {
    let bytes = s.as_bytes();
    let mut i = 0;
    
        if !s[i..].starts_with("left:") { return None; }
    i += 5;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_left = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let left = core::str::from_utf8(&bytes[start_left..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("top:") { return None; }
    i += 4;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_top = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let top = core::str::from_utf8(&bytes[start_top..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("width:") { return None; }
    i += 6;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_width = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let width = core::str::from_utf8(&bytes[start_width..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("height:") { return None; }
    i += 7;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_height = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let height = core::str::from_utf8(&bytes[start_height..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("background-color:") { return None; }
    i += 17;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_color = i;
    while i < bytes.len() && bytes[i] != b';' { i += 1; }
    let color_str = core::str::from_utf8(&bytes[start_color..i]).ok()?.trim();
    let bg_color = parser::parse_hex_color(color_str)?;
    
    Some(FastLayoutStyle {
        left,
        top,
        width,
        height,
        bg_color,
    })
}

/// Parses a CSS inline style string (e.g., "color: #ff0000; border-width: 2px;")
/// and applies the values directly onto the given styling structures.
pub fn parse_inline_style(style_str: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    if style_str.starts_with("left:") {
        if let Some(parsed) = parse_layout_style_fast(style_str) {
            taffy_style.inset.left = LengthPercentageAuto::length(parsed.left);
            taffy_style.inset.top = LengthPercentageAuto::length(parsed.top);
            taffy_style.size.width = Dimension::length(parsed.width);
            taffy_style.size.height = Dimension::length(parsed.height);
            current_style.background_color = Some(parsed.bg_color);
            return;
        }
    }

    for font_pass in [true, false] {
        for decl in style_str.split(';') {
            if let Some((key, val)) = decl.split_once(':') {
                let (key, val) = (key.trim(), val.trim());
                if !key.is_empty() && !val.is_empty() && sets_font_size(key) == font_pass {
                    apply_declaration(key, val, current_style, taffy_style);
                }
            }
        }
    }
}

/// Parses a `line-height` value. Negative values are invalid.
fn parse_line_height(val: &str) -> Option<crate::style::LineHeight> {
    use crate::style::LineHeight;
    let val = val.trim();
    let lh = if val == "normal" {
        LineHeight::Normal
    } else if let Ok(n) = val.parse::<f32>() {
        LineHeight::Number(n)
    } else if let Some(p) = val.strip_suffix('%') {
        LineHeight::Percent(p.trim().parse::<f32>().ok()? / 100.0)
    } else if let Some(em) = val.strip_suffix("em").filter(|v| !v.ends_with('r')) {
        LineHeight::Percent(em.trim().parse().ok()?)
    } else {
        LineHeight::Length(parse_px(val)?)
    };
    match lh {
        LineHeight::Number(v) | LineHeight::Length(v) | LineHeight::Percent(v) if v < 0.0 => None,
        lh => Some(lh),
    }
}

/// Applies a list of `(property, value)` declarations in cascade order.
pub fn apply_declarations(declarations: &[(&str, &str)], current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    for font_pass in [true, false] {
        for (prop, val) in declarations {
            if sets_font_size(prop) == font_pass {
                apply_declaration(prop, val, current_style, taffy_style);
            }
        }
    }
}

/// Whether `prop` can change the font size. Those declarations are applied
/// first, because `em` lengths in the others depend on the result.
pub fn sets_font_size(prop: &str) -> bool {
    matches!(prop, "font-size" | "font")
}

/// Rewrites `em` and `rem` lengths in `val` as `px`, or returns `None` if
/// there are none.
fn resolve_font_relative_lengths(val: &str, em: f32, rem: f32) -> Option<String> {
    if !val.contains("em") {
        return None;
    }
    let bytes = val.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.';
    let mut out = String::new();
    let (mut last, mut from) = (0, 0);
    while let Some(pos) = val[from..].find("em") {
        let unit_end = from + pos + 2;
        from = unit_end;
        let is_rem = from - 2 > 0 && bytes[from - 3] == b'r';
        let unit_start = if is_rem { unit_end - 3 } else { unit_end - 2 };
        let mut start = unit_start;
        while start > 0 && (bytes[start - 1].is_ascii_digit() || bytes[start - 1] == b'.') {
            start -= 1;
        }
        if start > 0 && matches!(bytes[start - 1], b'-' | b'+') {
            start -= 1;
        }
        let bounded = (start == 0 || !is_word(bytes[start - 1])) && (unit_end == bytes.len() || !is_word(bytes[unit_end]));
        let Ok(n) = val[start..unit_start].parse::<f32>() else { continue };
        if bounded {
            out.push_str(&val[last..start]);
            out.push_str(&format!("{}px", n * if is_rem { rem } else { em }));
            last = unit_end;
        }
    }
    (last > 0).then(|| out + &val[last..])
}

/// The side a `border-<side>[-<part>]` property names.
fn border_side<'a>(border: &'a mut crate::style::Borders, prop: &str) -> Option<&'a mut crate::style::BorderSide> {
    let rest = prop.strip_prefix("border-")?;
    let (side, part) = rest.split_once('-').map_or((rest, None), |(s, p)| (s, Some(p)));
    if !matches!(part, None | Some("width" | "style" | "color")) {
        return None;
    }
    Some(match side {
        "top" => &mut border.top,
        "right" => &mut border.right,
        "bottom" => &mut border.bottom,
        "left" => &mut border.left,
        _ => return None,
    })
}

/// Applies a single CSS key-value declaration to the given styling structures.
pub fn apply_declaration(prop: &str, val: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    // `line-height: 1.5em` stays relative: it is resolved where it is used.
    if prop != "line-height" {
        // In `font-size` itself, `em` is the parent's font size.
        let em = if sets_font_size(prop) { current_style.parent_font_size } else { current_style.font_size };
        if let Some(val) = resolve_font_relative_lengths(val, em, current_style.root_font_size) {
            return apply_resolved_declaration(prop, &val, current_style, taffy_style);
        }
    }
    apply_resolved_declaration(prop, val, current_style, taffy_style)
}

fn apply_resolved_declaration(prop: &str, val: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    match prop {
        "display" => {
            match val {
                "block" => current_style.display = Display::Block,
                "inline-block" | "inline" => current_style.display = Display::InlineBlock,
                "flex" => current_style.display = Display::Flex,
                "inline-flex" => current_style.display = Display::InlineFlex,
                "none" => current_style.display = Display::None,
                _ => {}
            }
        }
        "color-scheme" => {
            // Without a user preference to consult, "light dark" means light.
            let mut words = val.split_whitespace();
            current_style.color_scheme_dark = val.contains("dark") && !words.any(|w| w == "light");
        }
        "text-align" => {
            match val {
                "left" => current_style.text_align = Some(TextAlign::Left),
                "center" => current_style.text_align = Some(TextAlign::Center),
                "right" => current_style.text_align = Some(TextAlign::Right),
                _ => {}
            }
        }
        "color" => {
            if let Some(c) = parse_hex_color(val) {
                current_style.color = c;
            }
        }
        "accent-color" => {
            if val == "auto" {
                current_style.accent_color = None;
            } else if let Some(c) = parse_hex_color(val) {
                current_style.accent_color = Some(c);
            }
        }
        "background-color" => {
            if let Some(c) = parse_hex_color(val) {
                current_style.background_color = Some(c);
                current_style.background_gradient = None;
            }
        }
        "background" | "background-image" => {
            if val.contains("gradient(") {
                if let Some(grad) = gradient::parse_gradient(val) {
                    current_style.background_gradient = Some(grad);
                    current_style.background_color = None;
                }
            } else if let Some(c) = parse_hex_color(val) {
                current_style.background_color = Some(c);
                current_style.background_gradient = None;
            }
        }
        "font-size" => {
            if let Some(size) = parse_px(val) {
                current_style.font_size = size;
            } else if let Some(Ok(p)) = val.strip_suffix('%').map(|p| p.trim().parse::<f32>()) {
                current_style.font_size = current_style.parent_font_size * p / 100.0;
            }
        }
        "font-weight" => {
            if val == "bold" || val == "700" || val == "800" || val == "900" {
                current_style.weight = 1;
            } else {
                current_style.weight = 0;
            }
        }
        "font" => {
            // The shorthand resets line-height unless it gives one (`14px/1.4`).
            current_style.line_height = crate::style::LineHeight::Normal;
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                let subparts: Vec<&str> = part.split(['/', SLASH]).collect();
                if let Some(size) = parse_px(subparts[0]) {
                    current_style.font_size = size;
                    if let Some(lh) = subparts.get(1).and_then(|v| parse_line_height(v)) {
                        current_style.line_height = lh;
                    }
                } else if subparts[0] == "bold" {
                    current_style.weight = 1;
                }
            }
        }
        "overflow-wrap" | "word-wrap" => {
            use crate::text::WordBreak;
            current_style.overflow_wrap = match val.trim() {
                "normal" => WordBreak::Normal,
                "break-word" => WordBreak::BreakWord,
                "anywhere" => WordBreak::Anywhere,
                _ => return,
            };
        }
        "word-break" => match val.trim() {
            "normal" | "keep-all" => current_style.word_break_all = false,
            "break-all" => current_style.word_break_all = true,
            // Deprecated alias of `overflow-wrap: anywhere` with `word-break: normal`.
            "break-word" => {
                current_style.word_break_all = false;
                current_style.overflow_wrap = crate::text::WordBreak::Anywhere;
            }
            _ => {}
        },
        "text-overflow" => match val.trim() {
            "clip" => current_style.text_overflow_ellipsis = false,
            "ellipsis" => current_style.text_overflow_ellipsis = true,
            _ => {}
        },
        "white-space" => {
            use crate::style::WhiteSpace;
            current_style.white_space = match val.trim() {
                "normal" => WhiteSpace::Normal,
                "nowrap" => WhiteSpace::Nowrap,
                "pre" => WhiteSpace::Pre,
                "pre-wrap" | "break-spaces" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => return,
            };
        }
        "line-height" => {
            if let Some(lh) = parse_line_height(val) {
                current_style.line_height = lh;
            }
        }
        "box-shadow" => {
            if let Some(bs) = parser::parse_box_shadow(val) {
                current_style.box_shadow = Some(bs);
            } else if val.trim() == "none" {
                current_style.box_shadow = None;
            }
        }
        "border-radius" => {
            if let Some(r) = parse_px(val) {
                current_style.border_radius = r;
            } else if val.ends_with("%") {
                 if val.trim() == "50%" {
                     current_style.border_radius = 9999.0;
                 }
            }
        }
        "border" => {
            if let Some(side) = parser::parse_border_side(val) {
                current_style.border = crate::style::Borders::all(side);
            }
        }
        "border-width" => {
            if let Some(w) = parser::parse_sides(val, parser::parse_border_width) {
                let b = &mut current_style.border;
                (b.top.width, b.right.width, b.bottom.width, b.left.width) = (w.top, w.right, w.bottom, w.left);
            }
        }
        "border-style" => {
            if let Some(s) = parser::parse_sides(val, parser::parse_border_style) {
                let b = &mut current_style.border;
                (b.top.style, b.right.style, b.bottom.style, b.left.style) = (s.top, s.right, s.bottom, s.left);
            }
        }
        "border-color" => {
            if let Some(c) = parser::parse_sides(val, parser::parse_border_color) {
                let b = &mut current_style.border;
                (b.top.color, b.right.color, b.bottom.color, b.left.color) = (c.top, c.right, c.bottom, c.left);
            }
        }
        _ if prop.starts_with("border-") && border_side(&mut current_style.border, prop).is_some() => {
            // `border-top`, `border-top-width`, ...
            let rest = &prop["border-".len()..];
            let part = rest.split_once('-').map(|(_, part)| part);
            let side = border_side(&mut current_style.border, prop).unwrap();
            match part {
                None => if let Some(s) = parser::parse_border_side(val) { *side = s },
                Some("width") => if let Some(w) = parser::parse_border_width(val) { side.width = w },
                Some("style") => if let Some(s) = parser::parse_border_style(val) { side.style = s },
                Some("color") => if let Some(c) = parser::parse_border_color(val) { side.color = c },
                _ => {}
            }
        }
        "padding" => {
            if let Some(p) = parse_padding(val) {
                taffy_style.padding = p;
            }
            if let Some(p) = parser::parse_sides(val, parse_px) {
                current_style.padding_top = p.top;
                current_style.padding_right = p.right;
                current_style.padding_bottom = p.bottom;
                current_style.padding_left = p.left;
            }
        }
        "padding-left" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.left = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_left = w;
            }
        }
        "padding-right" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.right = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_right = w;
            }
        }
        "padding-top" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.top = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_top = w;
            }
        }
        "padding-bottom" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.bottom = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_bottom = w;
            }
        }
        "margin" => {
            if let Some(m) = parse_margin(val) {
                taffy_style.margin = m;
            }
        }
        "margin-left" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.left = m;
            }
        }
        "margin-right" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.right = m;
            }
        }
        "margin-top" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.top = m;
            }
        }
        "margin-bottom" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.bottom = m;
            }
        }
        "width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.size.width = d;
            }
            if let Some(w) = parse_px(val) {
                current_style.width = Some(w);
            }
        }
        "height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.size.height = d;
            }
            if let Some(w) = parse_px(val) {
                current_style.height = Some(w);
            }
        }
        "min-width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.min_size.width = d;
            }
        }
        "min-height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.min_size.height = d;
            }
        }
        "max-width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.max_size.width = d;
            }
        }
        "max-height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.max_size.height = d;
            }
        }
        "inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.inline_size = Some(d);
            }
        }
        "block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.block_size = Some(d);
            }
        }
        "min-inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.min_inline_size = Some(d);
            }
        }
        "max-inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.max_inline_size = Some(d);
            }
        }
        "min-block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.min_block_size = Some(d);
            }
        }
        "max-block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.max_block_size = Some(d);
            }
        }
         "flex-direction" => {
            match val {
                "row" => { taffy_style.flex_direction = FlexDirection::Row; current_style.flex_direction = FlexDirection::Row; }
                "column" => { taffy_style.flex_direction = FlexDirection::Column; current_style.flex_direction = FlexDirection::Column; }
                "row-reverse" => { taffy_style.flex_direction = FlexDirection::RowReverse; current_style.flex_direction = FlexDirection::RowReverse; }
                "column-reverse" => { taffy_style.flex_direction = FlexDirection::ColumnReverse; current_style.flex_direction = FlexDirection::ColumnReverse; }
                _ => {}
            }
        }
        "flex-wrap" => {
            match val {
                "nowrap" => { taffy_style.flex_wrap = FlexWrap::NoWrap; current_style.flex_wrap = FlexWrap::NoWrap; }
                "wrap" => { taffy_style.flex_wrap = FlexWrap::Wrap; current_style.flex_wrap = FlexWrap::Wrap; }
                "wrap-reverse" => { taffy_style.flex_wrap = FlexWrap::WrapReverse; current_style.flex_wrap = FlexWrap::WrapReverse; }
                _ => {}
            }
        }
        "flex-flow" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                match part {
                    "row" => { taffy_style.flex_direction = FlexDirection::Row; current_style.flex_direction = FlexDirection::Row; }
                    "column" => { taffy_style.flex_direction = FlexDirection::Column; current_style.flex_direction = FlexDirection::Column; }
                    "row-reverse" => { taffy_style.flex_direction = FlexDirection::RowReverse; current_style.flex_direction = FlexDirection::RowReverse; }
                    "column-reverse" => { taffy_style.flex_direction = FlexDirection::ColumnReverse; current_style.flex_direction = FlexDirection::ColumnReverse; }
                    "nowrap" => { taffy_style.flex_wrap = FlexWrap::NoWrap; current_style.flex_wrap = FlexWrap::NoWrap; }
                    "wrap" => { taffy_style.flex_wrap = FlexWrap::Wrap; current_style.flex_wrap = FlexWrap::Wrap; }
                    "wrap-reverse" => { taffy_style.flex_wrap = FlexWrap::WrapReverse; current_style.flex_wrap = FlexWrap::WrapReverse; }
                    _ => {}
                }
            }
        }
        "justify-content" => {
             match val {
                "flex-start" => {
                    taffy_style.justify_content = Some(AlignContent::FlexStart);
                    current_style.justify_content = Some(CssJustifyContent::FlexStart);
                }
                "flex-end" => {
                    taffy_style.justify_content = Some(AlignContent::FlexEnd);
                    current_style.justify_content = Some(CssJustifyContent::FlexEnd);
                }
                "center" => {
                    taffy_style.justify_content = Some(AlignContent::Center);
                    current_style.justify_content = Some(CssJustifyContent::Center);
                }
                "space-between" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceBetween);
                    current_style.justify_content = Some(CssJustifyContent::SpaceBetween);
                }
                "space-around" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceAround);
                    current_style.justify_content = Some(CssJustifyContent::SpaceAround);
                }
                "space-evenly" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceEvenly);
                    current_style.justify_content = Some(CssJustifyContent::SpaceEvenly);
                }
                "start" => {
                    taffy_style.justify_content = Some(AlignContent::Start);
                    current_style.justify_content = Some(CssJustifyContent::Start);
                }
                "end" => {
                    taffy_style.justify_content = Some(AlignContent::End);
                    current_style.justify_content = Some(CssJustifyContent::End);
                }
                "left" => {
                    taffy_style.justify_content = Some(AlignContent::Start);
                    current_style.justify_content = Some(CssJustifyContent::Left);
                }
                "right" => {
                    taffy_style.justify_content = Some(AlignContent::End);
                    current_style.justify_content = Some(CssJustifyContent::Right);
                }
                _ => {}
            }
        }
         "align-items" => {
               match val {
                  "flex-start" => taffy_style.align_items = Some(AlignItems::FlexStart),
                  "flex-end" => taffy_style.align_items = Some(AlignItems::FlexEnd),
                  "center" => taffy_style.align_items = Some(AlignItems::Center),
                  "baseline" => taffy_style.align_items = Some(AlignItems::Baseline),
                  "stretch" => taffy_style.align_items = Some(AlignItems::Stretch),
                  "start" | "left" => taffy_style.align_items = Some(AlignItems::Start),
                  "end" | "right" => taffy_style.align_items = Some(AlignItems::End),
                  _ => {}
              }
              current_style.align_items = taffy_style.align_items;
          }
          "align-self" => {
               match val {
                  "flex-start" => taffy_style.align_self = Some(AlignSelf::FlexStart),
                  "flex-end" => taffy_style.align_self = Some(AlignSelf::FlexEnd),
                  "center" => taffy_style.align_self = Some(AlignSelf::Center),
                  "baseline" => taffy_style.align_self = Some(AlignSelf::Baseline),
                  "stretch" => taffy_style.align_self = Some(AlignSelf::Stretch),
                  "start" | "left" => taffy_style.align_self = Some(AlignSelf::Start),
                  "end" | "right" => taffy_style.align_self = Some(AlignSelf::End),
                  _ => {}
              }
              current_style.align_self = taffy_style.align_self;
          }
          "align-content" => {
               match val {
                  "flex-start" => taffy_style.align_content = Some(AlignContent::FlexStart),
                  "flex-end" => taffy_style.align_content = Some(AlignContent::FlexEnd),
                  "center" => taffy_style.align_content = Some(AlignContent::Center),
                  "space-between" => taffy_style.align_content = Some(AlignContent::SpaceBetween),
                  "space-around" => taffy_style.align_content = Some(AlignContent::SpaceAround),
                  "space-evenly" => taffy_style.align_content = Some(AlignContent::SpaceEvenly),
                  "stretch" => taffy_style.align_content = Some(AlignContent::Stretch),
                  "start" | "left" => taffy_style.align_content = Some(AlignContent::Start),
                  "end" | "right" => taffy_style.align_content = Some(AlignContent::End),
                  _ => {}
              }
          }
          "flex-grow" => {
               if let Ok(f) = val.parse::<f32>() {
                   taffy_style.flex_grow = f;
               }
          }
          "flex-shrink" => {
               if let Ok(f) = val.parse::<f32>() {
                   taffy_style.flex_shrink = f;
               }
          }
          "flex-basis" => {
              if let Some(d) = parse_dimension(val) {
                  taffy_style.flex_basis = d;
              } else if val == "auto" {
                  taffy_style.flex_basis = taffy::style::Dimension::auto();
              }
          }
          "flex" => {
              let parts: Vec<&str> = val.split_whitespace().collect();
              match parts.len() {
                  1 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = taffy::style::Dimension::percent(0.0);
                      } else if parts[0] == "auto" {
                          taffy_style.flex_grow = 1.0;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      } else if parts[0] == "none" {
                          taffy_style.flex_grow = 0.0;
                          taffy_style.flex_shrink = 0.0;
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      } else if let Some(d) = parse_dimension(parts[0]) {
                          taffy_style.flex_grow = 1.0;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = d;
                      }
                  }
                  2 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                          if let Ok(s) = parts[1].parse::<f32>() {
                              taffy_style.flex_shrink = s;
                              taffy_style.flex_basis = taffy::style::Dimension::percent(0.0);
                          } else if let Some(d) = parse_dimension(parts[1]) {
                              taffy_style.flex_shrink = 1.0;
                              taffy_style.flex_basis = d;
                          }
                      }
                  }
                  3 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                      }
                      if let Ok(s) = parts[1].parse::<f32>() {
                          taffy_style.flex_shrink = s;
                      }
                      if let Some(d) = parse_dimension(parts[2]) {
                          taffy_style.flex_basis = d;
                      } else if parts[2] == "auto" {
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      }
                  }
                  _ => {}
              }
          }
         "z-index" => {
             if val == "auto" {
                 current_style.z_index = None;
             } else if let Ok(z) = val.parse::<i32>() {
                 current_style.z_index = Some(z);
             }
         }
         "overflow" => {
             match val {
                 // `clip` also forbids programmatic scrolling, which Xerune has none of.
                 "hidden" | "clip" => current_style.overflow = crate::Overflow::Hidden,
                 "scroll" => current_style.overflow = crate::Overflow::Scroll,
                 "auto" => current_style.overflow = crate::Overflow::Scroll,
                 "visible" => current_style.overflow = crate::Overflow::Visible,
                 _ => {}
             }
         }
         "order" => {
             if let Ok(v) = val.trim().parse::<i32>() {
                 current_style.order = v;
             }
         }
         "position" => {
             match val {
                 "absolute" => {
                     taffy_style.position = Position::Absolute;
                     current_style.position = crate::Position::Absolute;
                 }
                 "relative" => {
                     taffy_style.position = Position::Relative;
                     current_style.position = crate::Position::Relative;
                 }
                 "static" => {
                     taffy_style.position = Position::Relative;
                     current_style.position = crate::Position::Static;
                 }
                 _ => {}
             }
         }
          "left" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.left = v;
             }
         }
         "right" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.right = v;
             }
         }
         "top" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.top = v;
             }
         }
         "bottom" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.bottom = v;
             }
         }
         "float" => {
             if val == "left" || val == "right" {
                 current_style.is_floated = true;
                 if current_style.display == Display::Block {
                     current_style.display = Display::InlineBlock;
                 }
             }
         }
         "box-sizing" => {
             match val {
                 "border-box" => current_style.box_sizing = BoxSizing::BorderBox,
                 "content-box" => current_style.box_sizing = BoxSizing::ContentBox,
                 _ => {}
             }
         }
         "row-gap" => {
             if let Some(lp) = parse_length_percentage(val) {
                 taffy_style.gap.height = lp;
             }
         }
         "column-gap" => {
             if let Some(lp) = parse_length_percentage(val) {
                 taffy_style.gap.width = lp;
             }
         }
         "gap" => {
             let parts: Vec<&str> = val.split_whitespace().collect();
             match parts.len() {
                 1 => {
                     if let Some(lp) = parse_length_percentage(parts[0]) {
                         taffy_style.gap.width = lp;
                         taffy_style.gap.height = lp;
                     }
                 }
                 2 => {
                     if let Some(lp_y) = parse_length_percentage(parts[0]) {
                         taffy_style.gap.height = lp_y;
                     }
                     if let Some(lp_x) = parse_length_percentage(parts[1]) {
                         taffy_style.gap.width = lp_x;
                     }
                 }
                 _ => {}
             }
         }
         "direction" => {
             match val {
                 "rtl" => current_style.direction = Direction::Rtl,
                 "ltr" => current_style.direction = Direction::Ltr,
                 _ => {}
             }
         }
         "writing-mode" => {
             match val {
                 "horizontal-tb" => current_style.writing_mode = WritingMode::HorizontalTb,
                 _ => {}
             }
         }
         "animation-name" => {
             current_style.animation_name = Some(Arc::from(val.trim()));
         }
         "animation-duration" => {
             current_style.animation_duration = parse_duration_sec(val);
         }
         "animation-timing-function" => {
             current_style.animation_timing_function = Arc::from(val.trim());
         }
         "animation-delay" => {
             current_style.animation_delay = parse_duration_sec(val);
         }
         "animation-iteration-count" => {
             current_style.animation_iteration_count = if val.trim() == "infinite" {
                 AnimationIterationCount::Infinite
             } else {
                 AnimationIterationCount::Count(val.trim().parse::<f32>().unwrap_or(1.0))
             };
         }
         "animation-direction" => {
             current_style.animation_direction = Arc::from(val.trim().to_lowercase());
         }
         "animation-fill-mode" => {
             current_style.animation_fill_mode = Arc::from(val.trim().to_lowercase());
         }
         "animation-play-state" => {
             current_style.animation_play_state = Arc::from(val.trim().to_lowercase());
         }
         "animation" => {
             parse_animation_shorthand(val, current_style);
         }
         _ => {
             crate::diag::warn_once("Unsupported CSS property", prop);
         }
     }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::{resolve_font_relative_lengths, rewrite_for_simplecss};

    #[test]
    fn box_shorthands_take_one_to_four_values() {
        let sides = |v| super::parser::parse_sides(v, super::parse_px).map(|r| [r.top, r.right, r.bottom, r.left]);
        assert_eq!(sides("1px"), Some([1.0; 4]));
        assert_eq!(sides("1px 2px"), Some([1.0, 2.0, 1.0, 2.0]));
        assert_eq!(sides("1px 2px 3px"), Some([1.0, 2.0, 3.0, 2.0]));
        assert_eq!(sides("1px 2px 3px 4px"), Some([1.0, 2.0, 3.0, 4.0]));
        assert_eq!(sides("1px 2px 3px 4px 5px"), None);
        assert_eq!(sides(""), None);
        assert_eq!(sides("1px red"), None);
    }

    #[test]
    fn em_and_rem_lengths_become_px() {
        let resolve = |v| resolve_font_relative_lengths(v, 20.0, 10.0);
        assert_eq!(resolve("1em 0").as_deref(), Some("20px 0"));
        assert_eq!(resolve("-0.5em .5rem 2px").as_deref(), Some("-10px 5px 2px"));
        assert_eq!(resolve("bold 1.5em/1.2 Ahem").as_deref(), Some("bold 30px/1.2 Ahem"));
        assert_eq!(resolve("1px solid red"), None);
        // Not lengths: words and URLs that merely contain "em".
        assert_eq!(resolve("lorem"), None);
        assert_eq!(resolve("url(a2em.png)"), None);
        assert_eq!(resolve("system-ui, 'Emoji'"), None);
    }

    #[test]
    fn rewrite_for_simplecss_rewrites_structural_pseudo_classes() {
        assert_eq!(rewrite_for_simplecss("li:nth-child(3) { }"), "li.nth-child-3 { }");
        assert_eq!(rewrite_for_simplecss("li:nth-child( 12 ),a:last-child{}"), "li.nth-child-12,a.last-child{}");
        // Only plain numbers are supported; anything else is left alone.
        assert_eq!(rewrite_for_simplecss("li:nth-child(2n+1){}"), "li:nth-child(2n+1){}");
        assert_eq!(rewrite_for_simplecss("li:nth-child(){}"), "li:nth-child(){}");
    }

    #[test]
    fn rewrite_for_simplecss_protects_slash_numbers() {
        assert_eq!(rewrite_for_simplecss("font: 16px/1.5 sans"), "font: 16px\u{2044}1.5 sans");
        assert_eq!(rewrite_for_simplecss("a/b /2/3."), "a/b \u{2044}2\u{2044}3.");
        assert_eq!(rewrite_for_simplecss("x/"), "x/");
        // The whole rule survives simplecss.
        let css = rewrite_for_simplecss("p { font: 16px/1.5 sans; color: red; }");
        let sheet = simplecss::StyleSheet::parse(&css);
        assert_eq!(sheet.rules[0].declarations.len(), 2);
    }
}

