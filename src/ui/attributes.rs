#![allow(missing_docs)]

use taffy::prelude::*;
use crate::style::{ContainerStyle, ControlPart, RenderData, RenderKind};
use crate::css;
use crate::defaults;
use super::node_map::NodeMap;
use crate::alloc_prelude::*;

use super::Interaction;

pub struct ParsedAttributes {
    pub element_type: defaults::ElementType,
    /// A range's `value`; `None` is the middle of `min`..`max`.
    pub slider_value: Option<f32>,
    pub slider_min: f32,
    pub slider_max: f32,
    /// A range's `step`; `None` is `step="any"`.
    pub slider_step: Option<f32>,
    pub progress_value: f32,
    pub progress_max: f32,
    pub checkbox_checked: bool,
    pub interaction_id: Option<Interaction>,
    pub image_src: String,
    pub canvas_id: String,
    pub element_id: Option<String>,
    pub text_input_text: Option<String>,
    pub placeholder: Option<String>,
}

impl ParsedAttributes {
    pub fn new(element_type: defaults::ElementType) -> Self {
        Self {
            element_type,
            slider_value: None,
            slider_min: 0.0,
            slider_max: 100.0,
            slider_step: Some(1.0),
            progress_value: 0.0,
            progress_max: 1.0,
            checkbox_checked: false,
            interaction_id: None,
            image_src: String::new(),
            canvas_id: String::new(),
            element_id: None,
            text_input_text: None,
            placeholder: None,
        }
    }
}

pub fn parse_attributes_generic<'a>(
    tag: &str,
    attrs: impl IntoIterator<Item = (&'a str, &'a str)>,
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &impl Fn(&str) -> bool,
) {
    for (name, value) in attrs {
        match name {
            "id" => {
                parsed.canvas_id = value.to_string();
                parsed.element_id = Some(value.to_string());
            },
            "style" => css::parse_inline_style(value, current_style, layout_style),
            "type" if tag == "input" => {
                // Compiled templates with a literal `type` start from these
                // defaults (`ElementStyle::new_input`) so CSS can override
                // them; only a dynamic `type` still needs them applied here.
                if let Some(key) = defaults::input_defaults_key(value) {
                    let type_defaults = defaults::get_default_style(key, current_style);
                    if parsed.element_type != type_defaults.element_type {
                        parsed.element_type = type_defaults.element_type;
                        if value == "text" {
                            *layout_style = type_defaults.taffy_style;
                            *current_style = type_defaults.container_style;
                        } else {
                            layout_style.size = type_defaults.taffy_style.size;
                            layout_style.margin = type_defaults.taffy_style.margin;
                        }
                    }
                }
            },
            "value" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.slider_value = Some(v);
                    parsed.progress_value = v;
                }
                parsed.text_input_text = Some(value.to_string());
            },
            "max" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.progress_max = v;
                    parsed.slider_max = v;
                }
            },
            "placeholder" => parsed.placeholder = Some(value.to_string()),
            "min" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.slider_min = v;
                }
            },
            "step" => {
                parsed.slider_step = match value.parse::<f32>() {
                    Ok(v) if v > 0.0 => Some(v),
                    _ if value.eq_ignore_ascii_case("any") => None,
                    _ => Some(1.0),
                };
            },
            "checked" => {
                if value == "false" {
                    parsed.checkbox_checked = false;
                } else {
                    parsed.checkbox_checked = true;
                }
            },
            "width" => {
                 if let Ok(w) = value.parse::<f32>() {
                     layout_style.size.width = length(w);
                 }
             },
             "height" => {
                 if let Ok(h) = value.parse::<f32>() {
                     layout_style.size.height = length(h);
                 }
             },
             "src" => parsed.image_src = value.to_string(),
             "data-on-click" => {
                 if !message_validator(value) {
                     log::warn!("Invalid message in data-on-click: {}", value);
                 }
                 parsed.interaction_id = Some(Interaction::Text(value.to_string()));
             }
             _ => {
                 log::debug!("Ignoring attribute: {} on tag: {}", name, tag);
             }
        }
    }
}

/// Non-generic [`parse_attributes_generic`] for derive-generated templates,
/// so each element in a template doesn't instantiate its own copy.
pub fn parse_attribute_slice(
    tag: &str,
    attrs: &[(Cow<'static, str>, Cow<'_, str>)],
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &dyn Fn(&str) -> bool,
) {
    parse_attributes_generic(
        tag,
        attrs.iter().map(|(k, v)| (k.as_ref(), v.as_ref())),
        current_style,
        layout_style,
        parsed,
        &message_validator,
    );
}

/// Where a range's value sits between its `min` and `max`, from 0 to 1,
/// after HTML's value sanitization: the value defaults to the middle, is
/// clamped to the range and snapped to a `step` from `min`.
fn range_fraction(parsed: &ParsedAttributes) -> f32 {
    let min = parsed.slider_min;
    let max = parsed.slider_max.max(min);
    let mut value = parsed.slider_value.unwrap_or(min + (max - min) / 2.0).clamp(min, max);
    if let Some(step) = parsed.slider_step {
        value = min + crate::graphics::floor((value - min) / step + 0.5) * step;
        if value > max {
            value -= step;
        }
    }
    if max > min { (value - min) / (max - min) } else { 0.0 }
}

pub(crate) fn process_element_type(
    id: NodeId,
    parsed: &ParsedAttributes,
    current_style: ContainerStyle,
    render_data: &mut NodeMap<RenderData>,
) {
    let kind = match parsed.element_type {
        defaults::ElementType::Image => RenderKind::Image(parsed.image_src.clone()),
        defaults::ElementType::Checkbox => RenderKind::Checkbox(parsed.checkbox_checked),
        defaults::ElementType::Slider => {
            let parts = super::controls::resolve_parts(&current_style, &[ControlPart::SliderTrack, ControlPart::SliderThumb, ControlPart::SliderFill]);
            RenderKind::Slider(range_fraction(parsed), parts)
        }
        defaults::ElementType::Progress => {
            let parts = super::controls::resolve_parts(&current_style, &[ControlPart::ProgressBar, ControlPart::ProgressValue]);
            RenderKind::Progress(parsed.progress_value, parsed.progress_max, parts)
        }
        defaults::ElementType::Canvas => RenderKind::Canvas(parsed.canvas_id.clone()),
        defaults::ElementType::TextInput => {
            let placeholder = parsed.placeholder.as_ref()
                .filter(|text| !text.is_empty())
                .map(|text| Box::new((text.clone(), super::controls::placeholder_style(&current_style))));
            RenderKind::TextInput(parsed.element_id.clone().unwrap_or_default(), parsed.text_input_text.clone(), placeholder)
        }
        defaults::ElementType::Container => RenderKind::Container,
    };
    render_data.insert(id, RenderData::new(kind, current_style));
}

