use crate::Color;
use crate::TextStyle;
use csscolorparser::parse as parse_color;
use taffy::prelude::*;
use taffy::style::Style;

pub fn parse_inline_style(style_str: &str, current_style: &mut TextStyle, taffy_style: &mut Style) {
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
            "background-color" | "background" => {
                if let Ok(c) = parse_color(val) {
                    current_style.background_color = Some(Color::from_rgba8(
                        (c.r * 255.0) as u8,
                        (c.g * 255.0) as u8,
                        (c.b * 255.0) as u8,
                        (c.a * 255.0) as u8,
                    ));
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
            "padding" => {
                if let Some(p) = parse_padding(val) {
                    taffy_style.padding = p;
                }
            }
            "margin" => {
                if let Some(m) = parse_margin(val) {
                    taffy_style.margin = m;
                }
            }
            "width" => {
                if let Some(w) = parse_px(val) {
                    taffy_style.size.width = length(w);
                }
            }
            "height" => {
                if let Some(h) = parse_px(val) {
                    taffy_style.size.height = length(h);
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
            _ => {}
        }
    }
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
