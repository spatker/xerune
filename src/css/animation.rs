use taffy::prelude::*;
use taffy::style::CompactLength;
use crate::graphics::{Color, Gradient};
use crate::style::{ContainerStyle, AnimationIterationCount};
use crate::alloc_prelude::*;

#[derive(Clone, Debug)]
pub struct Keyframe {
    pub percentage: f32, // 0.0 to 1.0
    /// The values the keyframe sets, parsed once.
    pub declarations: Vec<(Property, Value)>,
    /// The keyframe's `animation-timing-function`, for the segment after it.
    pub easing: Option<Easing>,
}

/// A timing function (`animation-timing-function`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Easing {
    Linear,
    CubicBezier(f32, f32, f32, f32),
    /// `steps(n, position)`: `jumps_at_start` and `jumps_at_end` say which
    /// ends of the interval jump (`jump-start`/`start`, `jump-end`/`end`,
    /// both for `jump-both`, neither for `jump-none`).
    Steps { steps: u32, jumps_at_start: bool, jumps_at_end: bool },
}

impl Easing {
    /// Parses a timing function; `None` if it isn't one.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        let bezier = |x1, y1, x2, y2| Some(Easing::CubicBezier(x1, y1, x2, y2));
        match value.as_str() {
            "linear" => return Some(Easing::Linear),
            "ease" => return bezier(0.25, 0.1, 0.25, 1.0),
            "ease-in" => return bezier(0.42, 0.0, 1.0, 1.0),
            "ease-out" => return bezier(0.0, 0.0, 0.58, 1.0),
            "ease-in-out" => return bezier(0.42, 0.0, 0.58, 1.0),
            "step-start" => return Some(Easing::Steps { steps: 1, jumps_at_start: true, jumps_at_end: false }),
            "step-end" => return Some(Easing::Steps { steps: 1, jumps_at_start: false, jumps_at_end: true }),
            _ => {}
        }
        let (name, args) = value.strip_suffix(')')?.split_once('(')?;
        let args: Vec<&str> = args.split(',').map(str::trim).collect();
        match (name.trim(), &args[..]) {
            ("cubic-bezier", [x1, y1, x2, y2]) => {
                let n = |v: &str| v.parse::<f32>().ok();
                let (x1, y1, x2, y2) = (n(x1)?, n(y1)?, n(x2)?, n(y2)?);
                ((0.0..=1.0).contains(&x1) && (0.0..=1.0).contains(&x2)).then_some(Easing::CubicBezier(x1, y1, x2, y2))
            }
            ("steps", [steps, rest @ ..]) => {
                let steps: u32 = steps.parse().ok().filter(|&n| n > 0)?;
                let (jumps_at_start, jumps_at_end) = match rest {
                    [] | ["end" | "jump-end"] => (false, true),
                    ["start" | "jump-start"] => (true, false),
                    ["jump-both"] => (true, true),
                    ["jump-none"] if steps > 1 => (false, false),
                    _ => return None,
                };
                Some(Easing::Steps { steps, jumps_at_start, jumps_at_end })
            }
            _ => None,
        }
    }

    /// The eased progress at input progress `t` (0..=1).
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::CubicBezier(x1, y1, x2, y2) => solve_cubic_bezier(x1, y1, x2, y2, t),
            Easing::Steps { steps, jumps_at_start, jumps_at_end } => {
                // CSS Easing 1, "step easing functions".
                let steps = steps as f32;
                let mut step = crate::graphics::floor(t * steps);
                if jumps_at_start {
                    step += 1.0;
                }
                if (0.0..=1.0).contains(&t) {
                    step = step.max(0.0);
                }
                let jumps = steps + jumps_at_start as u32 as f32 + jumps_at_end as u32 as f32 - 1.0;
                if t <= 1.0 {
                    step = step.min(jumps);
                }
                step / jumps
            }
        }
    }
}

fn solve_cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 {
        return 0.0;
    }
    if t >= 1.0 {
        return 1.0;
    }
    let curve = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    // Newton's method on x(u) = t, then y(u).
    let mut u = t;
    for _ in 0..8 {
        let v = 1.0 - u;
        let dx = 3.0 * v * v * x1 + 6.0 * v * u * (x2 - x1) + 3.0 * u * u * (1.0 - x2);
        if dx.abs() < 1e-6 {
            break;
        }
        u = (u - (curve(x1, x2, u) - t) / dx).clamp(0.0, 1.0);
    }
    curve(y1, y2, u)
}

/// A property keyframe animations can interpolate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Property {
    Color,
    BackgroundColor,
    /// `background` / `background-image`: a gradient or a color.
    Background,
    BorderColor,
    BorderWidth,
    BorderRadius,
    FontSize,
    Width,
    Height,
    /// `top`, `right`, `bottom`, `left`.
    Inset(Side),
    Margin(Side),
    Padding(Side),
}

