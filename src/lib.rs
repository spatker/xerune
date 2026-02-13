mod css;
mod defaults;

use taffy::prelude::*;
use taffy::TaffyError;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;

use std::collections::HashMap;

pub type Interaction = String;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
}

#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: u8, 
    pub g: u8, 
    pub b: u8, 
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

     pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone)]
pub struct LinearGradient {
    pub angle: f32, // in degrees
    pub stops: Vec<(Color, f32)>, // Color and position (0.0 to 1.0)
}

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }
}

pub enum DrawCommand {
    Clip { rect: Rect },
    PopClip,
    DrawRect {
        rect: Rect,
        color: Option<Color>,
        gradient: Option<LinearGradient>,
        border_radius: f32,
        border_width: f32,
        border_color: Option<Color>,
    },
    DrawText { 
        text: String, 
        x: f32, 
        y: f32, 
        color: Color, 
        font_size: f32,
        weight: u16,
    },
    DrawImage {
        src: String,
        rect: Rect,
        border_radius: f32,
    },
    DrawCheckbox {
        rect: Rect,
        checked: bool,
        color: Color,
    },
    DrawSlider {
        rect: Rect,
        value: f32,
        color: Color,
    },
}

pub trait TextMeasurer {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32);
}

pub trait Renderer: TextMeasurer {
    fn render(&mut self, commands: &[DrawCommand]);
}

#[derive(Debug, Clone)]
pub struct TextStyle {
    pub color: Color,
    pub font_size: f32,
    pub weight: u16, // 0 = Regular, 1 = Bold
    pub background_color: Option<Color>,
    pub border_radius: f32,
    pub border_width: f32,
    pub border_color: Option<Color>,
    pub background_gradient: Option<LinearGradient>,
    pub overflow: Overflow,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: Color::from_rgba8(0, 0, 0, 255),
            font_size: 16.0,
            weight: 0,
            background_color: None,
            border_radius: 0.0,
            border_width: 0.0,

            border_color: None,
            background_gradient: None,
            overflow: Overflow::Visible,
        }
    }
}

pub enum RenderData {
    Container(TextStyle),
    Text(String, TextStyle),
    Image(String, TextStyle),
    Checkbox(bool, TextStyle),
    Slider(f32, TextStyle),
}




pub trait Model {
    fn view(&self) -> String;
    fn update(&mut self, msg: &str);
}

pub enum InputEvent {
    Click { x: f32, y: f32 },
    Hover { x: f32, y: f32 },
    Scroll { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    Tick,
}

pub struct Runtime<M, R> {
    model: M,
    measurer: R,
    ui: Ui,
    default_style: TextStyle,
    scroll_offsets: HashMap<NodeId, (f32, f32)>, // Persist scroll offsets
    cached_size: Size<AvailableSpace>,
}

impl<M: Model, R: TextMeasurer> Runtime<M, R> {
    pub fn new(model: M, measurer: R) -> Self {
         let default_style = TextStyle::default();
         let html = model.view();
         let ui = Ui::new(&html, &measurer, default_style.clone()).unwrap();
         Self {
             model,
             measurer,
             ui,
             default_style,
             scroll_offsets: HashMap::new(),
             cached_size: Size::MAX_CONTENT,
         }
    }

    fn restore_scroll(&mut self) {
        self.ui.scroll_offsets = self.scroll_offsets.clone();
    }

    pub fn handle_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::Click { x, y } => {
                if let Some(msg) = self.ui.hit_test(x, y) {
                    self.model.update(&msg);
                    let html = self.model.view();
                    // Recreate UI to reflect changes
                    self.ui = Ui::new(&html, &self.measurer, self.default_style.clone()).unwrap();
                    let _ = self.ui.compute_layout(self.cached_size);
                    self.restore_scroll();
                    return true;
                }
            }
            InputEvent::Tick => {
                self.model.update("tick");
                let html = self.model.view();
                self.ui = Ui::new(&html, &self.measurer, self.default_style.clone()).unwrap();
                let _ = self.ui.compute_layout(self.cached_size);
                self.restore_scroll();
                return true;
            }

