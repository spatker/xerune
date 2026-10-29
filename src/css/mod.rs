#![allow(missing_docs)]

pub(crate) mod parser;
pub mod declared;
pub(crate) mod animation;
pub(crate) mod gradient;
pub mod media;
/// `transition` and its longhands.
pub mod transition;

pub use gradient::{parse_gradient, interpolate_gradient};
pub use parser::{parse_hex_color, parse_px, parse_dimension, parse_length_percentage, parse_length_percentage_auto, parse_box_shadow};
pub use animation::{Easing, Keyframe, KeyframesAnimation, Property as AnimatedProperty, Side, Value as AnimatedValue, parse_duration_sec, parse_animation_shorthand};
pub use animation::parse_keyframes;

use crate::{Color, ContainerStyle, Display, TextAlign, Direction, WritingMode, FlexDirection, FlexWrap, BoxSizing, style::AnimationIterationCount};
use taffy::prelude::*;
use taffy::style::Style;
use crate::alloc_prelude::*;


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
/// and applies the values directly onto the given styling structures; the
/// `!important` ones are left for [`parse_inline_style_important`].
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

    apply_inline_style(style_str, false, current_style, taffy_style);
}

/// Applies the `!important` declarations of a `style` attribute, which
/// come after the stylesheet's (see [`parse_inline_style`] for the others).
pub fn parse_inline_style_important(style_str: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    if style_str.contains('!') {
        apply_inline_style(style_str, true, current_style, taffy_style);
    }
}

fn apply_inline_style(style_str: &str, important: bool, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    for font_pass in [true, false] {
        for d in xerune_css::sheet::declarations(style_str) {
            if d.important == important && sets_font_size(&d.name) == font_pass {
                let _ = apply_declaration(&d.name, &d.value, current_style, taffy_style);
            }
        }
    }
}

/// Parses an `opacity` value: a number or a percentage, clamped to 0..1
/// as CSS clamps it when used.
pub fn parse_opacity(val: &str) -> Option<f32> {
    let val = val.trim();
    let opacity = match val.strip_suffix('%') {
        Some(percent) => percent.trim().parse::<f32>().ok()? / 100.0,
        None => val.parse::<f32>().ok()?,
    };
    opacity.is_finite().then(|| opacity.clamp(0.0, 1.0))
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
                let _ = apply_declaration(prop, val, current_style, taffy_style);
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

/// Why [`apply_declaration`] left a declaration out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// Xerune doesn't know the property.
    Property,
    /// Xerune knows the property, but not this value (or it is invalid).
    Value,
}

/// Applies a single CSS key-value declaration to the given styling
/// structures. One Xerune doesn't support is logged once and does nothing,
/// or (in shorthands) only its supported parts.
pub fn apply_declaration(prop: &str, val: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) -> Result<(), Unsupported> {
    // `line-height: 1.5em` stays relative: it is resolved where it is used.
    let resolved = if prop != "line-height" {
        // In `font-size` itself, `em` is the parent's font size.
        let em = if sets_font_size(prop) { current_style.parent_font_size } else { current_style.font_size };
        resolve_font_relative_lengths(val, em, current_style.root_font_size)
    } else {
        None
    };
    let result = apply_resolved_declaration(prop, resolved.as_deref().unwrap_or(val), current_style, taffy_style);
    match result {
        Ok(()) => {}
        Err(Unsupported::Property) => crate::diag::warn_once("Unsupported CSS property", prop),
        Err(Unsupported::Value) => crate::diag::warn_once("Unsupported CSS value", &format!("{prop}: {val}")),
    }
    result
}