/// A side of a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

/// A value of a [`Property`].
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Color(Color),
    Gradient(Gradient),
    Px(f32),
    Dimension(Dimension),
    LengthAuto(LengthPercentageAuto),
    Length(LengthPercentage),
}

impl Property {
    /// The property named `name`, if it can be animated.
    pub fn from_name(name: &str) -> Option<Self> {
        let side = |s: &str| match s {
            "top" => Some(Side::Top),
            "right" => Some(Side::Right),
            "bottom" => Some(Side::Bottom),
            "left" => Some(Side::Left),
            _ => None,
        };
        Some(match name {
            "color" => Property::Color,
            "background-color" => Property::BackgroundColor,
            "background" | "background-image" => Property::Background,
            "border-color" => Property::BorderColor,
            "border-width" => Property::BorderWidth,
            "border-radius" => Property::BorderRadius,
            "font-size" => Property::FontSize,
            "width" => Property::Width,
            "height" => Property::Height,
            _ => {
                if let Some(side) = side(name) {
                    Property::Inset(side)
                } else if let Some(side) = name.strip_prefix("margin-").and_then(side) {
                    Property::Margin(side)
                } else {
                    Property::Padding(name.strip_prefix("padding-").and_then(side)?)
                }
            }
        })
    }

    /// Whether changing it changes layout, not only painting.
    pub fn affects_layout(self) -> bool {
        matches!(self, Property::FontSize | Property::Width | Property::Height | Property::Inset(_) | Property::Margin(_) | Property::Padding(_) | Property::BorderWidth)
    }

    /// Parses a value of the property.
    pub fn parse(self, value: &str) -> Option<Value> {
        match self {
            Property::Color | Property::BackgroundColor | Property::BorderColor => super::parse_hex_color(value).map(Value::Color),
            Property::Background => super::parse_gradient(value).map(Value::Gradient).or_else(|| super::parse_hex_color(value).map(Value::Color)),
            Property::BorderWidth | Property::BorderRadius | Property::FontSize => super::parse_px(value).map(Value::Px),
            Property::Width | Property::Height => super::parse_dimension(value).map(Value::Dimension),
            Property::Inset(_) | Property::Margin(_) => super::parse_length_percentage_auto(value).map(Value::LengthAuto),
            Property::Padding(_) => super::parse_length_percentage(value).map(Value::Length),
        }
    }

    /// The property's value in a computed style.
    pub fn get(self, style: &ContainerStyle, layout: &Style) -> Option<Value> {
        let pick = |rect: &taffy::geometry::Rect<LengthPercentageAuto>, side| match side {
            Side::Top => rect.top,
            Side::Right => rect.right,
            Side::Bottom => rect.bottom,
            Side::Left => rect.left,
        };
        Some(match self {
            Property::Color => Value::Color(style.color),
            // No background is transparent, no border color `currentColor`.
            Property::BackgroundColor => Value::Color(style.background_color.unwrap_or(Color::from_rgba8(0, 0, 0, 0))),
            Property::Background => match &style.background_gradient {
                Some(gradient) => Value::Gradient(gradient.clone()),
                None => Value::Color(style.background_color.unwrap_or(Color::from_rgba8(0, 0, 0, 0))),
            },
            Property::BorderColor => Value::Color(style.border.top.color.unwrap_or(style.color)),
            Property::BorderWidth => Value::Px(style.border.top.width),
            Property::BorderRadius => Value::Px(style.border_radius),
            Property::FontSize => Value::Px(style.font_size),
            Property::Width => Value::Dimension(layout.size.width),
            Property::Height => Value::Dimension(layout.size.height),
            Property::Inset(side) => Value::LengthAuto(pick(&layout.inset, side)),
            Property::Margin(side) => Value::LengthAuto(pick(&layout.margin, side)),
            Property::Padding(side) => Value::Length(match side {
                Side::Top => layout.padding.top,
                Side::Right => layout.padding.right,
                Side::Bottom => layout.padding.bottom,
                Side::Left => layout.padding.left,
            }),
        })
    }

