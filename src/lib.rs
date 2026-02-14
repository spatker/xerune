mod css;
mod defaults;

use taffy::prelude::*;
use taffy::TaffyError;
use markup5ever_rcdom::{Handle, NodeData, RcDom};
use html5ever::parse_document;
use html5ever::tendril::TendrilSink;

use std::collections::HashMap;
#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

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

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub dirty: bool,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
            dirty: true,
        }
    }
}

pub struct Context {
    pub canvases: HashMap<String, Canvas>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            canvases: HashMap::new(),
        }
    }
    
    pub fn canvas_mut(&mut self, id: &str) -> Option<&mut Canvas> {
        self.canvases.get_mut(id)
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

    DrawProgress {
        rect: Rect,
        value: f32,
        max: f32,
        color: Color,
    },
    DrawCanvas {
        id: String,
        rect: Rect,
    },
}

pub trait TextMeasurer {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32);
}

pub trait Renderer: TextMeasurer {
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>);
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
    Progress(f32, f32, TextStyle), // value, max, style
    Canvas(String, TextStyle),
}




pub trait Model {
    fn view(&self) -> String;
    fn update(&mut self, msg: &str, context: &mut Context);
}

pub enum InputEvent {
    Click { x: f32, y: f32 },
    Hover { x: f32, y: f32 },
    Scroll { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    Message(String),
}

pub struct Runtime<M, R> {
    model: M,
    measurer: R,
    ui: Ui,
    default_style: TextStyle,
    scroll_offsets: HashMap<NodeId, (f32, f32)>, // Persist scroll offsets
    cached_size: Size<AvailableSpace>,
    context: Context,
}

impl<M: Model, R: TextMeasurer> Runtime<M, R> {
    pub fn new(model: M, measurer: R) -> Self {
         let default_style = TextStyle::default();
         let html = model.view();
         let ui = Ui::new(&html, &measurer, default_style.clone()).unwrap();
         
         let mut context = Context::new();
         // Initial sync of canvases
         Runtime::<M, R>::sync_canvases(&ui, &mut context);

         Self {
             model,
             measurer,
             ui,
             default_style,
             scroll_offsets: HashMap::new(),
             cached_size: Size::MAX_CONTENT,
             context,
         }
    }

    fn sync_canvases(ui: &Ui, context: &mut Context) {
        for (node_id, data) in &ui.render_data {
            if let RenderData::Canvas(id, _style) = data {
                if !context.canvases.contains_key(id) {
                     // Try to get size from Taffy style (set by CSS or attributes)
                     let mut width = 200;
                     let mut height = 200;

                     if let Ok(style) = ui.taffy.style(*node_id) {
                         let w_dim = style.size.width;
                         if !w_dim.is_auto() {
                             let val = w_dim.value();
                             if w_dim == Dimension::length(val) {
                                  width = val as u32;
                             }
                         }

                         let h_dim = style.size.height;
                         if !h_dim.is_auto() {
                             let val = h_dim.value();
                             if h_dim == Dimension::length(val) {
                                 height = val as u32;
                             }
                         }
                     }

                     context.canvases.insert(id.clone(), Canvas::new(width, height));
                }
            }
        }
    }

    fn restore_scroll(&mut self) {
        self.ui.scroll_offsets = self.scroll_offsets.clone();
    }

    pub fn handle_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::Click { x, y } => {
                if let Some(msg) = self.ui.hit_test(x, y) {
                    {
                        profile!("update");
                        self.model.update(&msg, &mut self.context);
                    }
                    let html = {
                        profile!("view");
                        self.model.view()
                    };
                    // Recreate UI to reflect changes
                    self.ui = {
                        profile!("ui_new");
                        Ui::new(&html, &self.measurer, self.default_style.clone()).unwrap()
                    };
                    {
                        profile!("compute_layout");
                        let _ = self.ui.compute_layout(self.cached_size);
                    }
                    Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
                    self.restore_scroll();
                    return true;
                }
            }
            InputEvent::Message(msg) => {
                {
                    profile!("update");
                    self.model.update(&msg, &mut self.context);
                }
                let html = {
                    profile!("view");
                    self.model.view()
                };
                self.ui = {
                     profile!("ui_new");
                     Ui::new(&html, &self.measurer, self.default_style.clone()).unwrap()
                };
                {
                    profile!("compute_layout");
                    let _ = self.ui.compute_layout(self.cached_size);
                }
                Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
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
        profile!("render");
        self.ui.render(renderer, &self.context.canvases);
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

    pub fn render(&self, renderer: &mut impl Renderer, canvases: &HashMap<String, Canvas>) {
        let commands = layout_to_draw_commands(
            &self.taffy, 
            self.root, 
            &self.render_data, 
            &self.scroll_offsets,
            0.0, 
            0.0
        );
        renderer.render(&commands, canvases);
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
    parent_style: TextStyle,
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
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone()) {
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
            layout_style = defaults.style;
            current_style = defaults.text_style;
            
            // We ensure background is cleared if it was copied from parent, although get_default_style should handle defaults.
            // Resetting here is safe to ensure no unexpected inheritance.
            // (Note: defaults.text_style usually has the reset properties from input current_style, but get_default_style logic governs this)

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
                     if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone()) {
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
                    x,
                    y,
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
