#![allow(missing_docs)]

use taffy::prelude::*;
use crate::style::{ContainerStyle, RenderData};
use crate::css;
use crate::defaults;
use super::node_map::NodeMap;
use crate::alloc_prelude::*;

#[cfg(feature = "dynamic-parser")]
use crate::style::{Overflow, BoxSizing, Display, CssJustifyContent, Direction};
#[cfg(feature = "dynamic-parser")]
use super::Interaction;
#[cfg(feature = "dynamic-parser")]
use markup5ever_rcdom::{Handle as DomHandle, NodeData};
#[cfg(feature = "dynamic-parser")]
use std::rc::Rc;
#[cfg(feature = "dynamic-parser")]
use crate::graphics::TextMeasurer;

#[cfg(feature = "dynamic-parser")]
pub(crate) struct ElementWrapper(pub(crate) DomHandle);

#[cfg(feature = "dynamic-parser")]
impl simplecss::Element for ElementWrapper {
    fn parent_element(&self) -> Option<Self> {
        let parent_weak = self.0.parent.take();
        let parent_opt = parent_weak.as_ref().and_then(|weak| weak.upgrade());
        self.0.parent.set(parent_weak);
        parent_opt.map(ElementWrapper)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let parent = self.parent_element()?;
        let siblings = parent.0.children.borrow();
        let index = siblings.iter().position(|child| Rc::ptr_eq(child, &self.0))?;
        if index > 0 {
            for i in (0..index).rev() {
                let sibling = &siblings[i];
                if matches!(sibling.data, NodeData::Element { .. }) {
                    return Some(ElementWrapper(sibling.clone()));
                }
            }
        }
        None
    }

    fn has_local_name(&self, name: &str) -> bool {
        if let NodeData::Element { name: ref element_name, .. } = self.0.data {
            element_name.local.as_ref() == name
        } else {
            false
        }
    }

    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        if let NodeData::Element { attrs: ref attrs, .. } = self.0.data {
            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == local_name {
                    return operator.matches(attr.value.as_ref());
                }
            }
        }
        false
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => {
                if let Some(parent) = self.parent_element() {
                    let siblings = parent.0.children.borrow();
                    for child in siblings.iter() {
                        if matches!(child.data, NodeData::Element { .. }) {
                            return Rc::ptr_eq(child, &self.0);
                        }
                    }
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }
}

#[cfg(feature = "dynamic-parser")]
pub(crate) fn extract_styles(handle: &DomHandle, css_accumulator: &mut String) {
    if let NodeData::Element { name, .. } = &handle.data {
        if name.local.as_ref() == "style" {
            for child in handle.children.borrow().iter() {
                if let NodeData::Text { contents } = &child.data {
                    css_accumulator.push_str(&contents.borrow());
                    css_accumulator.push('\n');
                }
            }
        }
    }
    for child in handle.children.borrow().iter() {
        extract_styles(child, css_accumulator);
    }
}

#[cfg(feature = "dynamic-parser")]
pub(crate) fn preprocess_dom(handle: &DomHandle) {
    if let NodeData::Element { .. } = handle.data {
        let mut child_elements = Vec::new();
        for child in handle.children.borrow().iter() {
            if let NodeData::Element { .. } = child.data {
                child_elements.push(child.clone());
            }
            preprocess_dom(child);
        }
        
        let num_children = child_elements.len();
        for (i, child) in child_elements.iter().enumerate() {
            if let NodeData::Element { attrs: ref child_attrs, .. } = child.data {
                let index_class = format!("nth-child-{}", i + 1);
                let is_last = i + 1 == num_children;
                
                let mut attrs_borrow = child_attrs.borrow_mut();
                let mut class_attr_opt = None;
                for attr in attrs_borrow.iter_mut() {
                    if attr.name.local.as_ref() == "class" {
                        class_attr_opt = Some(attr);
                        break;
                    }
                }
                
                if let Some(class_attr) = class_attr_opt {
                    let mut val = class_attr.value.to_string();
                    val.push_str(" ");
                    val.push_str(&index_class);
                    if is_last {
                        val.push_str(" last-child");
                    }
                    class_attr.value = val.into();
                } else {
                    let mut class_val = index_class;
                    if is_last {
                        class_val.push_str(" last-child");
                    }
                    attrs_borrow.push(markup5ever::Attribute {
                        name: markup5ever::QualName::new(
                            None,
                            markup5ever::ns!(),
                            markup5ever::LocalName::from("class"),
                        ),
                        value: class_val.into(),
                    });
                }
            }
        }
    } else if let NodeData::Document = handle.data {
        for child in handle.children.borrow().iter() {
            preprocess_dom(child);
        }
    }
}

