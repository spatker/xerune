use taffy::prelude::*;
use crate::graphics::{Color, LinearGradient};
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

pub(crate) fn parse_linear_gradient(val: &str) -> Option<LinearGradient> {
    let inner = val.trim_start_matches("linear-gradient(").trim_end_matches(")");
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.is_empty() { return None; }

    let mut angle = 180.0;
    let mut stops = Vec::new();

    let mut start_idx = 0;
    if parts[0].contains("deg") {
        if let Some(num) = parts[0].trim().replace("deg", "").parse::<f32>().ok() {
            angle = num;
        }
        start_idx = 1;
    } else if parts[0].contains("to right") {
         angle = 90.0;
         start_idx = 1;
    } else if parts[0].contains("to bottom") {
         angle = 180.0;
         start_idx = 1;
    }

    for i in start_idx..parts.len() {
        let stop_str = parts[i].trim();
        let stop_parts: Vec<&str> = stop_str.split_whitespace().collect();
        if stop_parts.is_empty() { continue; }
        
        let color_str = stop_parts[0];
        if let Some(color) = parse_hex_color(color_str) {
             let pos = if stop_parts.len() > 1 {
                 if let Some(p) = stop_parts[1].strip_suffix("%") {
                     p.parse::<f32>().unwrap_or(0.0) / 100.0
                 } else {
                     0.0
                 }
             } else {
                 if i == start_idx { 0.0 } else { 1.0 }
             };
             
             stops.push((color, pos));
         }
    }
    
    Some(LinearGradient { angle, stops: stops.into() })
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

pub(crate) fn parse_padding(val: &str) -> Option<taffy::geometry::Rect<LengthPercentage>> {
    let parts: Vec<&str> = val.split_whitespace().collect();
    match parts.len() {
        1 => {
            if let Some(v) = parse_length_percentage(parts[0]) {
                Some(taffy::geometry::Rect {
                    left: v,
                    right: v,
                    top: v,
                    bottom: v,
                })
            } else {
                None
            }
        }
        2 => {
            let v = parse_length_percentage(parts[0])?;
            let h = parse_length_percentage(parts[1])?;
            Some(taffy::geometry::Rect {
                left: h,
                right: h,
                top: v,
                bottom: v,
            })
        }
        4 => {
            let t = parse_length_percentage(parts[0])?;
            let r = parse_length_percentage(parts[1])?;
            let b = parse_length_percentage(parts[2])?;
            let l = parse_length_percentage(parts[3])?;
            Some(taffy::geometry::Rect {
                left: l,
                right: r,
                top: t,
                bottom: b,
            })
        }
        _ => None
    }
}

pub(crate) fn parse_margin(val: &str) -> Option<taffy::geometry::Rect<LengthPercentageAuto>> {
    let parts: Vec<&str> = val.split_whitespace().collect();
    
    match parts.len() {
        1 => {
            if let Some(v) = parse_length_percentage_auto(parts[0]) {
                Some(taffy::geometry::Rect {
                    left: v,
                    right: v,
                    top: v,
                    bottom: v,
                })
            } else {
                None
            }
        }
        2 => {
            let v = parse_length_percentage_auto(parts[0])?;
            let h = parse_length_percentage_auto(parts[1])?;
            Some(taffy::geometry::Rect {
                left: h,
                right: h,
                top: v,
                bottom: v,
            })
        }
        4 => {
            let t = parse_length_percentage_auto(parts[0])?;
            let r = parse_length_percentage_auto(parts[1])?;
            let b = parse_length_percentage_auto(parts[2])?;
            let l = parse_length_percentage_auto(parts[3])?;
            Some(taffy::geometry::Rect {
                left: l,
                right: r,
                top: t,
                bottom: b,
            })
        }
        _ => None
    }
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

