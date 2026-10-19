#![allow(missing_docs)]

use taffy::prelude::*;
use crate::style::{ContainerStyle, ControlPart, RenderData, RenderKind};
use crate::css;
use crate::defaults;
use super::node_map::NodeMap;
use crate::alloc_prelude::*;

use super::Interaction;

#[derive(Clone)]
pub struct ParsedAttributes {
    pub element_type: defaults::ElementType,
    /// A range's `min`, `max`, `step` and `value`.
    pub range: crate::style::Range,
    pub progress_value: f32,
    pub progress_max: f32,
    pub checkbox_checked: bool,
    pub interaction_id: Option<Interaction>,
    /// `data-on-hover` and `data-on-hover-out`.
    pub hover: super::HoverHandlers,
    /// `data-on-transitionrun` and the other transition events.
    pub transition: super::TransitionHandlers,
    /// `data-on-input` and `data-on-change`.
    pub value: super::ValueHandlers,
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
            range: crate::style::Range::default(),
            progress_value: 0.0,
            progress_max: 1.0,
            checkbox_checked: false,
            interaction_id: None,
            hover: super::HoverHandlers::default(),
            transition: super::TransitionHandlers::default(),
            value: super::ValueHandlers::default(),
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
                    parsed.range.value = Some(v);
                    parsed.progress_value = v;
                }
                parsed.text_input_text = Some(value.to_string());
            },
            "max" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.progress_max = v;
                    parsed.range.max = v;
                }
            },
            "placeholder" => parsed.placeholder = Some(value.to_string()),
            "min" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.range.min = v;
                }
            },
            "step" => {
                parsed.range.step = match value.parse::<f32>() {
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
             "data-on-hover" | "data-on-hover-out" => {
                 if !message_validator(value) {
                     log::warn!("Invalid message in {}: {}", name, value);
                 }
                 let handler = Some(Interaction::Text(value.to_string()));
                 if name == "data-on-hover" {
                     parsed.hover.enter = handler;
                 } else {
                     parsed.hover.leave = handler;
                 }
             }
             "data-on-input" | "data-on-change" => {
                 // The input's value follows: `seek` sends `seek:42`.
                 if !message_validator(&format!("{value}:")) {
                     log::warn!("Invalid message in {}: {}", name, value);
                 }
                 let handler = Some(value.to_string());
                 if name == "data-on-input" {
                     parsed.value.input = handler;
                 } else {
                     parsed.value.change = handler;
                 }
             }
             "data-on-transitionrun" | "data-on-transitionstart" | "data-on-transitionend" | "data-on-transitioncancel" => {
                 if !message_validator(value) {
                     log::warn!("Invalid message in {}: {}", name, value);
                 }
                 let handler = Some(Interaction::Text(value.to_string()));
                 match name {
                     "data-on-transitionrun" => parsed.transition.run = handler,
                     "data-on-transitionstart" => parsed.transition.start = handler,
                     "data-on-transitionend" => parsed.transition.end = handler,
                     _ => parsed.transition.cancel = handler,
                 }
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
            RenderKind::Slider(parsed.range, parts)
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

