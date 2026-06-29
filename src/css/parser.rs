use taffy::prelude::*;
use crate::graphics::{Color, LinearGradient};
use csscolorparser::parse as parse_color;
use std::collections::HashMap;

thread_local! {
    static COLOR_CACHE: std::cell::RefCell<HashMap<String, Color>> = std::cell::RefCell::new(HashMap::with_capacity(256));
}

pub fn parse_hex_color(val: &str) -> Option<Color> {
    let trimmed = val.trim();
    if let Some(color) = COLOR_CACHE.with(|cache| cache.borrow().get(trimmed).copied()) {
        return Some(color);
    }
    
    let color = parse_color_fast(trimmed).or_else(|| {
        parse_color(trimmed).ok().map(|c| {
            Color::from_rgba8(
                (c.r * 255.0) as u8,
                (c.g * 255.0) as u8,
                (c.b * 255.0) as u8,
                (c.a * 255.0) as u8,
            )
        })
    });
    
    if let Some(c) = color {
        COLOR_CACHE.with(|cache| {
            cache.borrow_mut().insert(trimmed.to_string(), c);
        });
    }
    color
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

pub fn parse_px(val: &str) -> Option<f32> {
    if let Some(stripped) = val.strip_suffix("px") {
        stripped.parse::<f32>().ok()
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
