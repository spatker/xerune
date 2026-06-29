use taffy::prelude::*;
use crate::style::{ContainerStyle, AnimationIterationCount};

#[derive(Clone, Debug)]
pub struct ActiveAnimation {
    pub node_id: NodeId,
    pub name: std::sync::Arc<str>,
    pub duration: f32,
    pub timing_function: std::sync::Arc<str>,
    pub delay: f32,
    pub iteration_count: AnimationIterationCount,
    pub direction: std::sync::Arc<str>,
    pub fill_mode: std::sync::Arc<str>,
    pub play_state: std::sync::Arc<str>,
    pub elapsed: std::time::Duration,
    pub is_finished: bool,
}

pub(crate) fn ease(t: f32, func: &str) -> f32 {
    match func {
        "linear" => t,
        "ease" => solve_cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
        "ease-in" => solve_cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
        "ease-out" => solve_cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
        "ease-in-out" => solve_cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
        _ => {
            if func.starts_with("cubic-bezier(") && func.ends_with(')') {
                let inner = &func["cubic-bezier(".len()..func.len() - 1];
                let parts: Vec<&str> = inner.split(',').collect();
                if parts.len() == 4 {
                    let x1 = parts[0].trim().parse::<f32>().unwrap_or(0.0);
                    let y1 = parts[1].trim().parse::<f32>().unwrap_or(0.0);
                    let x2 = parts[2].trim().parse::<f32>().unwrap_or(1.0);
                    let y2 = parts[3].trim().parse::<f32>().unwrap_or(1.0);
                    return solve_cubic_bezier(x1, y1, x2, y2, t);
                }
            }
            t
        }
    }
}

fn solve_cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 { return 0.0; }
    if t >= 1.0 { return 1.0; }

    let mut u = t;
    for _ in 0..8 {
        let x = sample_curve_x(x1, x2, u);
        let dx = sample_curve_derivative_x(x1, x2, u);
        if dx.abs() < 1e-6 {
            break;
        }
        let next_u = u - (x - t) / dx;
        u = next_u.clamp(0.0, 1.0);
    }
    sample_curve_y(y1, y2, u)
}

fn sample_curve_x(x1: f32, x2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * t * x1 + 3.0 * tm * t * t * x2 + t * t * t
}

fn sample_curve_y(y1: f32, y2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * t * y1 + 3.0 * tm * t * t * y2 + t * t * t
}

fn sample_curve_derivative_x(x1: f32, x2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * x1 + 6.0 * tm * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2)
}

fn interpolate_color(c1: crate::graphics::Color, c2: crate::graphics::Color, t: f32) -> crate::graphics::Color {
    crate::graphics::Color {
        r: ((1.0 - t) * c1.r as f32 + t * c2.r as f32).round() as u8,
        g: ((1.0 - t) * c1.g as f32 + t * c2.g as f32).round() as u8,
        b: ((1.0 - t) * c1.b as f32 + t * c2.b as f32).round() as u8,
        a: ((1.0 - t) * c1.a as f32 + t * c2.a as f32).round() as u8,
    }
}

fn interpolate_f32(v1: f32, v2: f32, t: f32) -> f32 {
    (1.0 - t) * v1 + t * v2
}

