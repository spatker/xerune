pub mod node_map;
pub mod metadata;
pub mod builder;
pub mod attributes;
pub mod style_resolution;

pub use node_map::{NodeMap, NodeMapIter, NodeMapValues};
pub use metadata::NodeMetadata;
pub use builder::UiBuilder;

use taffy::prelude::*;
use taffy::TaffyError;
use std::collections::HashMap;

#[cfg(feature = "dynamic-parser")]
use html5ever::parse_document;
#[cfg(feature = "dynamic-parser")]
use html5ever::tendril::TendrilSink;
#[cfg(feature = "dynamic-parser")]
use markup5ever_rcdom::{Handle as DomHandle, NodeData, RcDom};

#[cfg(feature = "dynamic-parser")]
pub type Handle = DomHandle;

#[cfg(not(feature = "dynamic-parser"))]
pub type Handle = ();

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Canvas, DrawCommand, Rect, TextMeasurer};
use crate::style::{ContainerStyle, Overflow, RenderData};
use crate::css;

pub type Interaction = String;

pub trait TemplateLayout {
    fn stylesheet(&self) -> &'static str;
    fn build_ui(&self, builder: &mut UiBuilder) -> NodeId;
}

pub struct Ui {
    pub taffy: TaffyTree,
    pub render_data: NodeMap<RenderData>,
    pub interactions: NodeMap<Interaction>,
    pub scroll_offsets: NodeMap<(f32, f32)>,
    pub root: NodeId,
    pub node_to_handle: NodeMap<Handle>,
    pub base_styles: NodeMap<(Style, ContainerStyle)>,
    pub keyframes: HashMap<String, css::KeyframesAnimation>,
}