            InputEvent::Scroll { x, y, delta_x, delta_y } => {
                if self.ui.handle_scroll(x, y, delta_x, delta_y) {
                    self.scroll_offsets = self.ui.scroll_offsets.clone();
                    return true;
                }
            }
             _ => {}
        }
        false
    }
    
    pub fn render(&self, renderer: &mut impl Renderer) {
        self.ui.render(renderer);
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
         let _ = self.ui.compute_layout(Size {
            width: length(width),
             height: length(height),
         });
    }

    pub fn compute_layout(&mut self, size: Size<AvailableSpace>) {
        self.cached_size = size;
        let _ = self.ui.compute_layout(size);
    }
    
    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.ui.scroll_into_view(interaction_id);
        self.scroll_offsets = self.ui.scroll_offsets.clone();
    }
}

pub struct Ui {

    taffy: TaffyTree,
    render_data: HashMap<NodeId, RenderData>,
    interactions: HashMap<NodeId, Interaction>,
    scroll_offsets: HashMap<NodeId, (f32, f32)>,
    root: NodeId,
}

impl Ui {
    pub fn new(
        html: &str, 
        measurer: &impl TextMeasurer,
        default_style: TextStyle,
    ) -> Result<Self, TaffyError> {
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
            default_style
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
// ...

    pub fn compute_layout(&mut self, available_space: Size<AvailableSpace>) -> Result<(), TaffyError> {
        self.taffy.compute_layout(self.root, available_space)
    }

    pub fn render(&self, renderer: &mut impl Renderer) {
        let commands = layout_to_draw_commands(
            &self.taffy, 
            self.root, 
            &self.render_data, 
            &self.scroll_offsets,
            0.0, 
            0.0
        );
        renderer.render(&commands);
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<Interaction> {
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




// Private helpers

fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: TextStyle,
) -> Option<NodeId> {
    
    // Default to handling as a container

    let mut checkbox_checked = false;
    let mut is_image = false;
    let mut is_checkbox = false;
    let mut is_slider = false;
    let mut slider_value = 0.0;
    
    let mut current_style = parent_style.clone();
    // Reset background color for children unless explicitly set, 
    // but inheriting color/font is correct.
    // Actually background color is NOT inherited in CSS.
    // But RenderData::Container uses current_style.
    // If we don't clear it, every child gets a background.
    // We should parse styles first, then decide.
    // We probably want to Copy inherited props (font, color) but Reset layout/background props.
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border_width = 0.0;
    current_style.border_radius = 0.0;
    // Reset properties
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border_width = 0.0;
    current_style.border_radius = 0.0;
    current_style.border_color = None;
    current_style.overflow = Overflow::Visible;

    let mut style = Style::default();

    if let NodeData::Element { ref name, ref attrs, .. } = handle.data {
        let tag = name.local.as_ref();
        
        // Get defaults
         let defaults = defaults::get_default_style(tag, &current_style); 
        style = defaults.style;
        current_style = defaults.text_style;
        // Background color is not inherited by default from parent_style in our simple model.
        // but get_default_style copies parent_style. Let's ensure background is None if not set.
        // Actually get_default_style copies everything including color/font/weight.
        // We probably want to reset background manually if defaults copied it (it didn't, parent passed in has it).
        // Wait, parent_style MIGHT have background. We should clear it.
        current_style.background_color = None;

        is_image = defaults.is_image;
        is_checkbox = defaults.is_checkbox;

        for attr in attrs.borrow().iter() {
            let name = attr.name.local.as_ref();
            let value = &attr.value;
            
            match name {
                "style" => {
                    css::parse_inline_style(value, &mut current_style, &mut style);
                },

                "type" if value.as_ref() == "checkbox" => {
                    if tag == "input" {
                        is_checkbox = true;
                        style.size = Size { width: length(20.0), height: length(20.0) };
                        style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
                    }
                },
                "type" if value.as_ref() == "range" => {
                    if tag == "input" {
                        is_slider = true;
                        // Default size for slider
                        style.size = Size { width: length(100.0), height: length(20.0) };
                    }
                },
                "value" => {
                    if let Ok(v) = value.parse::<f32>() {
                        slider_value = v.clamp(0.0, 1.0);
                    }
                },
                "checked" => checkbox_checked = true,
                "width" => {
                     if let Ok(w) = value.parse::<f32>() {
                         style.size.width = length(w);
                     }
                 },
                 "height" => {
                     if let Ok(h) = value.parse::<f32>() {
                         style.size.height = length(h);
                     }
                 },
                 _ => {}
            }
        }
    }

    let mut children = Vec::new();
    // Only process children if not a leaf-like element
    if !is_image && !is_checkbox && !is_slider {
        for child in handle.children.borrow().iter() {
             if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone()) {
                 children.push(id);
             }
        }
    }
    
    match handle.data {
        NodeData::Document => {
            let id = taffy.new_with_children(style, &children).ok()?;
            render_data.insert(id, RenderData::Container(current_style));
            Some(id)
        },

        NodeData::Element { ref attrs, .. } => {
            let id = taffy.new_with_children(style, &children).ok()?;
            
            if is_image {
                let image_src = attrs.borrow()
                    .iter()
                    .find(|attr| attr.name.local.as_ref() == "src")
                    .map(|attr| attr.value.as_ref().to_string())
                    .unwrap_or_default();
                
                render_data.insert(id, RenderData::Image(image_src, current_style));
            } else if is_checkbox {
                render_data.insert(id, RenderData::Checkbox(checkbox_checked, current_style));
            } else if is_slider {
                render_data.insert(id, RenderData::Slider(slider_value, current_style));
            } else {
                render_data.insert(id, RenderData::Container(current_style));
            }

            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == "data-on-click" {
                    interactions.insert(id, attr.value.to_string());
                }
            }

            Some(id)
        },

        NodeData::Text { ref contents } => {
            let text = contents.borrow();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                let (width, height) = text_measurer.measure_text(trimmed, current_style.font_size, current_style.weight);

                let text_style = Style {
                    size: Size {
                        width: length(width),
                        height: length(height),
                    },
                    ..Style::default()
                };

                let id = taffy.new_leaf(text_style).ok()?;
                render_data.insert(id, RenderData::Text(trimmed.to_string(), current_style));
                Some(id)
            }
        }

        _ => None,
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

    match render_data.get(&root) {
        Some(RenderData::Container(style)) => {
            overflow = style.overflow;
            if style.background_color.is_some() || style.background_gradient.is_some() || style.border_width > 0.0 {
                 commands.push(DrawCommand::DrawRect {
                    rect: Rect { x, y, width, height },
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: style.border_width,
                    border_color: style.border_color,
                });
            }
        },
        Some(RenderData::Text(text, style)) => {
            // Text background logic
             if style.background_color.is_some() || style.background_gradient.is_some() || style.border_width > 0.0 {
                 commands.push(DrawCommand::DrawRect {
                    rect: Rect { x, y, width, height },
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: style.border_width,
                    border_color: style.border_color,
                });
            }

            // Text centering is handled by Taffy layout engine (align-items: center).
            // We draw text at the Taffy-provided (x, y) coordinates.
            // Let's assume (x,y) is top-left.
            
            // Just use y directly. fontdue/skia_renderer expects top-left of the text bounding box logic mostly, 
            // or the layout engine handles centering. 
            // If we use fontdue::layout::CoordinateSystem::PositiveYDown, (0,0) is top-left of the layout box.
            // Taffy gives us the top-left of where the content should be.
            // If align-items is center, Taffy centers the content box within the parent.
            // So 'y' is the top of the text.
            
            commands.push(DrawCommand::DrawText {
                text: text.clone(),
                x,
                y, // Was y + style.font_size * 0.8
                color: style.color,
                font_size: style.font_size,
                weight: style.weight,
            });
        },
        Some(RenderData::Image(src, style)) => {
             commands.push(DrawCommand::DrawImage {
                src: src.clone(),
                rect: Rect { x, y, width, height },
                border_radius: style.border_radius,
            });
        },
        Some(RenderData::Checkbox(checked, style)) => {
            commands.push(DrawCommand::DrawCheckbox {
                rect: Rect { x, y, width, height },
                checked: *checked,
                color: style.color,
            });
        },
        Some(RenderData::Slider(value, style)) => {
             commands.push(DrawCommand::DrawSlider {
                rect: Rect { x, y, width, height },
                value: *value,
                color: style.color,
            });
        },
        _ => {}
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::Clip { rect: Rect { x, y, width, height } });
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
            // We pass 0.0 for offset because x, y calculated above includes offset_x/y
            // Wait, recursive call expects cumulative offset?
            // "offset_x + layout.location.x".
            // So we should pass 'child_offset_x' as the new 'offset_x' BASE?
            // "x" is absolute position of current node.
            // Child position = x + child_layout.x
            // So passing 'x' (or adjusted x) is correct.
            traverse_layout(taffy, child, render_data, scroll_offsets, child_offset_x, child_offset_y, commands);
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::PopClip);
    }
}