fn interpolate_length_percentage(lp1: LengthPercentage, lp2: LengthPercentage, t: f32) -> Option<LengthPercentage> {
    let r1 = lp1.into_raw();
    let r2 = lp2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(LengthPercentage::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(LengthPercentage::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

fn interpolate_length_percentage_auto(lp1: LengthPercentageAuto, lp2: LengthPercentageAuto, t: f32) -> Option<LengthPercentageAuto> {
    let r1 = lp1.into_raw();
    let r2 = lp2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(LengthPercentageAuto::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(LengthPercentageAuto::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

fn interpolate_dimension(d1: Dimension, d2: Dimension, t: f32) -> Option<Dimension> {
    let r1 = d1.into_raw();
    let r2 = d2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(Dimension::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(Dimension::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

pub(crate) fn interpolate_property(
    prop: &str,
    val1: &str,
    val2: &str,
    t: f32,
    style: &mut ContainerStyle,
    taffy_style: &mut Style,
) {
    if ["color", "background-color", "border-color"].contains(&prop) {
        if let (Some(c1), Some(c2)) = (crate::css::parse_hex_color(val1), crate::css::parse_hex_color(val2)) {
            let interpolated = interpolate_color(c1, c2, t);
            match prop {
                "color" => style.color = interpolated,
                "background-color" => style.background_color = Some(interpolated),
                "border-color" => style.border_color = Some(interpolated),
                _ => {}
            }
        }
    }
    else if ["border-radius", "border-width", "font-size"].contains(&prop) {
        if let (Some(v1), Some(v2)) = (crate::css::parse_px(val1), crate::css::parse_px(val2)) {
            let interpolated = (1.0 - t) * v1 + t * v2;
            match prop {
                "border-radius" => style.border_radius = interpolated,
                "border-width" => style.border_width = interpolated,
                "font-size" => style.font_size = interpolated,
                _ => {}
            }
        }
    }
    else if ["width", "height"].contains(&prop) {
        if let (Some(d1), Some(d2)) = (crate::css::parse_dimension(val1), crate::css::parse_dimension(val2)) {
            if let Some(interpolated) = interpolate_dimension(d1, d2, t) {
                match prop {
                    "width" => {
                        taffy_style.size.width = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.width = Some(raw.value());
                        }
                    }
                    "height" => {
                        taffy_style.size.height = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.height = Some(raw.value());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    else if ["left", "right", "top", "bottom", "margin-left", "margin-right", "margin-top", "margin-bottom"].contains(&prop) {
        if let (Some(lp1), Some(lp2)) = (crate::css::parse_length_percentage_auto(val1), crate::css::parse_length_percentage_auto(val2)) {
            if let Some(interpolated) = interpolate_length_percentage_auto(lp1, lp2, t) {
                match prop {
                    "left" => taffy_style.inset.left = interpolated,
                    "right" => taffy_style.inset.right = interpolated,
                    "top" => taffy_style.inset.top = interpolated,
                    "bottom" => taffy_style.inset.bottom = interpolated,
                    "margin-left" => taffy_style.margin.left = interpolated,
                    "margin-right" => taffy_style.margin.right = interpolated,
                    "margin-top" => taffy_style.margin.top = interpolated,
                    "margin-bottom" => taffy_style.margin.bottom = interpolated,
                    _ => {}
                }
            }
        }
    }
    else if ["padding-left", "padding-right", "padding-top", "padding-bottom"].contains(&prop) {
        if let (Some(lp1), Some(lp2)) = (crate::css::parse_length_percentage(val1), crate::css::parse_length_percentage(val2)) {
            if let Some(interpolated) = interpolate_length_percentage(lp1, lp2, t) {
                match prop {
                    "padding-left" => {
                        taffy_style.padding.left = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_left = raw.value();
                        }
                    }
                    "padding-right" => {
                        taffy_style.padding.right = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_right = raw.value();
                        }
                    }
                    "padding-top" => {
                        taffy_style.padding.top = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_top = raw.value();
                        }
                    }
                    "padding-bottom" => {
                        taffy_style.padding.bottom = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_bottom = raw.value();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

pub(crate) fn get_prop_val(
    kf: Option<&crate::css::Keyframe>,
    prop: &str,
    base_container: &ContainerStyle,
    base_layout: &Style,
) -> Option<String> {
    if let Some(k) = kf {
        if let Some((_, val)) = k.declarations.iter().find(|(p, _)| p == prop) {
            return Some(val.clone());
        }
    }
    match prop {
        "color" => Some(format!("rgba({},{},{},{})", base_container.color.r, base_container.color.g, base_container.color.b, base_container.color.a as f32 / 255.0)),
        "background-color" => base_container.background_color.map(|c| format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a as f32 / 255.0)),
        "border-radius" => Some(format!("{}px", base_container.border_radius)),
        "border-width" => Some(format!("{}px", base_container.border_width)),
        "border-color" => base_container.border_color.map(|c| format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a as f32 / 255.0)),
        "font-size" => Some(format!("{}px", base_container.font_size)),
        "width" => format_compact_length(base_layout.size.width.into_raw()),
        "height" => format_compact_length(base_layout.size.height.into_raw()),
        "padding-left" => format_compact_length(base_layout.padding.left.into_raw()),
        "padding-right" => format_compact_length(base_layout.padding.right.into_raw()),
        "padding-top" => format_compact_length(base_layout.padding.top.into_raw()),
        "padding-bottom" => format_compact_length(base_layout.padding.bottom.into_raw()),
        "margin-left" => format_compact_length(base_layout.margin.left.into_raw()),
        "margin-right" => format_compact_length(base_layout.margin.right.into_raw()),
        "margin-top" => format_compact_length(base_layout.margin.top.into_raw()),
        "margin-bottom" => format_compact_length(base_layout.margin.bottom.into_raw()),
        "left" => format_compact_length(base_layout.inset.left.into_raw()),
        "right" => format_compact_length(base_layout.inset.right.into_raw()),
        "top" => format_compact_length(base_layout.inset.top.into_raw()),
        "bottom" => format_compact_length(base_layout.inset.bottom.into_raw()),
        _ => None,
    }
}

fn format_compact_length(cl: taffy::style::CompactLength) -> Option<String> {
    match cl.tag() {
        taffy::style::CompactLength::LENGTH_TAG => Some(format!("{}px", cl.value())),
        taffy::style::CompactLength::PERCENT_TAG => Some(format!("{}%", cl.value() * 100.0)),
        _ => None,
    }
}
