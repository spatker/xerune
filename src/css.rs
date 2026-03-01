use crate::{Color, ContainerStyle, LinearGradient};
use csscolorparser::parse as parse_color;
use taffy::prelude::*;
use taffy::style::Style;

pub fn parse_inline_style(style_str: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    for decl in style_str.split(';') {
        let decl = decl.trim();
        if decl.is_empty() {
            continue;
        }

        let parts: Vec<&str> = decl.splitn(2, ':').collect();
        if parts.len() != 2 {
            continue;
        }

        let prop = parts[0].trim().to_lowercase();
        let val = parts[1].trim();

        match prop.as_str() {
            "color" => {
                if let Ok(c) = parse_color(val) {
                    current_style.color = Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    );
                }
            }
            "background-color" => {
                if let Ok(c) = parse_color(val) {
                    current_style.background_color = Some(Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ));
                    current_style.background_gradient = None; // Color overrides gradient if set.
                }
            }
             "background" => {
                 if val.contains("linear-gradient") {
                     if let Some(grad) = parse_linear_gradient(val) {
                         current_style.background_gradient = Some(grad);
                         current_style.background_color = None;
                     }
                 } else if let Ok(c) = parse_color(val) {
                    current_style.background_color = Some(Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ));
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
                if let Ok(c) = parse_color(val) {
                    current_style.border_color = Some(Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ));
                }
            }
            "border" => {
                // Simplified: "1px solid #fff"
                let parts: Vec<&str> = val.split_whitespace().collect();
                for part in parts {
                    if let Some(w) = parse_px(part) {
                        current_style.border_width = w;
                    } else if let Ok(c) = parse_color(part) {
                         current_style.border_color = Some(Color::from_rgba8(
                            (c.r * 255.0) as u8,
                            (c.g * 255.0) as u8,
                            (c.b * 255.0) as u8,
                            (c.a * 255.0) as u8,
                        ));
                    }
                }
            }
            "padding" => {
                if let Some(p) = parse_padding(val) {
                    taffy_style.padding = p;
                }
            }
            "padding-left" => {
                if let Some(p) = parse_px(val) {
                    taffy_style.padding.left = length(p);
                }
            }
            "padding-right" => {
                if let Some(p) = parse_px(val) {
                    taffy_style.padding.right = length(p);
                }
            }
            "padding-top" => {
                if let Some(p) = parse_px(val) {
                    taffy_style.padding.top = length(p);
                }
            }
            "padding-bottom" => {
                if let Some(p) = parse_px(val) {
                    taffy_style.padding.bottom = length(p);
                }
            }
            "margin" => {
                if let Some(m) = parse_margin(val) {
                    taffy_style.margin = m;
                }
            }
            "margin-left" => {
                if let Some(m) = parse_px(val) {
                    taffy_style.margin.left = length(m);
                }
            }
            "margin-right" => {
                if let Some(m) = parse_px(val) {
                    taffy_style.margin.right = length(m);
                }
            }
            "margin-top" => {
                if let Some(m) = parse_px(val) {
                    taffy_style.margin.top = length(m);
                }
            }
            "margin-bottom" => {
                if let Some(m) = parse_px(val) {
                    taffy_style.margin.bottom = length(m);
                }
            }
            "width" => {
                if val.ends_with("%") {
                    if let Ok(p) = val.trim_end_matches('%').parse::<f32>() {
                        taffy_style.size.width = Dimension::percent(p / 100.0);
                    }
                } else if let Some(w) = parse_px(val) {
                    taffy_style.size.width = length(w);
                }
            }
            "height" => {
                if let Some(h) = parse_px(val) {
                    taffy_style.size.height = length(h);
                }
            }
            "min-height" => {
                if let Some(h) = parse_px(val) {
                    taffy_style.min_size.height = length(h);
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
                if let Some(v) = parse_px(val) {
                    taffy_style.inset.left = LengthPercentageAuto::length(v);
                }
            }
            "right" => {
                if let Some(v) = parse_px(val) {
                    taffy_style.inset.right = LengthPercentageAuto::length(v);
                }
            }
            "top" => {
                if let Some(v) = parse_px(val) {
                    taffy_style.inset.top = LengthPercentageAuto::length(v);
                }
            }
            "bottom" => {
                if let Some(v) = parse_px(val) {
                    taffy_style.inset.bottom = LengthPercentageAuto::length(v);
                }
            }
            _ => {
                log::warn!("Unsupported CSS property: {}", prop);
            }
        }
    }
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
        if let Ok(c) = parse_color(color_str) {
             let color = Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
             );
             
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

fn parse_padding(val: &str) -> Option<taffy::geometry::Rect<LengthPercentage>> {
    let parts: Vec<&str> = val.split_whitespace().collect();
    match parts.len() {
        1 => {
            if let Some(v) = parse_px(parts[0]) {
                Some(taffy::geometry::Rect {
                    left: LengthPercentage::length(v),
                    right: LengthPercentage::length(v),
                    top: LengthPercentage::length(v),
                    bottom: LengthPercentage::length(v),
                })
            } else {
                None
            }
        }
        2 => {
            let v = parse_px(parts[0])?;
            let h = parse_px(parts[1])?;
            Some(taffy::geometry::Rect {
                left: LengthPercentage::length(h),
                right: LengthPercentage::length(h),
                top: LengthPercentage::length(v),
                bottom: LengthPercentage::length(v),
            })
        }
        4 => {
            let t = parse_px(parts[0])?;
            let r = parse_px(parts[1])?;
            let b = parse_px(parts[2])?;
            let l = parse_px(parts[3])?;
            Some(taffy::geometry::Rect {
                left: LengthPercentage::length(l),
                right: LengthPercentage::length(r),
                top: LengthPercentage::length(t),
                bottom: LengthPercentage::length(b),
            })
        }
        _ => None
    }
}

fn parse_margin(val: &str) -> Option<taffy::geometry::Rect<LengthPercentageAuto>> {
    let parts: Vec<&str> = val.split_whitespace().collect();
    // Helper closure to convert px to LengthPercentageAuto
    let to_lpa = |v: f32| LengthPercentageAuto::length(v);
    
    match parts.len() {
        1 => {
            if let Some(v) = parse_px(parts[0]) {
                Some(taffy::geometry::Rect {
                    left: to_lpa(v),
                    right: to_lpa(v),
                    top: to_lpa(v),
                    bottom: to_lpa(v),
                })
            } else {
                None
            }
        }
        2 => {
            let v = parse_px(parts[0])?;
            let h = parse_px(parts[1])?;
            Some(taffy::geometry::Rect {
                left: to_lpa(h),
                right: to_lpa(h),
                top: to_lpa(v),
                bottom: to_lpa(v),
            })
        }
        4 => {
            let t = parse_px(parts[0])?;
            let r = parse_px(parts[1])?;
            let b = parse_px(parts[2])?;
            let l = parse_px(parts[3])?;
            Some(taffy::geometry::Rect {
                left: to_lpa(l),
                right: to_lpa(r),
                top: to_lpa(t),
                bottom: to_lpa(b),
            })
        }
        _ => None
    }
}
