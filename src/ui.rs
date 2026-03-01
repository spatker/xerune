use taffy::prelude::*;
use taffy::TaffyError;
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use std::collections::HashMap;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Canvas, DrawCommand, Rect, TextMeasurer};
use crate::style::{ContainerStyle, Overflow, RenderData};
use crate::css;
use crate::defaults;

pub type Interaction = String;

pub struct Ui {
    pub taffy: TaffyTree,
    pub render_data: HashMap<NodeId, RenderData>,
    pub interactions: HashMap<NodeId, Interaction>,
    pub scroll_offsets: HashMap<NodeId, (f32, f32)>,
    pub root: NodeId,
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

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        let root = dom_to_taffy(
            &mut taffy, 
            &dom.document, 
            measurer, 
            &mut render_data, 
            &mut interactions, 
            default_style,
            message_validator
        ).ok_or(TaffyError::ChildIndexOutOfBounds { parent: NodeId::new(0), child_index: 0, child_count: 0 })?; 

        Ok(Self {
            taffy,
            render_data,
            interactions,
            scroll_offsets: HashMap::new(),
            root,
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
        self.taffy.compute_layout(self.root, available_space)
    }

    pub fn build_commands(&self, canvases: &HashMap<String, Canvas>) -> Vec<DrawCommand> {
        layout_to_draw_commands(
            &self.taffy, 
            self.root, 
            &self.render_data, 
            &self.scroll_offsets,
            0.0, 
            0.0
        )
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Interaction> {
         profile!("hit_test");
         if let Some(clicked_node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
             let mut current = Some(clicked_node);
             while let Some(node) = current {
                 if let Some(act) = self.interactions.get(&node) {
                     return Some(act.clone());
                 }
                 current = self.taffy.parent(node);
             }
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
            "id" => parsed.canvas_id = value.to_string(),
            "style" => css::parse_inline_style(value, current_style, layout_style),
            "type" if tag == "input" => {
                if value.as_ref() == "checkbox" {
                    parsed.element_type = defaults::ElementType::Checkbox;
                    layout_style.size = Size { width: length(20.0), height: length(20.0) };
                    layout_style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
                } else if value.as_ref() == "range" {
                    parsed.element_type = defaults::ElementType::Slider;
                    layout_style.size = Size { width: length(100.0), height: length(20.0) };
                }
            },
            "value" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.slider_value = v.clamp(0.0, 1.0);
                    parsed.progress_value = v; 
                }
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
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator) {
                     children.push(id);
                 }
            }
            let id = taffy.new_with_children(Style::default(), &children).ok()?;
            render_data.insert(id, RenderData::Container(current_style));
            Some(id)
        },
        
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            
            let defaults = defaults::get_default_style(tag, &current_style); 
            let mut layout_style = defaults.taffy_style;
            current_style = defaults.container_style;
            
            let mut parsed = ParsedAttributes::new(defaults.element_type);
            
            parse_attributes(tag, &attrs.borrow(), &mut current_style, &mut layout_style, &mut parsed, message_validator);
            
            let mut children = Vec::new();
            if !matches!(parsed.element_type, defaults::ElementType::Image | defaults::ElementType::Checkbox | defaults::ElementType::Slider | defaults::ElementType::Progress | defaults::ElementType::Canvas) {
                for child in handle.children.borrow().iter() {
                     if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator) {
                         children.push(id);
                     }
                }
            }

            let id = taffy.new_with_children(layout_style, &children).ok()?;

            process_element_type(id, &parsed, current_style, render_data);

            if let Some(interaction) = parsed.interaction_id {
                interactions.insert(id, interaction);
            }

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
                let id = taffy.new_leaf(text_layout_style).ok()?;
                render_data.insert(id, RenderData::Text(normalized, current_style));
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
) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    traverse_layout(taffy, root, render_data, scroll_offsets, offset_x, offset_y, &mut commands);
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
            traverse_layout(taffy, child, render_data, scroll_offsets, child_offset_x, child_offset_y, commands);
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

        if let Some(RenderData::Container(style)) = render_data.get(&root) {
            if style.overflow == Overflow::Scroll {
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