pub struct ParsedAttributes {
    pub element_type: defaults::ElementType,
    pub slider_value: f32,
    pub progress_value: f32,
    pub progress_max: f32,
    pub checkbox_checked: bool,
    pub interaction_id: Option<String>,
    pub image_src: String,
    pub canvas_id: String,
    pub element_id: Option<String>,
    pub text_input_text: Option<String>,
}

impl ParsedAttributes {
    pub fn new(element_type: defaults::ElementType) -> Self {
        Self {
            element_type,
            slider_value: 0.0,
            progress_value: 0.0,
            progress_max: 1.0,
            checkbox_checked: false,
            interaction_id: None,
            image_src: String::new(),
            canvas_id: String::new(),
            element_id: None,
            text_input_text: None,
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
                    parsed.slider_value = v.clamp(0.0, 1.0);
                    parsed.progress_value = v; 
                }
                parsed.text_input_text = Some(value.to_string());
            },
            "max" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.progress_max = v;
                }
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
                 parsed.interaction_id = Some(value.to_string());
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

#[cfg(feature = "dynamic-parser")]
pub(crate) fn parse_attributes(
    tag: &str,
    attrs: &std::cell::Ref<Vec<markup5ever::Attribute>>,
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &impl Fn(&str) -> bool,
) {
    parse_attributes_generic(
        tag,
        attrs.iter().map(|attr| (attr.name.local.as_ref(), attr.value.as_ref())),
        current_style,
        layout_style,
        parsed,
        message_validator,
    );
}

pub(crate) fn process_element_type(
    id: NodeId,
    parsed: &ParsedAttributes,
    current_style: ContainerStyle,
    render_data: &mut NodeMap<RenderData>,
) {
    match parsed.element_type {
        defaults::ElementType::Image => {
            render_data.insert(id, RenderData::Image(parsed.image_src.clone(), current_style));
        },
        defaults::ElementType::Checkbox => {
            render_data.insert(id, RenderData::Checkbox(parsed.checkbox_checked, current_style));
        },
        defaults::ElementType::Slider => {
            render_data.insert(id, RenderData::Slider(parsed.slider_value, current_style));
        },
        defaults::ElementType::Progress => {
            render_data.insert(id, RenderData::Progress(parsed.progress_value, parsed.progress_max, current_style));
        },
        defaults::ElementType::Canvas => {
            render_data.insert(id, RenderData::Canvas(parsed.canvas_id.clone(), current_style));
        },
        defaults::ElementType::TextInput => {
            render_data.insert(id, RenderData::TextInput(parsed.element_id.clone().unwrap_or_default(), parsed.text_input_text.clone(), current_style));
        },
        _ => {
            render_data.insert(id, RenderData::Container(current_style));
        }
    }
}