fn apply_resolved_declaration(prop: &str, val: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) -> Result<(), Unsupported> {
    use Unsupported::{Property, Value};
    if let Some(declared) = declared::Declared::parse(prop, val) {
        declared.apply(current_style, taffy_style);
        return if declared.understood() { Ok(()) } else { Err(Value) };
    }
    match prop {
        "color-scheme" => {
            // Without a user preference to consult, "light dark" means light.
            let mut words = val.split_whitespace();
            current_style.color_scheme_dark = val.contains("dark") && !words.any(|w| w == "light");
        }
        "text-align" => {
            current_style.text_align = Some(match val {
                "left" => TextAlign::Left,
                "center" => TextAlign::Center,
                "right" => TextAlign::Right,
                _ => return Err(Value),
            });
        }
        "appearance" | "-webkit-appearance" | "-moz-appearance" => {
            current_style.appearance_none = val == "none";
        }
        "accent-color" => {
            current_style.accent_color = if val == "auto" { None } else { Some(parse_hex_color(val).ok_or(Value)?) };
        }
        "font-size" => {
            if let Some(size) = parse_px(val) {
                current_style.font_size = size;
            } else if let Some(Ok(p)) = val.strip_suffix('%').map(|p| p.trim().parse::<f32>()) {
                current_style.font_size = current_style.parent_font_size * p / 100.0;
            } else {
                return Err(Value);
            }
        }
        "font" => {
            // The shorthand resets line-height unless it gives one (`14px/1.4`).
            current_style.line_height = crate::style::LineHeight::Normal;
            let mut has_size = false;
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                let subparts: Vec<&str> = part.split('/').collect();
                if let Some(size) = parse_px(subparts[0]) {
                    current_style.font_size = size;
                    has_size = true;
                    if let Some(lh) = subparts.get(1).and_then(|v| parse_line_height(v)) {
                        current_style.line_height = lh;
                    }
                } else if subparts[0] == "bold" {
                    current_style.weight = 1;
                }
            }
            // The size is the one part every `font` gives.
            if !has_size {
                return Err(Value);
            }
        }
        "overflow-wrap" | "word-wrap" => {
            use crate::text::WordBreak;
            current_style.overflow_wrap = match val.trim() {
                "normal" => WordBreak::Normal,
                "break-word" => WordBreak::BreakWord,
                "anywhere" => WordBreak::Anywhere,
                _ => return Err(Value),
            };
        }
        "word-break" => match val.trim() {
            "normal" => (current_style.word_break_all, current_style.word_keep_all) = (false, false),
            "break-all" => (current_style.word_break_all, current_style.word_keep_all) = (true, false),
            "keep-all" => (current_style.word_break_all, current_style.word_keep_all) = (false, true),
            // Deprecated alias of `overflow-wrap: anywhere` with `word-break: normal`.
            "break-word" => {
                (current_style.word_break_all, current_style.word_keep_all) = (false, false);
                current_style.overflow_wrap = crate::text::WordBreak::Anywhere;
            }
            _ => return Err(Value),
        },
        "text-overflow" => match val.trim() {
            "clip" => current_style.text_overflow_ellipsis = false,
            "ellipsis" => current_style.text_overflow_ellipsis = true,
            _ => return Err(Value),
        },
        "white-space" => {
            use crate::style::WhiteSpace;
            current_style.white_space = match val.trim() {
                "normal" => WhiteSpace::Normal,
                "nowrap" => WhiteSpace::Nowrap,
                "pre" => WhiteSpace::Pre,
                "pre-wrap" | "break-spaces" => WhiteSpace::PreWrap,
                "pre-line" => WhiteSpace::PreLine,
                _ => return Err(Value),
            };
        }
        "line-height" => current_style.line_height = parse_line_height(val).ok_or(Value)?,
        "border-width" => {
            let w = parser::parse_sides(val, parser::parse_border_width).ok_or(Value)?;
            let b = &mut current_style.border;
            (b.top.width, b.right.width, b.bottom.width, b.left.width) = (w.top, w.right, w.bottom, w.left);
        }
        "border-style" => {
            let s = parser::parse_sides(val, parser::parse_border_style).ok_or(Value)?;
            let b = &mut current_style.border;
            (b.top.style, b.right.style, b.bottom.style, b.left.style) = (s.top, s.right, s.bottom, s.left);
        }
        "border-color" => {
            let c = parser::parse_sides(val, parser::parse_border_color).ok_or(Value)?;
            let b = &mut current_style.border;
            (b.top.color, b.right.color, b.bottom.color, b.left.color) = (c.top, c.right, c.bottom, c.left);
        }
        _ if prop.starts_with("border-") && border_side(&mut current_style.border, prop).is_some() => {
            // `border-top`, `border-top-width`, ...
            let rest = &prop["border-".len()..];
            let part = rest.split_once('-').map(|(_, part)| part);
            let side = border_side(&mut current_style.border, prop).unwrap();
            match part {
                None => *side = parser::parse_border_side(val).ok_or(Value)?,
                Some("width") => side.width = parser::parse_border_width(val).ok_or(Value)?,
                Some("style") => side.style = parser::parse_border_style(val).ok_or(Value)?,
                Some("color") => side.color = parser::parse_border_color(val).ok_or(Value)?,
                _ => return Err(Property),
            }
        }
        "inline-size" => current_style.inline_size = Some(parse_dimension(val).ok_or(Value)?),
        "block-size" => current_style.block_size = Some(parse_dimension(val).ok_or(Value)?),
        "min-inline-size" => current_style.min_inline_size = Some(parse_dimension(val).ok_or(Value)?),
        "max-inline-size" => current_style.max_inline_size = Some(parse_dimension(val).ok_or(Value)?),
        "min-block-size" => current_style.min_block_size = Some(parse_dimension(val).ok_or(Value)?),
        "max-block-size" => current_style.max_block_size = Some(parse_dimension(val).ok_or(Value)?),
        "flex-flow" => {
            let mut result = Ok(());
            for part in val.split_whitespace() {
                match part {
                    "row" => { taffy_style.flex_direction = FlexDirection::Row; current_style.flex_direction = FlexDirection::Row; }
                    "column" => { taffy_style.flex_direction = FlexDirection::Column; current_style.flex_direction = FlexDirection::Column; }
                    "row-reverse" => { taffy_style.flex_direction = FlexDirection::RowReverse; current_style.flex_direction = FlexDirection::RowReverse; }
                    "column-reverse" => { taffy_style.flex_direction = FlexDirection::ColumnReverse; current_style.flex_direction = FlexDirection::ColumnReverse; }
                    "nowrap" => { taffy_style.flex_wrap = FlexWrap::NoWrap; current_style.flex_wrap = FlexWrap::NoWrap; }
                    "wrap" => { taffy_style.flex_wrap = FlexWrap::Wrap; current_style.flex_wrap = FlexWrap::Wrap; }
                    "wrap-reverse" => { taffy_style.flex_wrap = FlexWrap::WrapReverse; current_style.flex_wrap = FlexWrap::WrapReverse; }
                    _ => result = Err(Value),
                }
            }
            return result;
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
                    } else {
                        return Err(Value);
                    }
                }
                2 => {
                    let g = parts[0].parse::<f32>().map_err(|_| Value)?;
                    taffy_style.flex_grow = g;
                    if let Ok(s) = parts[1].parse::<f32>() {
                        taffy_style.flex_shrink = s;
                        taffy_style.flex_basis = taffy::style::Dimension::percent(0.0);
                    } else if let Some(d) = parse_dimension(parts[1]) {
                        taffy_style.flex_shrink = 1.0;
                        taffy_style.flex_basis = d;
                    } else {
                        return Err(Value);
                    }
                }
                3 => {
                    let mut result = Ok(());
                    match parts[0].parse::<f32>() {
                        Ok(g) => taffy_style.flex_grow = g,
                        Err(_) => result = Err(Value),
                    }
                    match parts[1].parse::<f32>() {
                        Ok(s) => taffy_style.flex_shrink = s,
                        Err(_) => result = Err(Value),
                    }
                    if let Some(d) = parse_dimension(parts[2]) {
                        taffy_style.flex_basis = d;
                    } else if parts[2] == "auto" {
                        taffy_style.flex_basis = taffy::style::Dimension::auto();
                    } else {
                        result = Err(Value);
                    }
                    return result;
                }
                _ => return Err(Value),
            }
        }
        "order" => current_style.order = val.trim().parse::<i32>().map_err(|_| Value)?,
        "float" => match val {
            "left" | "right" => {
                current_style.is_floated = true;
                if current_style.display == Display::Block {
                    current_style.display = Display::InlineBlock;
                }
            }
            "none" => {}
            _ => return Err(Value),
        },
        "box-sizing" => {
            current_style.box_sizing = match val {
                "border-box" => BoxSizing::BorderBox,
                "content-box" => BoxSizing::ContentBox,
                _ => return Err(Value),
            };
        }
        "row-gap" => taffy_style.gap.height = parse_length_percentage(val).ok_or(Value)?,
        "column-gap" => taffy_style.gap.width = parse_length_percentage(val).ok_or(Value)?,
        "direction" => {
            current_style.direction = match val {
                "rtl" => Direction::Rtl,
                "ltr" => Direction::Ltr,
                _ => return Err(Value),
            };
        }
        "writing-mode" => match val {
            "horizontal-tb" => current_style.writing_mode = WritingMode::HorizontalTb,
            _ => return Err(Value),
        },
        "animation-name" => {
            current_style.animation_name = Some(Arc::from(val.trim()));
        }
        "animation-duration" => {
            current_style.animation_duration = parse_duration_sec(val);
        }
        "animation-timing-function" => {
            current_style.animation_timing_function = animation::Easing::parse(val).ok_or(Value)?;
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
            current_style.animation_direction = animation::keyword(val, &animation::DIRECTIONS).ok_or(Value)?;
        }
        "animation-fill-mode" => {
            current_style.animation_fill_mode = animation::keyword(val, &animation::FILL_MODES).ok_or(Value)?;
        }
        "animation-play-state" => {
            current_style.animation_play_state = animation::keyword(val, &animation::PLAY_STATES).ok_or(Value)?;
        }
        "animation" => {
            parse_animation_shorthand(val, current_style);
        }
        p if p.starts_with("transition") => {
            let transitions = current_style.transitions.get_or_insert_with(Default::default);
            if !transition::apply_transition_property(p, val, Arc::make_mut(transitions)) {
                return Err(Property);
            }
        }
        _ => return Err(Property),
    }
    Ok(())
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::resolve_font_relative_lengths;

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
    fn unsupported_properties_and_values_are_reported() {
        use super::Unsupported::{Property, Value};
        let apply = |prop, val| super::apply_declaration(prop, val, &mut crate::ContainerStyle::default(), &mut taffy::style::Style::default());
        assert_eq!(apply("display", "flex"), Ok(()));
        assert_eq!(apply("display", "table-cell"), Err(Value));
        assert_eq!(apply("width", "2em"), Ok(()));
        assert_eq!(apply("position", "fixed"), Err(Value));
        assert_eq!(apply("text-align", "justify"), Err(Value));
        assert_eq!(apply("border-left-width", "thick"), Ok(()));
        assert_eq!(apply("flex-flow", "column wrap"), Ok(()));
        assert_eq!(apply("flex-flow", "column sideways"), Err(Value));
        assert_eq!(apply("cursor", "pointer"), Err(Property));
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
}
