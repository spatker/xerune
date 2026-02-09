use taffy::prelude::*;
use markup5ever_rcdom::{Handle, NodeData};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint};
use std::collections::HashMap;

pub type Interaction = String;

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

    pub fn from_id(id: u8) -> Self {
        let r = (id * 100) % 255;
        let g = (id * 50) % 255;
        let b = (id * 200) % 255;
        Self { r, g, b, a: 255 }
    }

     pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
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
    DrawRect { rect: Rect, color: Color },
    DrawText { 
        text: String, 
        x: f32, 
        y: f32, 
        color: Color, 
        font_size: f32 
    },
}

pub trait TextMeasurer {
    fn measure_text(&self, text: &str, font_size: f32) -> (f32, f32);
}

pub trait Renderer: TextMeasurer {
    fn render(&mut self, commands: &[DrawCommand]);
}

#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub color: Color,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
        }
    }
}

pub enum RenderData {
    Container,
    Text(String, TextStyle),
}

pub struct TinySkiaMeasurer<'a> {
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for TinySkiaMeasurer<'a> {
    fn measure_text(&self, text: &str, font_size: f32) -> (f32, f32) {
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        layout.reset(&fontdue::layout::LayoutSettings {
            ..fontdue::layout::LayoutSettings::default()
        });
        layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, font_size, 0));

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for glyph in layout.glyphs() {
            let gx = glyph.x;
            let gy = glyph.y;
            let gw = glyph.width as f32;
            let gh = glyph.height as f32;

            if gx < min_x { min_x = gx; }
            if gy < min_y { min_y = gy; }
            if gx + gw > max_x { max_x = gx + gw; }
            if gy + gh > max_y { max_y = gy + gh; }
        }

        let width = if max_x > min_x { max_x - min_x } else { 20.0 };
        let height = if max_y > min_y { max_y - min_y } else { 20.0 };
        (width, height)
    }
}

pub struct TinySkiaRenderer<'a> {
    pub pixmap: &'a mut Pixmap,
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for TinySkiaRenderer<'a> {
    fn measure_text(&self, text: &str, font_size: f32) -> (f32, f32) {
        let measurer = TinySkiaMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size)
    }
}

/// Parses the DOM tree and builds a Taffy layout tree.
pub fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: TextStyle,
) -> Option<NodeId> {
    let mut current_style = parent_style;
    
    // Check for class updates before processing children
    if let NodeData::Element { ref attrs, .. } = handle.data {
        for attr in attrs.borrow().iter() {
            if attr.name.local.as_ref() == "class" {
                let classes: Vec<&str> = attr.value.split_whitespace().collect();
                if classes.contains(&"completed") {
                    current_style.color = Color::from_rgba8(180, 180, 180, 255);
                }
            }
        }
    }

    let mut children = Vec::new();
    for child in handle.children.borrow().iter() {
        if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style) {
            children.push(id);
        }
    }

    // TODO parse style from attributes
    let style = Style::default();

    match handle.data {
        NodeData::Document => {
            let id = taffy.new_with_children(style, &children).ok()?;
            render_data.insert(id, RenderData::Container);
            Some(id)
        },

        NodeData::Element { ref attrs, .. } => {
            let id = taffy.new_with_children(style, &children).ok()?;
            render_data.insert(id, RenderData::Container);

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
                let (width, height) = text_measurer.measure_text(trimmed, 20.0);

                let style = Style {
                    size: Size {
                        width: length(width),
                        height: length(height),
                    },
                    ..Style::default()
                };

                let id = taffy.new_leaf(style).ok()?;
                render_data.insert(id, RenderData::Text(trimmed.to_string(), current_style));
                Some(id)
            }
        }

        _ => None,
    }
}

pub fn layout_to_draw_commands(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    offset_x: f32,
    offset_y: f32,
) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    traverse_layout(taffy, root, render_data, offset_x, offset_y, &mut commands);
    commands
}

fn traverse_layout(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
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

    if let Some(RenderData::Text(content, style)) = render_data.get(&root) {
        commands.push(DrawCommand::DrawText {
            text: content.clone(),
            x,
            y,
            color: style.color,
            font_size: 20.0,
        });
    }

    if let Ok(children) = taffy.children(root) {
        for child in children {
            traverse_layout(taffy, child, render_data, x, y, commands);
        }
    }
}

impl<'a> Renderer for TinySkiaRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand]) {
        for command in commands {
            match command {
                DrawCommand::DrawText { text, x, y, color, font_size } => {
                    let mut text_layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                    text_layout.reset(&fontdue::layout::LayoutSettings {
                        ..fontdue::layout::LayoutSettings::default()
                    });
                    text_layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, *font_size, 0));

                    let color_r = color.r;
                    let color_g = color.g;
                    let color_b = color.b;
                    let color_a = color.a;

                    for glyph in text_layout.glyphs() {
                        let (metrics, bitmap) = self.fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
                        
                        if metrics.width == 0 || metrics.height == 0 {
                            continue;
                        }

                        if let Some(mut glyph_pixmap) = Pixmap::new(metrics.width as u32, metrics.height as u32) {
                            let data = glyph_pixmap.data_mut();
                            
                            for (i, alpha) in bitmap.iter().enumerate() {
                                let a = (*alpha as f32 / 255.0) * (color_a as f32 / 255.0);
                                
                                // Premultiplied alpha
                                let r = (color_r as f32 * a) as u8;
                                let g = (color_g as f32 * a) as u8;
                                let b = (color_b as f32 * a) as u8;
                                let a_byte = (a * 255.0) as u8;

                                data[i*4 + 0] = r;
                                data[i*4 + 1] = g;
                                data[i*4 + 2] = b;
                                data[i*4 + 3] = a_byte;
                            }

                            let gx = x + glyph.x;
                            let gy = y + glyph.y;

                            self.pixmap.draw_pixmap(
                                gx as i32,
                                gy as i32,
                                glyph_pixmap.as_ref(),
                                &PixmapPaint::default(),
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                }
                DrawCommand::DrawRect { rect: _, color: _ } => {
                     // TODO: Implement DrawRect if needed, but wasn't in original code
                }
                _ => {}
            }
        }
    }
}

pub fn render_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    pixmap: &mut Pixmap,
    offset_x: f32,
    offset_y: f32,
    fonts: &[Font],
) {
    let mut renderer = TinySkiaRenderer { pixmap, fonts };
    let commands = layout_to_draw_commands(taffy, root, render_data, offset_x, offset_y);
    renderer.render(&commands);
}

pub fn hit_test(
    taffy: &TaffyTree,
    root: NodeId,
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
        if let Ok(children) = taffy.children(root) {
             for child in children.iter().rev() {
                 if let Some(hit) = hit_test(taffy, *child, x, y, left, top) {
                     return Some(hit);
                 }
             }
        }
        return Some(root);
    }
    None
}