#[cfg(feature = "dynamic-parser")]
pub(crate) fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &DomHandle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &simplecss::StyleSheet<'_>,
    node_to_handle: &mut NodeMap<DomHandle>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
) -> Option<NodeId> {
    
    let mut current_style = parent_style.clone();
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border = crate::style::Borders::NONE;
    current_style.border_radius = 0.0;
    current_style.overflow = Overflow::Visible;

    match &handle.data {
        NodeData::Document => {
             let mut children = Vec::new();
             for child in handle.children.borrow().iter() {
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator, stylesheet, node_to_handle, base_styles) {
                     children.push(id);
                 }
             }
             // The initial containing block.
             let style = Style { display: taffy::style::Display::Block, ..Style::default() };
             let id = taffy.new_with_children(style.clone(), &children).ok()?;
             render_data.insert(id, RenderData::Container(current_style.clone()));
             base_styles.insert(id, (style, current_style));
             node_to_handle.insert(id, handle.clone());
             Some(id)
        },
        
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if tag == "style" || tag == "script" || tag == "head" {
                return None;
            }
            
            let defaults = defaults::get_default_style(tag, &current_style); 
            let mut layout_style = defaults.taffy_style;
            current_style = defaults.container_style;
            
            let mut parsed = ParsedAttributes::new(defaults.element_type);
            
            let el_wrapper = ElementWrapper(handle.clone());
            let matched: Vec<_> = stylesheet.rules.iter().filter(|rule| rule.selector.matches(&el_wrapper)).collect();
            for font_pass in [true, false] {
                for rule in &matched {
                    for decl in &rule.declarations {
                        let name_lower;
                        let name = if decl.name.chars().any(|c| c.is_ascii_uppercase()) {
                            name_lower = decl.name.to_lowercase();
                            &name_lower
                        } else {
                            decl.name
                        };
                        if css::sets_font_size(name) == font_pass {
                            css::apply_declaration(name, decl.value, &mut current_style, &mut layout_style);
                        }
                    }
                }
            }
            parse_attributes(tag, &attrs.borrow(), &mut current_style, &mut layout_style, &mut parsed, message_validator);

            if let Some(d) = current_style.inline_size {
                layout_style.size.width = d;
                if d == taffy::style::Dimension::length(d.value()) {
                    current_style.width = Some(d.value());
                }
            }
            if let Some(d) = current_style.block_size {
                layout_style.size.height = d;
                if d == taffy::style::Dimension::length(d.value()) {
                    current_style.height = Some(d.value());
                }
            }
            if let Some(d) = current_style.min_inline_size {
                layout_style.min_size.width = d;
            }
            if let Some(d) = current_style.max_inline_size {
                layout_style.max_size.width = d;
            }
            if let Some(d) = current_style.min_block_size {
                layout_style.min_size.height = d;
            }
            if let Some(d) = current_style.max_block_size {
                layout_style.max_size.height = d;
            }

            let add_h = current_style.padding_left + current_style.padding_right + current_style.border.horizontal_width();
            let add_v = current_style.padding_top + current_style.padding_bottom + current_style.border.vertical_width();

            if current_style.box_sizing == BoxSizing::ContentBox {
                layout_style.size.width = super::style_resolution::to_border_box(layout_style.size.width, add_h);
                layout_style.size.height = super::style_resolution::to_border_box(layout_style.size.height, add_v);
            } else {
                layout_style.min_size.width = super::style_resolution::to_content_box(layout_style.min_size.width, add_h);
                layout_style.min_size.height = super::style_resolution::to_content_box(layout_style.min_size.height, add_v);
                layout_style.max_size.width = super::style_resolution::to_content_box(layout_style.max_size.width, add_h);
                layout_style.max_size.height = super::style_resolution::to_content_box(layout_style.max_size.height, add_v);
            }

            if layout_style.position == Position::Absolute {
                enum Alignment {
                    Start,
                    End,
                    Center,
                }

                if layout_style.inset.left.is_auto() && layout_style.inset.right.is_auto() {
                    let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
                    let h_align = if is_parent_row {
                        let jc = parent_style.justify_content.unwrap_or(CssJustifyContent::FlexStart);
                        match jc {
                            CssJustifyContent::FlexStart => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            CssJustifyContent::FlexEnd => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            CssJustifyContent::Center | CssJustifyContent::SpaceAround | CssJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            CssJustifyContent::SpaceBetween => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            CssJustifyContent::Start => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            CssJustifyContent::End => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            CssJustifyContent::Left => {
                                Alignment::Start
                            }
                            CssJustifyContent::Right => {
                                Alignment::End
                            }
                        }
                    } else {
                        let align = layout_style.align_self.map(|s| match s {
                            AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                            AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                            AlignSelf::Center => AlignItems::Center,
                            AlignSelf::Baseline => AlignItems::Baseline,
                            AlignSelf::Stretch => AlignItems::Stretch,
                        }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                        match align {
                            AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                                if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            AlignItems::FlexEnd | AlignItems::End => {
                                if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            AlignItems::Center => {
                                Alignment::Center
                            }
                        }
                    };

                    match h_align {
                        Alignment::Start => {
                            layout_style.inset.left = length(parent_style.padding_left);
                        }
                        Alignment::End => {
                            if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                                layout_style.inset.left = length(pw - cw + parent_style.padding_left);
                            } else {
                                layout_style.inset.right = length(parent_style.padding_right);
                            }
                        }
                        Alignment::Center => {
                            if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                                layout_style.inset.left = length((pw - cw) / 2.0 + parent_style.padding_left);
                            } else {
                                layout_style.inset.left = length(parent_style.padding_left);
                            }
                        }
                    }
                }

                if layout_style.inset.top.is_auto() && layout_style.inset.bottom.is_auto() {
                    let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
                    let v_align = if !is_parent_row {
                        let jc = parent_style.justify_content.unwrap_or(CssJustifyContent::FlexStart);
                        match jc {
                            CssJustifyContent::FlexStart => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            CssJustifyContent::FlexEnd => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            CssJustifyContent::Center | CssJustifyContent::SpaceAround | CssJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            CssJustifyContent::SpaceBetween => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            CssJustifyContent::Start | CssJustifyContent::Left | CssJustifyContent::Right => {
                                Alignment::Start
                            }
                            CssJustifyContent::End => {
                                Alignment::End
                            }
                        }
                    } else {
                        let align = layout_style.align_self.map(|s| match s {
                            AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                            AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                            AlignSelf::Center => AlignItems::Center,
                            AlignSelf::Baseline => AlignItems::Baseline,
                            AlignSelf::Stretch => AlignItems::Stretch,
                        }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                        match align {
                            AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                                if parent_style.flex_wrap == FlexWrap::WrapReverse {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            AlignItems::FlexEnd | AlignItems::End => {
                                if parent_style.flex_wrap == FlexWrap::WrapReverse {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            AlignItems::Center => {
                                Alignment::Center
                            }
                        }
                    };

                    match v_align {
                        Alignment::Start => {
                            layout_style.inset.top = length(parent_style.padding_top);
                        }
                        Alignment::End => {
                            if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                                layout_style.inset.top = length(ph - ch + parent_style.padding_top);
                            } else {
                                layout_style.inset.bottom = length(parent_style.padding_bottom);
                            }
                        }
                        Alignment::Center => {
                            if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                                layout_style.inset.top = length((ph - ch) / 2.0 + parent_style.padding_top);
                            } else {
                                layout_style.inset.top = length(parent_style.padding_top);
                            }
                        }
                    }
                }
            }
            
            let mut children = Vec::new();
            if !matches!(parsed.element_type, defaults::ElementType::Image | defaults::ElementType::Checkbox | defaults::ElementType::Slider | defaults::ElementType::Progress | defaults::ElementType::Canvas) {
                for child in handle.children.borrow().iter() {
                     if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator, stylesheet, node_to_handle, base_styles) {
                         children.push(id);
                     }
                }
            }

            if current_style.display.is_flex() {
                children.sort_by_key(|child_id| {
                    render_data.get(child_id).map(|data| data.style().order).unwrap_or(0)
                });
            }

            if current_style.display == Display::None {
                layout_style.display = taffy::style::Display::None;
            } else if !current_style.display.is_flex() {
                super::style_resolution::apply_block_display(taffy, tag, &current_style, &mut layout_style, &children, render_data);
            }

            layout_style.border = current_style.border.widths().map(length);

            if current_style.direction == Direction::Rtl {
                match layout_style.flex_direction {
                    FlexDirection::Row | FlexDirection::RowReverse => {
                        layout_style.flex_direction = match layout_style.flex_direction {
                            FlexDirection::Row => FlexDirection::RowReverse,
                            FlexDirection::RowReverse => FlexDirection::Row,
                            other => other,
                        };
                        layout_style.justify_content = match layout_style.justify_content {
                            Some(JustifyContent::FlexStart) | None => Some(JustifyContent::FlexEnd),
                            Some(JustifyContent::FlexEnd) => Some(JustifyContent::FlexStart),
                            other => other,
                        };
                    }
                    FlexDirection::Column | FlexDirection::ColumnReverse => {
                        layout_style.align_items = match layout_style.align_items {
                            Some(AlignItems::FlexStart) | None => Some(AlignItems::FlexEnd),
                            Some(AlignItems::FlexEnd) => Some(AlignItems::FlexStart),
                            other => other,
                        };
                        layout_style.align_content = match layout_style.align_content {
                            Some(AlignContent::FlexStart) | None => Some(AlignContent::FlexEnd),
                            Some(AlignContent::FlexEnd) => Some(AlignContent::FlexStart),
                            other => other,
                        };
                    }
                }
            }

            if !layout_style.max_size.height.is_auto() {
                let val = layout_style.max_size.height.value();
                if layout_style.max_size.height == Dimension::length(val) {
                    layout_style.max_size.height = Dimension::length(val + current_style.border.vertical_width() + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.max_size.width.is_auto() {
                let val = layout_style.max_size.width.value();
                if layout_style.max_size.width == Dimension::length(val) {
                    layout_style.max_size.width = Dimension::length(val + current_style.border.horizontal_width() + current_style.padding_left + current_style.padding_right);
                }
            }
            if !layout_style.min_size.height.is_auto() {
                let val = layout_style.min_size.height.value();
                if layout_style.min_size.height == Dimension::length(val) {
                    layout_style.min_size.height = Dimension::length(val + current_style.border.vertical_width() + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.min_size.width.is_auto() {
                let val = layout_style.min_size.width.value();
                if layout_style.min_size.width == Dimension::length(val) {
                    layout_style.min_size.width = Dimension::length(val + current_style.border.horizontal_width() + current_style.padding_left + current_style.padding_right);
                }
            }

            let is_parent_flex = parent_style.display.is_flex();
            if is_parent_flex {
                let resolved_align = match layout_style.align_self {
                    None => parent_style.align_items.unwrap_or(AlignItems::Stretch),
                    Some(AlignSelf::FlexStart) => AlignItems::FlexStart,
                    Some(AlignSelf::FlexEnd) => AlignItems::FlexEnd,
                    Some(AlignSelf::Center) => AlignItems::Center,
                    Some(AlignSelf::Baseline) => AlignItems::Baseline,
                    Some(AlignSelf::Stretch) => AlignItems::Stretch,
                    Some(AlignSelf::Start) => AlignItems::Start,
                    Some(AlignSelf::End) => AlignItems::End,
                };
                if resolved_align == AlignItems::Baseline {
                    let is_column = parent_style.flex_direction == FlexDirection::Column 
                        || parent_style.flex_direction == FlexDirection::ColumnReverse;
                    if is_column {
                        layout_style.align_self = Some(AlignSelf::FlexStart);
                    }
                }
            }

            let id = taffy.new_with_children(layout_style.clone(), &children).ok()?;

            process_element_type(id, &parsed, current_style.clone(), render_data);

            if let Some(interaction) = parsed.interaction_id {
                interactions.insert(id, interaction);
            }

            base_styles.insert(id, (layout_style, current_style));
            node_to_handle.insert(id, handle.clone());
            Some(id)
        },
        
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let current_style = super::text_style(&parent_style);
            let normalized = super::collapse_white_space(&text, current_style.white_space);
            
            if normalized.is_empty() {
                None
            } else {
                // Sized by the layout's measure function (see `Ui::compute_layout`).
                let text_layout_style = super::text_leaf_style(&current_style);
                let id = taffy.new_leaf(text_layout_style.clone()).ok()?;
                render_data.insert(id, RenderData::Text(normalized.into_owned(), current_style.clone()));
                base_styles.insert(id, (text_layout_style, current_style));
                node_to_handle.insert(id, handle.clone());
                Some(id)
            }
        },

        _ => {
            log::warn!("Unsupported NodeData encountered");
            None
        }
    }
}