    /// Sets the property in a computed style.
    pub fn set(self, value: &Value, style: &mut ContainerStyle, layout: &mut Style) {
        let px = |length: CompactLength| (length.tag() == CompactLength::LENGTH_TAG).then(|| length.value());
        match (self, value) {
            (Property::Color, Value::Color(c)) => style.color = *c,
            (Property::BackgroundColor, Value::Color(c)) => style.background_color = Some(*c),
            (Property::Background, Value::Color(c)) => {
                style.background_color = Some(*c);
                style.background_gradient = None;
            }
            (Property::Background, Value::Gradient(g)) => {
                style.background_gradient = Some(g.clone());
                style.background_color = None;
            }
            (Property::BorderColor, Value::Color(c)) => style.border.sides_mut().into_iter().for_each(|side| side.color = Some(*c)),
            (Property::BorderWidth, Value::Px(v)) => style.border.sides_mut().into_iter().for_each(|side| side.width = *v),
            (Property::BorderRadius, Value::Px(v)) => style.border_radius = *v,
            (Property::FontSize, Value::Px(v)) => style.font_size = *v,
            (Property::Width, Value::Dimension(d)) => {
                layout.size.width = *d;
                style.width = px(d.into_raw());
            }
            (Property::Height, Value::Dimension(d)) => {
                layout.size.height = *d;
                style.height = px(d.into_raw());
            }
            (Property::Inset(side) | Property::Margin(side), Value::LengthAuto(v)) => {
                let rect = if matches!(self, Property::Inset(_)) { &mut layout.inset } else { &mut layout.margin };
                match side {
                    Side::Top => rect.top = *v,
                    Side::Right => rect.right = *v,
                    Side::Bottom => rect.bottom = *v,
                    Side::Left => rect.left = *v,
                }
            }
            (Property::Padding(side), Value::Length(v)) => {
                let value = px(v.into_raw()).unwrap_or(0.0);
                match side {
                    Side::Top => (layout.padding.top, style.padding_top) = (*v, value),
                    Side::Right => (layout.padding.right, style.padding_right) = (*v, value),
                    Side::Bottom => (layout.padding.bottom, style.padding_bottom) = (*v, value),
                    Side::Left => (layout.padding.left, style.padding_left) = (*v, value),
                }
            }
            _ => {}
        }
    }
}

impl Value {
    /// The value `t` of the way from `self` to `to`. Values that can't be
    /// interpolated (`auto` and a length, a color and a gradient) flip
    /// halfway, as discrete CSS values do.
    pub fn interpolate(&self, to: &Value, t: f32) -> Value {
        self.interpolate_continuous(to, t).unwrap_or_else(|| if t < 0.5 { self.clone() } else { to.clone() })
    }

    /// Whether the way from `self` to `to` is continuous, not a flip
    /// (CSS: the values are interpolable).
    pub fn interpolates_with(&self, to: &Value) -> bool {
        self.interpolate_continuous(to, 0.0).is_some()
    }

    fn interpolate_continuous(&self, to: &Value, t: f32) -> Option<Value> {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        let compact = |a: CompactLength, b: CompactLength| {
            (a.tag() == b.tag() && (a.tag() == CompactLength::LENGTH_TAG || a.tag() == CompactLength::PERCENT_TAG))
                .then(|| (a.tag() == CompactLength::LENGTH_TAG, mix(a.value(), b.value())))
        };
        let interpolated = match (self, to) {
            (Value::Color(a), Value::Color(b)) => {
                let c = |a: u8, b: u8| (mix(a as f32, b as f32) + 0.5).clamp(0.0, 255.0) as u8;
                Some(Value::Color(Color { r: c(a.r, b.r), g: c(a.g, b.g), b: c(a.b, b.b), a: c(a.a, b.a) }))
            }
            (Value::Gradient(a), Value::Gradient(b)) if super::gradient::gradients_interpolate(a, b) => Some(Value::Gradient(super::interpolate_gradient(a, b, t))),
            (Value::Px(a), Value::Px(b)) => Some(Value::Px(mix(*a, *b))),
            (Value::Dimension(a), Value::Dimension(b)) => compact(a.into_raw(), b.into_raw())
                .map(|(px, v)| Value::Dimension(if px { Dimension::length(v) } else { Dimension::percent(v) })),
            (Value::LengthAuto(a), Value::LengthAuto(b)) => compact(a.into_raw(), b.into_raw())
                .map(|(px, v)| Value::LengthAuto(if px { LengthPercentageAuto::length(v) } else { LengthPercentageAuto::percent(v) })),
            (Value::Length(a), Value::Length(b)) => compact(a.into_raw(), b.into_raw())
                .map(|(px, v)| Value::Length(if px { LengthPercentage::length(v) } else { LengthPercentage::percent(v) })),
            _ => None,
        };
        interpolated
    }
}

#[derive(Clone, Debug)]
pub struct KeyframesAnimation {
    pub name: String,
    pub keyframes: Vec<Keyframe>,
}

pub fn parse_duration_sec(val: &str) -> f32 {
    parse_time(val).unwrap_or(0.0)
}

/// A CSS time (`1.5s`, `200ms`) in seconds; `None` if `val` isn't one.
pub(crate) fn parse_time(val: &str) -> Option<f32> {
    let val = val.trim().to_ascii_lowercase();
    let (number, scale) = match val.strip_suffix("ms") {
        Some(number) => (number, 0.001),
        None => (val.strip_suffix('s')?, 1.0),
    };
    let numeric = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'+'));
    numeric.then(|| number.parse::<f32>().ok()).flatten().map(|n| n * scale)
}

