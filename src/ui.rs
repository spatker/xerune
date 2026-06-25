use taffy::prelude::*;
use taffy::TaffyError;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::HashMap;
use std::rc::Rc;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Canvas, DrawCommand, Rect, TextMeasurer};
use crate::style::{ContainerStyle, Overflow, RenderData, Display, TextAlign, Direction, MyJustifyContent, BoxSizing};
use crate::css;
use crate::defaults;

pub type Interaction = String;

struct ElementWrapper(Handle);

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
        if let NodeData::Element { ref attrs, .. } = self.0.data {
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

fn extract_styles(handle: &Handle, css_accumulator: &mut String) {
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


pub struct Ui {
    pub taffy: TaffyTree,
    pub render_data: HashMap<NodeId, RenderData>,
    pub interactions: HashMap<NodeId, Interaction>,
    pub scroll_offsets: HashMap<NodeId, (f32, f32)>,
    pub root: NodeId,
    pub node_to_handle: HashMap<NodeId, Handle>,
    pub base_styles: HashMap<NodeId, (Style, ContainerStyle)>,
    pub keyframes: HashMap<String, css::KeyframesAnimation>,
}

impl Ui {
    pub fn new(
        html: &str, 
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        profile!("ui_new_internal");
        let mut taffy = TaffyTree::new();
        let mut render_data = HashMap::new();
        let mut interactions = HashMap::new();
        let mut node_to_handle = HashMap::new();
        let mut base_styles = HashMap::new();

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        preprocess_dom(&dom.document);

        let mut css_str = String::new();
        extract_styles(&dom.document, &mut css_str);
        
        let keyframes = css::parse_keyframes(&css_str);
        
        let re_nth = regex::Regex::new(r":nth-child\(\s*(\d+)\s*\)").unwrap();
        let css_str = re_nth.replace_all(&css_str, ".nth-child-$1").into_owned();
        let css_str = css_str.replace(":last-child", ".last-child");
        let re_slash = regex::Regex::new(r"/[\d\.]+").unwrap();
        let css_str = re_slash.replace_all(&css_str, "").into_owned();
        
        let stylesheet = simplecss::StyleSheet::parse(&css_str);

        let root = dom_to_taffy(
            &mut taffy, 
            &dom.document, 
            measurer, 
            &mut render_data, 
            &mut interactions, 
            default_style,
            message_validator,
            &stylesheet,
            &mut node_to_handle,
            &mut base_styles,
        ).ok_or(TaffyError::ChildIndexOutOfBounds { parent: NodeId::new(0), child_index: 0, child_count: 0 })?;  

        Ok(Self {
            taffy,
            render_data,
            interactions,
            scroll_offsets: HashMap::new(),
            root,
            node_to_handle,
            base_styles,
            keyframes,
        })
    }

    pub fn handle_scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32) -> bool {
        profile!("handle_scroll");
        if let Some(mut node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
            loop {
                if let Some(RenderData::Container(style)) = self.render_data.get(&node) {
                    if style.overflow == Overflow::Scroll {
                         let (mut sx, mut sy) = self.scroll_offsets.get(&node).copied().unwrap_or((0.0, 0.0));
                         sx -= delta_x;
                         sy -= delta_y;
                         
                         if let Ok(layout) = self.taffy.layout(node) {
                             let container_width = layout.size.width;
                             let container_height = layout.size.height;
                             
                             let mut content_width = 0.0f32;
                             let mut content_height = 0.0f32;
                             
                             if let Ok(children) = self.taffy.children(node) {
                                 for child in children {
                                     if let Ok(child_layout) = self.taffy.layout(child) {
                                         let right = child_layout.location.x + child_layout.size.width;
                                         let bottom = child_layout.location.y + child_layout.size.height;
                                         if right > content_width { content_width = right; }
                                         if bottom > content_height { content_height = bottom; }
                                     }
                                 }
                             }
                             
                             let max_sx = (content_width - container_width).max(0.0);
                             let max_sy = (content_height - container_height).max(0.0);
                             
                             sx = sx.clamp(0.0, max_sx);
                             sy = sy.clamp(0.0, max_sy);
                         }

                         self.scroll_offsets.insert(node, (sx, sy));
                         return true;
                    }
                }
                
                if let Some(parent) = self.taffy.parent(node) {
                    node = parent;
                } else {
                    break;
                }
            }
        }
        false
    }

    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        let node_opt = self.interactions.iter().find(|(_, v)| *v == interaction_id).map(|(k, _)| *k);
        if let Some(node) = node_opt {
             let mut current = node;
             while let Some(parent) = self.taffy.parent(current) {
                 if let Some(RenderData::Container(style)) = self.render_data.get(&parent) {
                     if style.overflow == Overflow::Scroll {
                         if let Ok(parent_layout) = self.taffy.layout(parent) {
                             if let Ok(node_layout) = self.taffy.layout(node) {
                                  let (ck, cy) = self.scroll_offsets.get(&parent).copied().unwrap_or((0.0, 0.0));
                                  let node_y = node_layout.location.y; 
                                  
                                  let mut new_y = cy;
                                  if node_y < cy {
                                      new_y = node_y;
                                  } else if node_y + node_layout.size.height > cy + parent_layout.size.height {
                                      new_y = node_y + node_layout.size.height - parent_layout.size.height;
                                  }
                                  self.scroll_offsets.insert(parent, (ck, new_y));
                                  return;
                             }
                         }
                     }
                 }
                 current = parent;
             }
        }
    }

    pub fn compute_layout(&mut self, available_space: Size<AvailableSpace>) -> Result<(), TaffyError> {
        profile!("taffy_layout");
        self.taffy.compute_layout(self.root, available_space)?;
        Ok(())
    }

    pub fn build_commands(&self, _canvases: &HashMap<String, Canvas>, focused_id: Option<&str>) -> Vec<DrawCommand> {
        layout_to_draw_commands(
            &self.taffy,
            self.root,
            &self.render_data,
            &self.scroll_offsets,
            0.0,
            0.0,
            focused_id
        )
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<(Interaction, NodeId)> {
         profile!("hit_test");
         if let Some(clicked_node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
             let mut current = Some(clicked_node);
             while let Some(node) = current {
                 if let Some(act) = self.interactions.get(&node) {
                     return Some((act.clone(), clicked_node));
                 }
                 current = self.taffy.parent(node);
             }
             return Some((String::new(), clicked_node)); // Return clicked node even without interaction
         }
         None
    }
}

// Struct to hold parsed attributes for easier passing
struct ParsedAttributes {
    element_type: defaults::ElementType,
    slider_value: f32,
    progress_value: f32,
    progress_max: f32,
    checkbox_checked: bool,
    interaction_id: Option<String>,
    image_src: String,
    canvas_id: String,
    element_id: Option<String>,
    text_input_text: Option<String>,
}

impl ParsedAttributes {
    fn new(element_type: defaults::ElementType) -> Self {
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

fn parse_attributes(
    tag: &str,
    attrs: &std::cell::Ref<Vec<markup5ever::Attribute>>,
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &impl Fn(&str) -> bool,
) {
    for attr in attrs.iter() {
        let name = attr.name.local.as_ref();
        let value = &attr.value;
        
        match name {
            "id" => {
                parsed.canvas_id = value.to_string();
                parsed.element_id = Some(value.to_string());
            },
            "style" => css::parse_inline_style(value, current_style, layout_style),
            "type" if tag == "input" => {
                if value.as_ref() == "checkbox" {
                    parsed.element_type = defaults::ElementType::Checkbox;
                    layout_style.size = Size { width: length(20.0), height: length(20.0) };
                    layout_style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
                } else if value.as_ref() == "range" {
                    parsed.element_type = defaults::ElementType::Slider;
                    layout_style.size = Size { width: length(100.0), height: length(20.0) };
                } else if value.as_ref() == "text" {
                    let text_defaults = defaults::get_default_style("input_text", current_style);
                    parsed.element_type = text_defaults.element_type;
                    *layout_style = text_defaults.taffy_style;
                    *current_style = text_defaults.container_style;
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
            "checked" => parsed.checkbox_checked = true,
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

fn process_element_type(
    id: NodeId,
    parsed: &ParsedAttributes,
    current_style: ContainerStyle,
    render_data: &mut HashMap<NodeId, RenderData>,
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

fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &simplecss::StyleSheet<'_>,
    node_to_handle: &mut HashMap<NodeId, Handle>,
    base_styles: &mut HashMap<NodeId, (Style, ContainerStyle)>,
) -> Option<NodeId> {
    
    let mut current_style = parent_style.clone();
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border_width = 0.0;
    current_style.border_radius = 0.0;
    current_style.border_color = None;
    current_style.overflow = Overflow::Visible;

    match &handle.data {
        NodeData::Document => {
             let mut children = Vec::new();
             for child in handle.children.borrow().iter() {
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator, stylesheet, node_to_handle, base_styles) {
                     children.push(id);
                 }
            }
            let id = taffy.new_with_children(Style::default(), &children).ok()?;
            render_data.insert(id, RenderData::Container(current_style.clone()));
            base_styles.insert(id, (Style::default(), current_style));
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
            
            // Match CSS stylesheet rules
            let el_wrapper = ElementWrapper(handle.clone());
            for rule in &stylesheet.rules {
                if rule.selector.matches(&el_wrapper) {
                    for decl in &rule.declarations {
                        css::apply_declaration(&decl.name.to_lowercase(), decl.value, &mut current_style, &mut layout_style);
                    }
                }
            }
            parse_attributes(tag, &attrs.borrow(), &mut current_style, &mut layout_style, &mut parsed, message_validator);





            // Resolve logical properties (inline-size, block-size, etc.)
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

            // Convert box sizes to match Taffy's expected box model:
            // - Taffy expects Style::size to be border-box.
            // - Taffy expects Style::min_size and Style::max_size to be content-box.
            let add_h = current_style.padding_left + current_style.padding_right + current_style.border_width * 2.0;
            let add_v = current_style.padding_top + current_style.padding_bottom + current_style.border_width * 2.0;

            if current_style.box_sizing == BoxSizing::ContentBox {
                layout_style.size.width = to_border_box(layout_style.size.width, add_h);
                layout_style.size.height = to_border_box(layout_style.size.height, add_v);
            } else {
                layout_style.min_size.width = to_content_box(layout_style.min_size.width, add_h);
                layout_style.min_size.height = to_content_box(layout_style.min_size.height, add_v);
                layout_style.max_size.width = to_content_box(layout_style.max_size.width, add_h);
                layout_style.max_size.height = to_content_box(layout_style.max_size.height, add_v);
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
                        let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                        match jc {
                            MyJustifyContent::FlexStart => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::FlexEnd => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            MyJustifyContent::SpaceBetween => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Start => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::End => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Left => {
                                Alignment::Start
                            }
                            MyJustifyContent::Right => {
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
                        let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                        match jc {
                            MyJustifyContent::FlexStart => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::FlexEnd => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            MyJustifyContent::SpaceBetween => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Start | MyJustifyContent::Left | MyJustifyContent::Right => {
                                Alignment::Start
                            }
                            MyJustifyContent::End => {
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

            if current_style.display == Display::Flex {
                children.sort_by_key(|child_id| {
                    render_data.get(child_id).map(|data| data.style().order).unwrap_or(0)
                });
            }

            if current_style.display == Display::None {
                layout_style.display = taffy::style::Display::None;
            } else if current_style.display != Display::Flex {
                let mut has_inline_child = false;
                for child_id in &children {
                    if let Some(child_data) = render_data.get(child_id) {
                        match child_data {
                            RenderData::Container(child_style) => {
                                if child_style.display == Display::InlineBlock || child_style.is_floated {
                                    has_inline_child = true;
                                    break;
                                }
                            }
                            _ => {
                                has_inline_child = true;
                                break;
                            }
                        }
                    }
                }

                if has_inline_child || tag == "tr" {
                    layout_style.flex_direction = FlexDirection::Row;
                    layout_style.flex_wrap = FlexWrap::Wrap;
                    if let Some(align) = current_style.text_align {
                        match align {
                            TextAlign::Right => layout_style.justify_content = Some(JustifyContent::FlexEnd),
                            TextAlign::Center => layout_style.justify_content = Some(JustifyContent::Center),
                            TextAlign::Left => layout_style.justify_content = Some(JustifyContent::FlexStart),
                        }
                    }
                } else {
                    layout_style.flex_direction = FlexDirection::Column;
                }
            }

            if parent_style.display != Display::Flex && current_style.display == Display::Block {
                if layout_style.size.width.is_auto() {
                    layout_style.size.width = Dimension::percent(1.0);
                }
            }

            layout_style.border = taffy::geometry::Rect {
                left: length(current_style.border_width),
                right: length(current_style.border_width),
                top: length(current_style.border_width),
                bottom: length(current_style.border_width),
            };

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

            // Taffy's min_size and max_size constraints are relative to the border-box,
            // but CSS specifies them relative to the content-box. So we adjust them here.
            if !layout_style.max_size.height.is_auto() {
                let val = layout_style.max_size.height.value();
                if layout_style.max_size.height == Dimension::length(val) {
                    layout_style.max_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.max_size.width.is_auto() {
                let val = layout_style.max_size.width.value();
                if layout_style.max_size.width == Dimension::length(val) {
                    layout_style.max_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
                }
            }
            if !layout_style.min_size.height.is_auto() {
                let val = layout_style.min_size.height.value();
                if layout_style.min_size.height == Dimension::length(val) {
                    layout_style.min_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.min_size.width.is_auto() {
                let val = layout_style.min_size.width.value();
                if layout_style.min_size.width == Dimension::length(val) {
                    layout_style.min_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
                }
            }

            let is_parent_flex = parent_style.display == Display::Flex;
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
            let normalized = text.split_whitespace().collect::<Vec<&str>>().join(" ");
            
            if normalized.is_empty() {
                None
            } else {
                let (width, height) = text_measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                let text_layout_style = Style {
                    size: Size { width: length(width), height: length(height) },
                    ..Style::default()
                };
                let id = taffy.new_leaf(text_layout_style.clone()).ok()?;
                render_data.insert(id, RenderData::Text(normalized, current_style.clone()));
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

fn layout_to_draw_commands(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    offset_x: f32,
    offset_y: f32,
    focused_id: Option<&str>,
) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    traverse_layout(taffy, root, render_data, scroll_offsets, offset_x, offset_y, &mut commands, focused_id);
    commands
}

fn traverse_layout(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    offset_x: f32,
    offset_y: f32,
    commands: &mut Vec<DrawCommand>,
    focused_id: Option<&str>,
) {
    let layout = match taffy.layout(root) {
        Ok(l) => l,
        Err(_) => return,
    };
    
    let x = offset_x + layout.location.x;
    let y = offset_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    let mut overflow = Overflow::Visible;
    let rect = Rect { x, y, width, height };

    if let Some(data) = render_data.get(&root) {
        let maybe_style = match data {
            RenderData::Container(style) => Some(style),
            RenderData::Text(_, style) => Some(style),
            _ => None,
        };

        if let Some(style) = maybe_style {
            overflow = style.overflow;
            if style.background_color.is_some() || style.background_gradient.is_some() || style.border_width > 0.0 {
                 commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: style.border_width,
                    border_color: style.border_color,
                });
            }
        }

        match data {
            RenderData::Text(text, style) => {
                commands.push(DrawCommand::DrawText {
                    text: text.clone(),
                    rect,
                    color: style.color,
                    font_size: style.font_size,
                    weight: style.weight,
                });
            },
            RenderData::Image(src, style) => {
                 commands.push(DrawCommand::DrawImage {
                    src: src.clone(),
                    rect,
                    border_radius: style.border_radius,
                });
            },
            RenderData::Checkbox(checked, style) => {
                commands.push(DrawCommand::DrawCheckbox {
                    rect,
                    checked: *checked,
                    color: style.color,
                });
            },
            RenderData::Slider(value, style) => {
                 commands.push(DrawCommand::DrawSlider {
                    rect,
                    value: *value,
                    color: style.color,
                });
            },
            RenderData::Progress(value, max, style) => {
                 commands.push(DrawCommand::DrawProgress {
                    rect,
                    value: *value,
                    max: *max,
                    color: style.color,
                });
            },
            RenderData::Canvas(id, _) => {
                commands.push(DrawCommand::DrawCanvas {
                    id: id.clone(),
                    rect,
                });
            },
            RenderData::TextInput(id, text, style) => {
                let is_focused = focused_id == Some(id.as_str()) && !id.is_empty();
                
                let focus_outline_width = if is_focused { 2.0 } else { 0.0 };
                let focus_outline_color = if is_focused { Some(crate::Color::from_rgba8(0, 122, 255, 255)) } else { None };

                // Draw background and focus ring
                commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: if is_focused { focus_outline_width } else { style.border_width },
                    border_color: if is_focused { focus_outline_color } else { style.border_color },
                });

                // Draw text if present
                if let Some(t) = text {
                    if !t.is_empty() {
                        let text_rect = Rect {
                            x: rect.x + 8.0, // basic padding
                            y: rect.y + 5.0,
                            width: rect.width - 16.0,
                            height: rect.height - 10.0,
                        };
                        commands.push(DrawCommand::DrawText {
                            text: t.clone(),
                            rect: text_rect,
                            color: style.color,
                            font_size: style.font_size,
                            weight: style.weight,
                        });
                    }
                }
            },
            _ => {} 
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::Clip { rect });
    }

    let mut child_offset_x = x;
    let mut child_offset_y = y;

    if overflow == Overflow::Scroll {
        if let Some((sx, sy)) = scroll_offsets.get(&root) {
             child_offset_x -= sx;
             child_offset_y -= sy;
        }
    }

    if let Ok(children) = taffy.children(root) {
        for child in children {
            traverse_layout(taffy, child, render_data, scroll_offsets, child_offset_x, child_offset_y, commands, focused_id);
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::PopClip);
    }
}

pub fn hit_test_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    render_data: &HashMap<NodeId, RenderData>,
    x: f32,
    y: f32,
    abs_x: f32,
    abs_y: f32,
) -> Option<NodeId> {
    let layout = taffy.layout(root).ok()?;
    let left = abs_x + layout.location.x;
    let top = abs_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    if x >= left && x <= left + width && y >= top && y <= top + height {
        let mut child_abs_x = left;
        let mut child_abs_y = top;

        if let Some(data) = render_data.get(&root) {
            let overflow = match data {
                RenderData::Container(style) => style.overflow,
                RenderData::TextInput(_, _, style) => style.overflow,
                _ => Overflow::Visible,
            };

            if overflow == Overflow::Scroll {
                if let Some((sx, sy)) = scroll_offsets.get(&root) {
                    child_abs_x -= sx;
                    child_abs_y -= sy;
                }
            }
        }

        if let Ok(children) = taffy.children(root) {
             for child in children.iter().rev() {
                 if let Some(hit) = hit_test_recursive(taffy, *child, scroll_offsets, render_data, x, y, child_abs_x, child_abs_y) {
                     return Some(hit);
                 }
             }
        }
        return Some(root);
    }
    None
}

fn preprocess_dom(handle: &Handle) {
    if let NodeData::Element { .. } = handle.data {
        // First, recursively process all child element nodes
        let mut child_elements = Vec::new();
        for child in handle.children.borrow().iter() {
            if let NodeData::Element { .. } = child.data {
                child_elements.push(child.clone());
            }
            preprocess_dom(child);
        }
        
        // Add nth-child and last-child classes to each child element
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

fn to_border_box(dim: taffy::style::Dimension, add: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length(dim.value() + add)
    } else {
        dim
    }
}

fn to_content_box(dim: taffy::style::Dimension, sub: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length((dim.value() - sub).max(0.0))
    } else {
        dim
    }
}