impl Ui {
    #[cfg(feature = "dynamic-parser")]
    pub fn new(
        html: &str, 
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        profile!("ui_new_internal");
        let mut taffy = TaffyTree::new();
        let mut render_data = NodeMap::new();
        let mut interactions = NodeMap::new();
        let mut node_to_handle = NodeMap::new();
        let mut base_styles = NodeMap::new();

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        attributes::preprocess_dom(&dom.document);

        let mut css_str = String::new();
        attributes::extract_styles(&dom.document, &mut css_str);
        
        let keyframes = css::parse_keyframes(&css_str);
        
        let re_nth = regex::Regex::new(r":nth-child\(\s*(\d+)\s*\)").unwrap();
        let css_str = re_nth.replace_all(&css_str, ".nth-child-$1").into_owned();
        let css_str = css_str.replace(":last-child", ".last-child");
        let re_slash = regex::Regex::new(r"/[\d\.]+").unwrap();
        let css_str = re_slash.replace_all(&css_str, "").into_owned();
        
        let stylesheet = simplecss::StyleSheet::parse(&css_str);

        let root = attributes::dom_to_taffy(
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
            scroll_offsets: NodeMap::new(),
            root,
            node_to_handle,
            base_styles,
            keyframes,
        })
    }

    fn preprocess_compiled_tree(
        _taffy: &taffy::TaffyTree,
        node_metadata: &mut NodeMap<NodeMetadata>,
        node: NodeId,
    ) {
        let children = if let Some(meta) = node_metadata.get(&node) {
            if meta.tag == "#text" {
                return;
            }
            meta.children.clone()
        } else {
            return;
        };

        let mut child_elements = Vec::new();
        for child in children {
            if let Some(child_meta) = node_metadata.get(&child) {
                if child_meta.tag != "#text" {
                    child_elements.push(child);
                }
            }
            Self::preprocess_compiled_tree(_taffy, node_metadata, child);
        }

        let num_children = child_elements.len();
        for (i, child) in child_elements.iter().enumerate() {
            if let Some(child_meta) = node_metadata.get_mut(child) {
                let index_class = format!("nth-child-{}", i + 1);
                let is_last = i + 1 == num_children;
                
                // 1. Mutate optimized class field
                if let Some(ref mut class) = child_meta.class {
                    let class_str = class.to_mut();
                    class_str.push(' ');
                    class_str.push_str(&index_class);
                    if is_last {
                        class_str.push_str(" last-child");
                    }
                } else {
                    let mut class_val = index_class.clone();
                    if is_last {
                        class_val.push_str(" last-child");
                    }
                    child_meta.class = Some(std::borrow::Cow::Owned(class_val));
                }

                // 2. Mutate backward-compatible attrs field
                let mut found_class = false;
                for (k, v) in &mut child_meta.attrs {
                    if k == "class" {
                        v.push(' ');
                        v.push_str(&index_class);
                        if is_last {
                            v.push_str(" last-child");
                        }
                        found_class = true;
                        break;
                    }
                }
                if !found_class && !child_meta.attrs.is_empty() {
                    let mut class_val = index_class;
                    if is_last {
                        class_val.push_str(" last-child");
                    }
                    child_meta.attrs.push(("class".to_string(), class_val));
                }
            }
        }
    }

    pub fn new_compiled(
        model: &impl TemplateLayout,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        profile!("ui_new_compiled");
        let mut builder = UiBuilder::new();
        let root = {
            profile!("build_ui");
            model.build_ui(&mut builder)
        };

        let stylesheet_str = model.stylesheet();
        
        let cached = style_resolution::STYLESHEET_CACHE.with(|cache| {
            let mut cache_guard = cache.borrow_mut();
            if let Some(&c) = cache_guard.get(stylesheet_str) {
                c
            } else {
                let has_nth_or_last_child = stylesheet_str.contains(":nth-child") || stylesheet_str.contains(":last-child");
                let keyframes = css::parse_keyframes(stylesheet_str);
                
                let re_nth = regex::Regex::new(r":nth-child\(\s*(\d+)\s*\)").unwrap();
                let css_str = re_nth.replace_all(stylesheet_str, ".nth-child-$1").into_owned();
                let css_str = css_str.replace(":last-child", ".last-child");
                let re_slash = regex::Regex::new(r"/[\d\.]+").unwrap();
                let css_str = re_slash.replace_all(&css_str, "").into_owned();
                
                let static_css_str: &'static str = Box::leak(css_str.into_boxed_str());
                let stylesheet = simplecss::StyleSheet::parse(static_css_str);
                
                let cached_val: &'static style_resolution::CachedStyles = Box::leak(Box::new(style_resolution::CachedStyles {
                    stylesheet,
                    keyframes,
                    has_nth_or_last_child,
                    style_cache: std::cell::RefCell::new(HashMap::with_capacity(128)),
                }));
                cache_guard.insert(stylesheet_str, cached_val);
                cached_val
            }
        });

        if cached.has_nth_or_last_child {
            Self::preprocess_compiled_tree(&builder.taffy, &mut builder.node_metadata, root);
        }

        let mut base_styles = NodeMap::with_capacity(128);
        let mut style_cache = cached.style_cache.borrow_mut();
        
        {
            profile!("resolve_styles");
            style_resolution::resolve_styles(
                &mut builder.taffy,
                root,
                measurer,
                &mut builder.render_data,
                &mut builder.interactions,
                default_style,
                message_validator,
                &cached.stylesheet,
                &builder.node_metadata,
                &mut base_styles,
                &mut *style_cache,
            );
        }

        Ok(Self {
            taffy: builder.taffy,
            render_data: builder.render_data,
            interactions: builder.interactions,
            scroll_offsets: NodeMap::new(),
            root,
            node_to_handle: builder.node_to_handle,
            base_styles,
            keyframes: cached.keyframes.clone(),
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
        let node_opt = self.interactions.iter().find(|(_, v)| *v == interaction_id).map(|(k, _)| k);
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
             return Some((String::new(), clicked_node));
         }
         None
    }
}

pub(crate) fn normalize_text(text: &str) -> std::borrow::Cow<'_, str> {
    let mut needs_normalization = false;
    let mut last_was_space = false;
    let mut is_first = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if is_first || last_was_space || c != ' ' {
                needs_normalization = true;
                break;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        is_first = false;
    }
    if last_was_space && !text.is_empty() {
        needs_normalization = true;
    }
    if needs_normalization {
        std::borrow::Cow::Owned(text.split_whitespace().collect::<Vec<&str>>().join(" "))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

pub trait ToDisplayString {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str>;
}

impl ToDisplayString for str {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self)
    }
}

impl ToDisplayString for String {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self.as_str())
    }
}

impl<T: ToDisplayString + ?Sized> ToDisplayString for &T {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        (*self).to_display_string()
    }
}

impl ToDisplayString for f32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for f64 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for i32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for u32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for usize {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for bool {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(if *self { "true" } else { "false" })
    }
}

fn layout_to_draw_commands(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &NodeMap<RenderData>,
    scroll_offsets: &NodeMap<(f32, f32)>,
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
    render_data: &NodeMap<RenderData>,
    scroll_offsets: &NodeMap<(f32, f32)>,
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

                commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: if is_focused { focus_outline_width } else { style.border_width },
                    border_color: if is_focused { focus_outline_color } else { style.border_color },
                });

                if let Some(t) = text {
                    if !t.is_empty() {
                        let text_rect = Rect {
                            x: rect.x + 8.0,
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
    scroll_offsets: &NodeMap<(f32, f32)>,
    render_data: &NodeMap<RenderData>,
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