/// Applies an `animation` shorthand. Only its first animation is used
/// (several, comma-separated, aren't supported).
pub fn parse_animation_shorthand(val: &str, current_style: &mut ContainerStyle) {
    let first = xerune_css::text::split_top_level(val, b',').into_iter().next().unwrap_or("");
    let first = first.replace(['\t', '\n', '\r'], " ");
    let mut durations_found = 0;
    for part in xerune_css::text::split_top_level(&first, b' ').into_iter().filter(|p| !p.is_empty()) {
        let part_lower = part.to_ascii_lowercase();
        if let Some(time) = parse_time(part) {
            if durations_found == 0 {
                current_style.animation_duration = time;
            } else if durations_found == 1 {
                current_style.animation_delay = time;
            }
            durations_found += 1;
        } else if part_lower == "infinite" {
            current_style.animation_iteration_count = AnimationIterationCount::Infinite;
        } else if Easing::parse(part).is_some() {
            current_style.animation_timing_function = Arc::from(part);
        } else if ["normal", "reverse", "alternate", "alternate-reverse"].contains(&part_lower.as_str()) {
            current_style.animation_direction = Arc::from(part_lower);
        } else if ["forwards", "backwards", "both"].contains(&part_lower.as_str()) {
            current_style.animation_fill_mode = Arc::from(part_lower);
        } else if ["running", "paused"].contains(&part_lower.as_str()) {
            current_style.animation_play_state = Arc::from(part_lower);
        } else if let Ok(count) = part_lower.parse::<f32>() {
            current_style.animation_iteration_count = AnimationIterationCount::Count(count);
        } else if part_lower == "none" {
            current_style.animation_name = None;
        } else {
            current_style.animation_name = Some(Arc::from(part));
        }
    }
}

pub fn strip_css_comments(css: &str) -> String {
    let mut result = String::new();
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c2) = chars.next() {
                if c2 == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

pub fn parse_keyframes(css: &str) -> HashMap<String, KeyframesAnimation> {
    let css = strip_css_comments(css);
    let mut animations = HashMap::new();
    
    let mut pos = 0;
    while let Some(start_idx) = css[pos..].find("@keyframes") {
        let abs_start = pos + start_idx;
        let rest = &css[abs_start + 10..];
        
        if let Some(brace_idx) = rest.find('{') {
            let name = rest[..brace_idx].trim().to_string();
            let block_content_start = abs_start + 10 + brace_idx + 1;
            let mut brace_count = 1;
            let mut end_idx = None;
            for (idx, c) in css[block_content_start..].char_indices() {
                if c == '{' {
                    brace_count += 1;
                } else if c == '}' {
                    brace_count -= 1;
                    if brace_count == 0 {
                        end_idx = Some(block_content_start + idx);
                        break;
                    }
                }
            }
            
            if let Some(abs_end) = end_idx {
                let block_content = &css[block_content_start..abs_end];
                let keyframes = parse_keyframe_blocks(block_content);
                animations.insert(name.clone(), KeyframesAnimation {
                    name,
                    keyframes,
                });
                pos = abs_end + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    animations
}

fn parse_keyframe_blocks(content: &str) -> Vec<Keyframe> {
    let mut keyframes = Vec::new();
    let mut pos = 0;
    while pos < content.len() {
        let rest = &content[pos..];
        if let Some(brace_idx) = rest.find('{') {
            let selector = rest[..brace_idx].trim();
            let decl_start = pos + brace_idx + 1;
            if let Some(end_brace_idx) = content[decl_start..].find('}') {
                let decl_content = &content[decl_start..decl_start + end_brace_idx];
                let mut easing = None;
                let declarations: Vec<(Property, Value)> = xerune_css::sheet::declarations(decl_content).filter_map(|d| {
                    if d.name == "animation-timing-function" {
                        easing = Easing::parse(&d.value);
                        return None;
                    }
                    let property = Property::from_name(&d.name)?;
                    Some((property, property.parse(&d.value)?))
                }).collect();
                
                for sel in selector.split(',') {
                    let sel = sel.trim();
                    let percentage = if sel.eq_ignore_ascii_case("from") {
                        0.0
                    } else if sel.eq_ignore_ascii_case("to") {
                        1.0
                    } else if sel.ends_with('%') {
                        sel.trim_end_matches('%').parse::<f32>().unwrap_or(0.0) / 100.0
                    } else {
                        continue;
                    };
                    keyframes.push(Keyframe {
                        percentage,
                        declarations: declarations.clone(),
                        easing,
                    });
                }
                pos = decl_start + end_brace_idx + 1;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    keyframes.sort_by(|a, b| a.percentage.partial_cmp(&b.percentage).unwrap_or(core::cmp::Ordering::Equal));
    keyframes
}

