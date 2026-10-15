use taffy::prelude::*;
use crate::graphics::Color;
use crate::alloc_prelude::*;
fn parse_named_color(s: &str) -> Option<Color> {
    match s.trim().to_ascii_lowercase().as_str() {
        "transparent" => Some(Color::from_rgba8(0, 0, 0, 0)),
        "white" => Some(Color::from_rgba8(255, 255, 255, 255)),
        "black" => Some(Color::from_rgba8(0, 0, 0, 255)),
        "red" => Some(Color::from_rgba8(255, 0, 0, 255)),
        "green" => Some(Color::from_rgba8(0, 128, 0, 255)),
        "blue" => Some(Color::from_rgba8(0, 0, 255, 255)),
        "yellow" => Some(Color::from_rgba8(255, 255, 0, 255)),
        "orange" => Some(Color::from_rgba8(255, 165, 0, 255)),
        "purple" => Some(Color::from_rgba8(128, 0, 128, 255)),
        "pink" => Some(Color::from_rgba8(255, 192, 203, 255)),
        "gray" | "grey" => Some(Color::from_rgba8(128, 128, 128, 255)),
        "lightgray" | "lightgrey" => Some(Color::from_rgba8(211, 211, 211, 255)),
        "darkgray" | "darkgrey" => Some(Color::from_rgba8(169, 169, 169, 255)),
        "cyan" => Some(Color::from_rgba8(0, 255, 255, 255)),
        "magenta" => Some(Color::from_rgba8(255, 0, 255, 255)),
        "brown" => Some(Color::from_rgba8(165, 42, 42, 255)),
        "silver" => Some(Color::from_rgba8(192, 192, 192, 255)),
        "gold" => Some(Color::from_rgba8(255, 215, 0, 255)),
        "lime" => Some(Color::from_rgba8(0, 255, 0, 255)),
        "navy" => Some(Color::from_rgba8(0, 0, 128, 255)),
        "teal" => Some(Color::from_rgba8(0, 128, 128, 255)),
        "olive" => Some(Color::from_rgba8(128, 128, 0, 255)),
        "maroon" => Some(Color::from_rgba8(128, 0, 0, 255)),
        _ => None,
    }
}

/// Parse a CSS color string (#hex, rgb/rgba, or named CSS color).
pub fn parse_hex_color(val: &str) -> Option<Color> {
    let trimmed = val.trim();
    
    #[cfg(feature = "std")]
    {
        parse_color_fast(trimmed)
            .or_else(|| parse_named_color(trimmed))
            .or_else(|| {
                csscolorparser::parse(trimmed).ok().map(|c| {
                    Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    )
                })
            })
    }

    #[cfg(not(feature = "std"))]
    {
        parse_color_fast(trimmed)
            .or_else(|| parse_named_color(trimmed))
    }
}

fn parse_color_fast(s: &str) -> Option<Color> {
    if s.starts_with('#') {
        let hex = &s[1..];
        if hex.len() == 6 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            return Some(Color::from_rgba8(r, g, b, 255));
        } else if hex.len() == 3 {
            let r = u8::from_str_radix(&hex[0..1], 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2], 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3], 16).ok()?;
            return Some(Color::from_rgba8(r * 17, g * 17, b * 17, 255));
        } else if hex.len() == 8 {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            return Some(Color::from_rgba8(r, g, b, a));
        }
    } else if s.starts_with("rgba(") && s.ends_with(')') {
        let content = &s[5..s.len() - 1];
        let mut parts = content.split(',');
        let r = parts.next()?.trim().parse::<u8>().ok()?;
        let g = parts.next()?.trim().parse::<u8>().ok()?;
        let b = parts.next()?.trim().parse::<u8>().ok()?;
        let a_str = parts.next()?.trim();
        let a = (a_str.parse::<f32>().ok()? * 255.0) as u8;
        return Some(Color::from_rgba8(r, g, b, a));
    } else if s.starts_with("rgb(") && s.ends_with(')') {
        let content = &s[4..s.len() - 1];
        let mut parts = content.split(',');
        let r = parts.next()?.trim().parse::<u8>().ok()?;
        let g = parts.next()?.trim().parse::<u8>().ok()?;
        let b = parts.next()?.trim().parse::<u8>().ok()?;
        return Some(Color::from_rgba8(r, g, b, 255));
    }
    None
}

/// Resolves `vw`/`vh`/`vmin`/`vmax` lengths against the current global
/// viewport (see [`crate::screen`]). Returns `None` for other units.
fn parse_viewport_unit(val: &str) -> Option<f32> {
    let (num, unit) = if let Some(n) = val.strip_suffix("vmin") {
        (n, "vmin")
    } else if let Some(n) = val.strip_suffix("vmax") {
        (n, "vmax")
    } else if let Some(n) = val.strip_suffix("vw") {
        (n, "vw")
    } else if let Some(n) = val.strip_suffix("vh") {
        (n, "vh")
    } else {
        return None;
    };
    let n: f32 = num.trim().parse().ok()?;
    let (w, h) = crate::screen::size();
    Some(match unit {
        "vw" => n * w / 100.0,
        "vh" => n * h / 100.0,
        "vmin" => n * w.min(h) / 100.0,
        _ => n * w.max(h) / 100.0,
    })
}

