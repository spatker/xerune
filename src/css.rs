use crate::{Color, ContainerStyle, LinearGradient};
use csscolorparser::parse as parse_color;
use taffy::prelude::*;
use taffy::style::Style;

pub fn parse_inline_style(style_str: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    let tokenizer = simplecss::DeclarationTokenizer::from(style_str);
    for decl in tokenizer {
        let prop = decl.name.to_lowercase();
        let val = decl.value;

        match prop.as_str() {
            "color" => {
                if let Some(c) = parse_hex_color(val) {
                    current_style.color = c;
                }
            }
            "background-color" => {
                if let Some(c) = parse_hex_color(val) {
                    current_style.background_color = Some(c);
                    current_style.background_gradient = None; // Color overrides gradient if set.
                }
            }
             "background" => {
                 if val.contains("linear-gradient") {
                     if let Some(grad) = parse_linear_gradient(val) {
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
                }
            }
            "font-weight" => {
                if val == "bold" || val == "700" || val == "800" || val == "900" {
                    current_style.weight = 1; // Bold
                } else {
                    current_style.weight = 0; // Regular
                }
            }
            "border-radius" => {
                if let Some(r) = parse_px(val) {
                    current_style.border_radius = r;
                } else if val.ends_with("%") {
                     // Hack implementation for 50% on circles
                     if val.trim() == "50%" {
                         current_style.border_radius = 9999.0; // Large radius
                     }
                }
            }
            "border-width" => {
                if let Some(w) = parse_px(val) {
                    current_style.border_width = w;
                }
            }
            "border-color" => {
                if let Some(c) = parse_hex_color(val) {
                    current_style.border_color = Some(c);
                }
            }
            "border" => {
                // Simplified: "1px solid #fff"
                let parts: Vec<&str> = val.split_whitespace().collect();
                for part in parts {
                    if let Some(w) = parse_px(part) {
                        current_style.border_width = w;
                    } else if let Some(c) = parse_hex_color(part) {
                         current_style.border_color = Some(c);
                    }
                }
            }
            "padding" => {
                if let Some(p) = parse_padding(val) {
                    taffy_style.padding = p;
                }
            }
            "padding-left" => {
                if let Some(p) = parse_length_percentage(val) {
                    taffy_style.padding.left = p;
                }
            }
            "padding-right" => {
                if let Some(p) = parse_length_percentage(val) {
                    taffy_style.padding.right = p;
                }
            }
            "padding-top" => {
                if let Some(p) = parse_length_percentage(val) {
                    taffy_style.padding.top = p;
                }
            }
            "padding-bottom" => {
                if let Some(p) = parse_length_percentage(val) {
                    taffy_style.padding.bottom = p;
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
            }
            "height" => {
                if let Some(d) = parse_dimension(val) {
                    taffy_style.size.height = d;
                }
            }
            "min-height" => {
                if let Some(d) = parse_dimension(val) {
                    taffy_style.min_size.height = d;
                }
            }
             "flex-direction" => {
                match val {
                    "row" => taffy_style.flex_direction = FlexDirection::Row,
                    "column" => taffy_style.flex_direction = FlexDirection::Column,
                    "row-reverse" => taffy_style.flex_direction = FlexDirection::RowReverse,
                    "column-reverse" => taffy_style.flex_direction = FlexDirection::ColumnReverse,
                    _ => {}
                }
            }
            "justify-content" => {
                 match val {
                    "flex-start" => taffy_style.justify_content = Some(JustifyContent::FlexStart),
                    "flex-end" => taffy_style.justify_content = Some(JustifyContent::FlexEnd),
                    "center" => taffy_style.justify_content = Some(JustifyContent::Center),
                    "space-between" => taffy_style.justify_content = Some(JustifyContent::SpaceBetween),
                    "space-around" => taffy_style.justify_content = Some(JustifyContent::SpaceAround),
                    "space-evenly" => taffy_style.justify_content = Some(JustifyContent::SpaceEvenly),
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
            "overflow" => {
                match val {
                    "hidden" => current_style.overflow = crate::Overflow::Hidden,
                    "scroll" => current_style.overflow = crate::Overflow::Scroll,
                    "auto" => current_style.overflow = crate::Overflow::Scroll, // Treat auto as scroll for now
                    "visible" => current_style.overflow = crate::Overflow::Visible,
                    _ => {}
                }
            }
            "position" => {
                match val {
                    "absolute" => taffy_style.position = Position::Absolute,
                    "relative" => taffy_style.position = Position::Relative,
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
            _ => {
                log::warn!("Unsupported CSS property: {}", prop);
            }
        }
    }
}

fn parse_hex_color(val: &str) -> Option<Color> {
    parse_color(val).ok().map(|c| {
        Color::from_rgba8(
            (c.r * 255.0) as u8,
            (c.g * 255.0) as u8,
            (c.b * 255.0) as u8,
            (c.a * 255.0) as u8,
        )
    })
}

fn parse_linear_gradient(val: &str) -> Option<LinearGradient> {
    // linear-gradient(180deg, #121212 0%, #1ed760 100%)
    // Simplified parsing: assumes "linear-gradient(" prefix and ")" suffix
    let inner = val.trim_start_matches("linear-gradient(").trim_end_matches(")");
    let parts: Vec<&str> = inner.split(',').collect();
    if parts.is_empty() { return None; }

    let mut angle = 180.0; // Default to bottom
    let mut stops = Vec::new();

    let mut start_idx = 0;
    // Check first part for angle
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
    // ... other directions omitted for brevity

    for i in start_idx..parts.len() {
        let stop_str = parts[i].trim();
        // Split color and percentage
        let stop_parts: Vec<&str> = stop_str.split_whitespace().collect();
        if stop_parts.is_empty() { continue; }
        
        let color_str = stop_parts[0];
        if let Some(color) = parse_hex_color(color_str) {
             let pos = if stop_parts.len() > 1 {
                 if let Some(p) = stop_parts[1].strip_suffix("%") {
                     p.parse::<f32>().unwrap_or(0.0) / 100.0
                 } else {
                     0.0 // Default or parse partial
                 }
             } else {
                 // Distribute evenly if possible
                 if i == start_idx { 0.0 } else { 1.0 }
             };
             
             stops.push((color, pos));
        }
    }
    
    Some(LinearGradient { angle, stops })
}

fn parse_px(val: &str) -> Option<f32> {
    if let Some(stripped) = val.strip_suffix("px") {
        stripped.parse::<f32>().ok()
    } else {
        val.parse::<f32>().ok()
    }
}

fn parse_dimension(val: &str) -> Option<Dimension> {
    if val.ends_with("%") {
        if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
            return Some(Dimension::percent(p / 100.0));
        }
    } else if let Some(w) = parse_px(val) {
        return Some(length(w));
    }
    None
}

fn parse_length_percentage(val: &str) -> Option<LengthPercentage> {
    if val.ends_with("%") {
        if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
            return Some(LengthPercentage::percent(p / 100.0));
        }
    } else if let Some(w) = parse_px(val) {
        return Some(LengthPercentage::length(w));
    }
    None
}

fn parse_length_percentage_auto(val: &str) -> Option<LengthPercentageAuto> {
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

fn parse_padding(val: &str) -> Option<taffy::geometry::Rect<LengthPercentage>> {
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

fn parse_margin(val: &str) -> Option<taffy::geometry::Rect<LengthPercentageAuto>> {
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
