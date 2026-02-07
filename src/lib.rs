use taffy::prelude::*;
use markup5ever_rcdom::{Handle, NodeData};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint, Color};
use std::collections::HashMap;

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

pub type Interaction = String;

/// Parses the DOM tree and builds a Taffy layout tree.
pub fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    fonts: &[Font],
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
        if let Some(id) = dom_to_taffy(taffy, child, fonts, render_data, interactions, current_style) {
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
                // Use PositiveYDown to match screen coordinates (Y goes down)
                let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                layout.reset(&fontdue::layout::LayoutSettings {
                    ..fontdue::layout::LayoutSettings::default()
                });
                layout.append(fonts, &fontdue::layout::TextStyle::new(trimmed, 20.0, 0));

                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let max_y = f32::MIN;

                for glyph in layout.glyphs() {
                    let gx = glyph.x;
                    let gy = glyph.y;
                    let gw = glyph.width as f32;
                    let gh = glyph.height as f32;

                    if gx < min_x { min_x = gx; }
                    if gy < min_y { min_y = gy; }
                    if gx + gw > max_x { max_x = gx + gw; }
                    if gy + gh > max_y { max_x = gx + gw; }
                }

                let width = if max_x > min_x { max_x - min_x } else { 20.0 }; // Default width if empty
                let height = if max_y > min_y { max_y - min_y } else { 20.0 };

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

pub fn render_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    pixmap: &mut Pixmap,
    offset_x: f32,
    offset_y: f32,
    fonts: &[Font],
) {
    let layout = taffy.layout(root).unwrap();
    let x = offset_x + layout.location.x;
    let y = offset_y + layout.location.y;

    if let Some(RenderData::Text(content, style)) = render_data.get(&root) {
        let mut text_layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        text_layout.reset(&fontdue::layout::LayoutSettings {
            ..fontdue::layout::LayoutSettings::default()
        });
        text_layout.append(fonts, &fontdue::layout::TextStyle::new(content, 20.0, 0));

        let color_u8_r = (style.color.red() * 255.0) as u8;
        let color_u8_g = (style.color.green() * 255.0) as u8;
        let color_u8_b = (style.color.blue() * 255.0) as u8;
        let color_u8_a = (style.color.alpha() * 255.0) as u8;

        for glyph in text_layout.glyphs() {
            let (metrics, bitmap) = fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
            
            if metrics.width == 0 || metrics.height == 0 {
                continue;
            }

            let mut glyph_pixmap = Pixmap::new(metrics.width as u32, metrics.height as u32).unwrap();
            let data = glyph_pixmap.data_mut();
            
            for (i, alpha) in bitmap.iter().enumerate() {
                let a = (*alpha as f32 / 255.0) * (color_u8_a as f32 / 255.0);
                
                // Premultiplied alpha
                let r = (color_u8_r as f32 * a) as u8;
                let g = (color_u8_g as f32 * a) as u8;
                let b = (color_u8_b as f32 * a) as u8;
                let a_byte = (a * 255.0) as u8;

                data[i*4 + 0] = r;
                data[i*4 + 1] = g;
                data[i*4 + 2] = b;
                data[i*4 + 3] = a_byte;
            }

            let gx = x + glyph.x;
            let gy = y + glyph.y;

            pixmap.draw_pixmap(
                gx as i32,
                gy as i32,
                glyph_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    if let Ok(children) = taffy.children(root) {
        for child in children {
            render_recursive(taffy, child, render_data, pixmap, x, y, fonts);
        }
    }
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