pub fn parse_px(val: &str) -> Option<f32> {
    if let Some(stripped) = val.strip_suffix("px") {
        stripped.parse::<f32>().ok()
    } else if let Some(v) = parse_viewport_unit(val) {
        Some(v)
    } else {
        val.parse::<f32>().ok()
    }
}

pub fn parse_dimension(val: &str) -> Option<Dimension> {
    if val.ends_with("%") {
        if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
            return Some(Dimension::percent(p / 100.0));
        }
    } else if let Some(w) = parse_px(val) {
        return Some(length(w));
    }
    None
}

pub fn parse_length_percentage(val: &str) -> Option<LengthPercentage> {
    if val.ends_with("%") {
        if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
            return Some(LengthPercentage::percent(p / 100.0));
        }
    } else if let Some(w) = parse_px(val) {
        return Some(LengthPercentage::length(w));
    }
    None
}

pub fn parse_length_percentage_auto(val: &str) -> Option<LengthPercentageAuto> {
    if val == "auto" {
        return Some(LengthPercentageAuto::auto());
    }
    if val.ends_with("%") {
        if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
            return Some(LengthPercentageAuto::percent(p / 100.0));
        }
    } else if let Some(w) = parse_px(val) {
        return Some(LengthPercentageAuto::length(w));
    }
    None
}

/// Expands a 1-4 value box shorthand (`margin`, `padding`, ...) into
/// top/right/bottom/left: missing right copies top, bottom copies top, left
/// copies right.
pub(crate) fn parse_sides<T: Copy>(val: &str, parse: impl Fn(&str) -> Option<T>) -> Option<taffy::geometry::Rect<T>> {
    let mut values = [None; 4];
    let mut count = 0;
    for part in val.split_whitespace() {
        if count == 4 {
            return None;
        }
        values[count] = Some(parse(part)?);
        count += 1;
    }
    let top = values[0]?;
    let right = values[1].unwrap_or(top);
    let bottom = values[2].unwrap_or(top);
    let left = values[3].unwrap_or(right);
    Some(taffy::geometry::Rect { left, right, top, bottom })
}

pub(crate) fn parse_padding(val: &str) -> Option<taffy::geometry::Rect<LengthPercentage>> {
    parse_sides(val, parse_length_percentage)
}

pub(crate) fn parse_margin(val: &str) -> Option<taffy::geometry::Rect<LengthPercentageAuto>> {
    parse_sides(val, parse_length_percentage_auto)
}

pub fn parse_box_shadow(val: &str) -> Option<crate::style::BoxShadow> {
    let mut s = val.trim();
    if s.is_empty() || s == "none" {
        return None;
    }

    let mut inset = false;
    if s.starts_with("inset") {
        inset = true;
        s = s[5..].trim();
    } else if s.ends_with("inset") {
        inset = true;
        s = s[..s.len() - 5].trim();
    }

    let mut color = Color::from_rgba8(0, 0, 0, 128);

    if let Some(rgba_start) = s.find("rgba(").or_else(|| s.find("rgb(")) {
        if let Some(rgba_end) = s[rgba_start..].find(')') {
            let color_sub = &s[rgba_start..=rgba_start + rgba_end];
            if let Some(c) = parse_hex_color(color_sub) {
                color = c;
            }
            let mut prefix = s[..rgba_start].trim().to_string();
            let suffix = s[rgba_start + rgba_end + 1..].trim();
            if !suffix.is_empty() {
                if !prefix.is_empty() {
                    prefix.push(' ');
                }
                prefix.push_str(suffix);
            }
            let parts: Vec<&str> = prefix.split_whitespace().collect();
            let lengths: Vec<f32> = parts.iter().filter_map(|p| parse_px(p)).collect();
            if lengths.len() >= 2 {
                return Some(crate::style::BoxShadow {
                    offset_x: lengths[0],
                    offset_y: lengths[1],
                    blur_radius: if lengths.len() > 2 { lengths[2] } else { 0.0 },
                    spread_radius: if lengths.len() > 3 { lengths[3] } else { 0.0 },
                    color,
                    inset,
                });
            }
        }
    }

    let parts: Vec<&str> = s.split_whitespace().collect();
    let mut num_parts = Vec::new();
    for part in parts {
        if let Some(c) = parse_hex_color(part) {
            color = c;
        } else if part != "inset" {
            num_parts.push(part);
        }
    }
    let lengths: Vec<f32> = num_parts.iter().filter_map(|p| parse_px(p)).collect();
    if lengths.len() >= 2 {
        Some(crate::style::BoxShadow {
            offset_x: lengths[0],
            offset_y: lengths[1],
            blur_radius: if lengths.len() > 2 { lengths[2] } else { 0.0 },
            spread_radius: if lengths.len() > 3 { lengths[3] } else { 0.0 },
            color,
            inset,
        })
    } else {
        None
    }
}