fn hit_test_recursive(
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
#[cfg(test)]
mod tests {
    use super::*;
    use taffy::prelude::TaffyMaxContent;

    struct MockModel;
    impl Model for MockModel {
        fn view(&self) -> String {
            // Use simple structure to ensure deterministic NodeIds
            r#"
            <div style="height: 100px; overflow: scroll;">
                <div style="height: 200px; flex-shrink: 0;" data-on-click="test_interaction">Content</div>
            </div>
            "#.to_string()
        }
        fn update(&mut self, _msg: &str) {}
    }

    struct MockMeasurer;
    impl TextMeasurer for MockMeasurer {
        fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (10.0, 10.0)
        }
    }

    #[test]
    fn test_scroll_persistence() {
        let model = MockModel;
        let measurer = MockMeasurer;
        let mut runtime = Runtime::new(model, measurer);
        
        // Initial layout
        runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);

        // Scroll
        let handled = runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -10.0 // Scroll down 10px
        });
        
        assert!(handled, "Scroll event should be handled");
        
        // Verify offset
        let offsets = &runtime.scroll_offsets;
        let offset = offsets.values().next().expect("Should have scroll offset");
        assert_eq!(offset.1, 10.0, "Offset should be 10.0 after first scroll");
        
        // Trigger UI Recreation via Tick
        runtime.handle_event(InputEvent::Tick); 
        
        // Verify persistence
        let offsets_after = &runtime.scroll_offsets;
        let offset_after = offsets_after.values().next().expect("Should have scroll offset after tick");
        assert_eq!(offset_after.1, 10.0, "Offset should persist after Tick/Ui Recreation");
        
        // Scroll more
        runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -10.0 
        });
        
        let offsets_final = &runtime.scroll_offsets;
        let offset_final = offsets_final.values().next().expect("Should have scroll offset");
        assert_eq!(offset_final.1, 20.0, "Offset should accumulate (10+10=20)");

        // Test Clamping (Content height 200, Container 100 -> Max scroll 100)
        // Try scrolling to 200
         runtime.handle_event(InputEvent::Scroll { 
            x: 10.0, y: 10.0, 
            delta_x: 0.0, delta_y: -500.0 // Big scroll down
        });
        
        // Should clamp to 100.0
        let offsets_clamped = &runtime.scroll_offsets;
        let offset_clamped = offsets_clamped.values().next().expect("Should have scroll offset");
        assert_eq!(offset_clamped.1, 100.0, "Offset should be clamped to max scroll (100.0)");

        // Test Hit Testing with Scroll
        // Content is at (0, 0) relative to container.
        // Container scroll is (0, 100).
        // Click at (10, 10) in window (container coords).
        // Should map to (10, 10 + 100) = (10, 110) in content.
        // Content height 200 via children.
        // So hitting child.
        
        // MockModel has data-on-click="test_interaction" on the child.
        // Hit test at (10, 10). Scroll is (0, 100).
        // Abs x=10, y=10.
        // Child abs pos = 0 - 0 = 0 (x), 0 - 100 = -100 (y).
        // Child rect = (0, -100, width?, 200).
        // y=10 is inside [-100, 100].
        // So it should hit.
        
        let hit = runtime.ui.hit_test(10.0, 10.0);
        assert!(hit.is_some(), "Should hit child content after scrolling");
        assert_eq!(hit.unwrap(), "test_interaction".to_string());
    }
}
