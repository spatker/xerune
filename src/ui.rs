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
        ).ok_or(TaffyError::ChildIndexOutOfBounds { parent: NodeId::new(0), child_index: 0, child_count: 0 })?; // TODO: Better error

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
        // Find node under x,y
         if let Some(mut node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
            // Walk up looking for scrollable
            loop {
                if let Some(RenderData::Container(style)) = self.render_data.get(&node) {
                    if style.overflow == Overflow::Scroll {
                         let (mut sx, mut sy) = self.scroll_offsets.get(&node).copied().unwrap_or((0.0, 0.0));
                         sx -= delta_x;
                         sy -= delta_y;
                         
                         // Clamping Logic
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
        // Find node key by interaction string
        let node_opt = self.interactions.iter().find(|(_, v)| *v == interaction_id).map(|(k, _)| *k);
        if let Some(node) = node_opt {
             // Simplest impl: ensure specific node is visible in its scrollable parent.
             // Walk up to find scrollable parent.
             let mut current = node;
             while let Some(parent) = self.taffy.parent(current) {
                 if let Some(RenderData::Container(style)) = self.render_data.get(&parent) {
                     if style.overflow == Overflow::Scroll {
                         // Calculate new offset
                         // Need layout of 'node' relative to 'parent'
                         // Layouts are absolute? No, relative to parent location.
                         // We need recursive position.
                         // Actually Taffy layout.location is relative to parent.
                         
                         // Logic:
                         // Node top relative to parent content box.
                         // Parent scroll offset.
                         // Parent size.
                         
                         // We must access layouts.
                         if let Ok(parent_layout) = self.taffy.layout(parent) {
                             if let Ok(node_layout) = self.taffy.layout(node) {
                                  // This simple relative check only works for direct children.
                                  // For nested, we need to accumulate.
                                  // Let's assume direct children or simple nesting for now.
                                  // Or just generic "scroll to top".
                                  
                                  // Update offset to 0 (top) for testing
                                  // self.scroll_offsets.insert(parent, (0.0, 0.0));
                                  
                                  // Better: make it visible.
                                  let (ck, cy) = self.scroll_offsets.get(&parent).copied().unwrap_or((0.0, 0.0));
                                  // Node relative y in parent content:
                                  let node_y = node_layout.location.y; 
                                  // If node_y < cy, cy = node_y (scroll up)
                                  // If node_y + h > cy + parent_h, cy = node_y + h - parent_h (scroll down)
                                  
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

// Private helpers

fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
) -> Option<NodeId> {
    
    // Prepare styles: Inherit font-related properties, but reset box-model properties.
    let mut current_style = parent_style.clone();
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border_width = 0.0;
    current_style.border_radius = 0.0;
    current_style.border_color = None;
    current_style.overflow = Overflow::Visible;

    let mut layout_style = Style::default();

    match &handle.data {
        NodeData::Document => {
            // Document just acts as a wrapper, process children
             let mut children = Vec::new();
             for child in handle.children.borrow().iter() {
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator) {
                     children.push(id);
                 }
            }
            let id = taffy.new_with_children(layout_style, &children).ok()?;
            render_data.insert(id, RenderData::Container(current_style));
            Some(id)
        },
        
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            
            // 1. Apply default styles for the tag
            let defaults = defaults::get_default_style(tag, &current_style); 
            layout_style = defaults.taffy_style;
            current_style = defaults.container_style;
            
            // We ensure background is cleared if it was copied from parent, although get_default_style should handle defaults.
            // Resetting here is safe to ensure no unexpected inheritance.
            // (Note: defaults.container_style usually has the reset properties from input current_style, but get_default_style logic governs this)

            let mut element_type = defaults::ElementType::Container; // Default to container if not overridden
            if defaults.element_type != defaults::ElementType::Container {
                element_type = defaults.element_type;
            }

            let mut slider_value = 0.0;
            let mut progress_value = 0.0;
            let mut progress_max = 1.0;
            let mut checkbox_checked = false;
            let mut interaction_id: Option<String> = None;
            let mut image_src = String::new();
            let mut canvas_id = String::new();
            
            // 2. Parse Attributes
            for attr in attrs.borrow().iter() {
                let name = attr.name.local.as_ref();
                let value = &attr.value;
                
                match name {
                    "id" => {
                        canvas_id = value.to_string();
                    },
                    "style" => {
                        css::parse_inline_style(value, &mut current_style, &mut layout_style);
                    },
                    "type" if tag == "input" => {
                        if value.as_ref() == "checkbox" {
                            element_type = defaults::ElementType::Checkbox;
                            layout_style.size = Size { width: length(20.0), height: length(20.0) };
                            layout_style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
                        } else if value.as_ref() == "range" {
                            element_type = defaults::ElementType::Slider;
                            layout_style.size = Size { width: length(100.0), height: length(20.0) };
                        }
                    },

                    "value" => {
                        if let Ok(v) = value.parse::<f32>() {
                            slider_value = v.clamp(0.0, 1.0);
                            progress_value = v; 
                        }
                    },
                    "max" => {
                        if let Ok(v) = value.parse::<f32>() {
                            progress_max = v;
                        }
                    },
                    "checked" => checkbox_checked = true,
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
                     "src" => {
                         // Always capture src if present, mostly for images
                         image_src = value.to_string();
                     },
                     "data-on-click" => {
                         if !message_validator(value) {
                             log::warn!("Invalid message in data-on-click: {}", value);
                         }
                         interaction_id = Some(value.to_string());
                     }
                     _ => {
                         log::debug!("Ignoring attribute: {} on tag: {}", name, tag);
                     }
                }
            }
            
            // 3. Process Children (recurse if not a leaf element like img/input)
            let mut children = Vec::new();
            if element_type != defaults::ElementType::Image && element_type != defaults::ElementType::Checkbox  && element_type != defaults::ElementType::Slider && element_type != defaults::ElementType::Progress && element_type != defaults::ElementType::Canvas {
                for child in handle.children.borrow().iter() {
                     if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator) {
                         children.push(id);
                     }
                }
            }

            // 4. Create Taffy Node
            let id = taffy.new_with_children(layout_style, &children).ok()?;

            // 5. Store Render Data
            match element_type {
                defaults::ElementType::Image => {
                    render_data.insert(id, RenderData::Image(image_src, current_style));
                },
                defaults::ElementType::Checkbox => {
                    render_data.insert(id, RenderData::Checkbox(checkbox_checked, current_style));
                },
                defaults::ElementType::Slider => {
                    render_data.insert(id, RenderData::Slider(slider_value, current_style));
                },
                defaults::ElementType::Progress => {
                    render_data.insert(id, RenderData::Progress(progress_value, progress_max, current_style));
                },
                defaults::ElementType::Canvas => {
                    render_data.insert(id, RenderData::Canvas(canvas_id, current_style));
                },
                _ => {
                    render_data.insert(id, RenderData::Container(current_style));
                }

            }

             // 6. Register Interaction
            if let Some(interaction) = interaction_id {
                interactions.insert(id, interaction);
            }

            Some(id)
        },
        
        NodeData::Text { contents } => {
            let text = contents.borrow();
            // Normalize whitespace: collapse all whitespace sequences to a single space
            // and trim leading/trailing whitespace.
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
        // Shared background/border drawing logic for Containers and Text
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

        // Content Specific Drawing
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
            _ => {} // Container and others handled by shared logic or ignored
        }
    }

    // Handle Clipping for Overflow
    if overflow != Overflow::Visible {
        commands.push(DrawCommand::Clip { rect });
    }

    // Calculate Child Offsets (Scroll Handling)
    let mut child_offset_x = x;
    let mut child_offset_y = y;

    if overflow == Overflow::Scroll {
        if let Some((sx, sy)) = scroll_offsets.get(&root) {
             child_offset_x -= sx;
             child_offset_y -= sy;
        }
    }

    // Recurse to Children
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
