pub mod parser;
pub mod animation;

pub use parser::{parse_hex_color, parse_px, parse_dimension, parse_length_percentage, parse_length_percentage_auto};
use parser::{parse_padding, parse_margin};
pub use animation::{Keyframe, KeyframesAnimation, parse_keyframes, parse_duration_sec, parse_animation_shorthand};

use crate::{Color, ContainerStyle, Display, TextAlign, Direction, WritingMode, FlexDirection, FlexWrap, AlignContent, AlignItems, MyJustifyContent, BoxSizing, style::AnimationIterationCount};
use taffy::prelude::*;
use taffy::style::Style;

struct FastLayoutStyle {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    bg_color: Color,
}

fn parse_layout_style_fast(s: &str) -> Option<FastLayoutStyle> {
    let bytes = s.as_bytes();
    let mut i = 0;
    
    if !s[i..].starts_with("left:") { return None; }
    i += 5;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_left = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let left = std::str::from_utf8(&bytes[start_left..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("top:") { return None; }
    i += 4;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_top = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let top = std::str::from_utf8(&bytes[start_top..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("width:") { return None; }
    i += 6;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_width = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let width = std::str::from_utf8(&bytes[start_width..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("height:") { return None; }
    i += 7;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_height = i;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.' || bytes[i] == b'-') { i += 1; }
    let height = std::str::from_utf8(&bytes[start_height..i]).ok()?.parse::<f32>().ok()?;
    
    if !s[i..].starts_with("px;") { return None; }
    i += 3;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    if !s[i..].starts_with("background-color:") { return None; }
    i += 17;
    while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') { i += 1; }
    
    let start_color = i;
    while i < bytes.len() && bytes[i] != b';' { i += 1; }
    let color_str = std::str::from_utf8(&bytes[start_color..i]).ok()?.trim();
    let bg_color = parser::parse_hex_color(color_str)?;
    
    Some(FastLayoutStyle {
        left,
        top,
        width,
        height,
        bg_color,
    })
}

pub fn parse_inline_style(style_str: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    if style_str.starts_with("left:") {
        if let Some(parsed) = parse_layout_style_fast(style_str) {
            taffy_style.inset.left = LengthPercentageAuto::length(parsed.left);
            taffy_style.inset.top = LengthPercentageAuto::length(parsed.top);
            taffy_style.size.width = Dimension::length(parsed.width);
            taffy_style.size.height = Dimension::length(parsed.height);
            current_style.background_color = Some(parsed.bg_color);
            return;
        }
    }

    let mut rest = style_str;
    while !rest.is_empty() {
        let decl;
        if let Some(semi_idx) = rest.find(';') {
            decl = &rest[..semi_idx];
            rest = &rest[semi_idx + 1..];
        } else {
            decl = rest;
            rest = "";
        }
        
        if let Some(colon_idx) = decl.find(':') {
            let key = decl[..colon_idx].trim();
            let val = decl[colon_idx + 1..].trim();
            if !key.is_empty() && !val.is_empty() {
                apply_declaration(key, val, current_style, taffy_style);
            }
        }
    }
}

pub fn apply_declaration(prop: &str, val: &str, current_style: &mut ContainerStyle, taffy_style: &mut Style) {
    match prop {
        "display" => {
            match val {
                "block" => current_style.display = Display::Block,
                "inline-block" | "inline" => current_style.display = Display::InlineBlock,
                "flex" => current_style.display = Display::Flex,
                "none" => current_style.display = Display::None,
                _ => {}
            }
        }
        "text-align" => {
            match val {
                "left" => current_style.text_align = Some(TextAlign::Left),
                "center" => current_style.text_align = Some(TextAlign::Center),
                "right" => current_style.text_align = Some(TextAlign::Right),
                _ => {}
            }
        }
        "color" => {
            if let Some(c) = parse_hex_color(val) {
                current_style.color = c;
            }
        }
        "background-color" => {
            if let Some(c) = parse_hex_color(val) {
                current_style.background_color = Some(c);
                current_style.background_gradient = None;
            }
        }
         "background" => {
             if val.contains("linear-gradient") {
                 if let Some(grad) = parser::parse_linear_gradient(val) {
                     current_style.background_gradient = Some(grad);
                     current_style.background_color = None;
                 }
             } else if let Some(c) = parse_hex_color(val) {
                current_style.background_color = Some(c);
                 current_style.background_gradient = None;
            }
        }
        "font-size" => {
            if let Some(size) = parse_px(val) {
                current_style.font_size = size;
            }
        }
        "font-weight" => {
            if val == "bold" || val == "700" || val == "800" || val == "900" {
                current_style.weight = 1;
            } else {
                current_style.weight = 0;
            }
        }
        "font" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                let subparts: Vec<&str> = part.split('/').collect();
                if let Some(size) = parse_px(subparts[0]) {
                    current_style.font_size = size;
                } else if subparts[0] == "bold" {
                    current_style.weight = 1;
                }
            }
        }
        "border-radius" => {
            if let Some(r) = parse_px(val) {
                current_style.border_radius = r;
            } else if val.ends_with("%") {
                 if val.trim() == "50%" {
                     current_style.border_radius = 9999.0;
                 }
            }
        }
        "border-width" => {
            if let Some(w) = parse_px(val) {
                current_style.border_width = w;
            }
        }
        "border-color" => {
            if let Some(c) = parse_hex_color(val) {
                current_style.border_color = Some(c);
            }
        }
        "border" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                if let Some(w) = parse_px(part) {
                    current_style.border_width = w;
                } else if let Some(c) = parse_hex_color(part) {
                     current_style.border_color = Some(c);
                }
            }
        }
        "padding" => {
            if let Some(p) = parse_padding(val) {
                taffy_style.padding = p;
            }
            let parts: Vec<&str> = val.split_whitespace().collect();
            match parts.len() {
                1 => {
                    if let Some(w) = parse_px(parts[0]) {
                        current_style.padding_left = w;
                        current_style.padding_right = w;
                        current_style.padding_top = w;
                        current_style.padding_bottom = w;
                    }
                }
                2 => {
                    let v = parse_px(parts[0]);
                    let h = parse_px(parts[1]);
                    if let Some(w) = v {
                        current_style.padding_top = w;
                        current_style.padding_bottom = w;
                    }
                    if let Some(w) = h {
                        current_style.padding_left = w;
                        current_style.padding_right = w;
                    }
                }
                4 => {
                    if let Some(w) = parse_px(parts[0]) { current_style.padding_top = w; }
                    if let Some(w) = parse_px(parts[1]) { current_style.padding_right = w; }
                    if let Some(w) = parse_px(parts[2]) { current_style.padding_bottom = w; }
                    if let Some(w) = parse_px(parts[3]) { current_style.padding_left = w; }
                }
                _ => {}
            }
        }
        "padding-left" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.left = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_left = w;
            }
        }
        "padding-right" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.right = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_right = w;
            }
        }
        "padding-top" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.top = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_top = w;
            }
        }
        "padding-bottom" => {
            if let Some(p) = parse_length_percentage(val) {
                taffy_style.padding.bottom = p;
            }
            if let Some(w) = parse_px(val) {
                current_style.padding_bottom = w;
            }
        }
        "margin" => {
            if let Some(m) = parse_margin(val) {
                taffy_style.margin = m;
            }
        }
        "margin-left" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.left = m;
            }
        }
        "margin-right" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.right = m;
            }
        }
        "margin-top" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.top = m;
            }
        }
        "margin-bottom" => {
            if let Some(m) = parse_length_percentage_auto(val) {
                taffy_style.margin.bottom = m;
            }
        }
        "width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.size.width = d;
            }
            if let Some(w) = parse_px(val) {
                current_style.width = Some(w);
            }
        }
        "height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.size.height = d;
            }
            if let Some(w) = parse_px(val) {
                current_style.height = Some(w);
            }
        }
        "min-width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.min_size.width = d;
            }
        }
        "min-height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.min_size.height = d;
            }
        }
        "max-width" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.max_size.width = d;
            }
        }
        "max-height" => {
            if let Some(d) = parse_dimension(val) {
                taffy_style.max_size.height = d;
            }
        }
        "inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.inline_size = Some(d);
            }
        }
        "block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.block_size = Some(d);
            }
        }
        "min-inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.min_inline_size = Some(d);
            }
        }
        "max-inline-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.max_inline_size = Some(d);
            }
        }
        "min-block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.min_block_size = Some(d);
            }
        }
        "max-block-size" => {
            if let Some(d) = parse_dimension(val) {
                current_style.max_block_size = Some(d);
            }
        }
         "flex-direction" => {
            match val {
                "row" => { taffy_style.flex_direction = FlexDirection::Row; current_style.flex_direction = FlexDirection::Row; }
                "column" => { taffy_style.flex_direction = FlexDirection::Column; current_style.flex_direction = FlexDirection::Column; }
                "row-reverse" => { taffy_style.flex_direction = FlexDirection::RowReverse; current_style.flex_direction = FlexDirection::RowReverse; }
                "column-reverse" => { taffy_style.flex_direction = FlexDirection::ColumnReverse; current_style.flex_direction = FlexDirection::ColumnReverse; }
                _ => {}
            }
        }
        "flex-wrap" => {
            match val {
                "nowrap" => { taffy_style.flex_wrap = FlexWrap::NoWrap; current_style.flex_wrap = FlexWrap::NoWrap; }
                "wrap" => { taffy_style.flex_wrap = FlexWrap::Wrap; current_style.flex_wrap = FlexWrap::Wrap; }
                "wrap-reverse" => { taffy_style.flex_wrap = FlexWrap::WrapReverse; current_style.flex_wrap = FlexWrap::WrapReverse; }
                _ => {}
            }
        }
        "flex-flow" => {
            let parts: Vec<&str> = val.split_whitespace().collect();
            for part in parts {
                match part {
                    "row" => { taffy_style.flex_direction = FlexDirection::Row; current_style.flex_direction = FlexDirection::Row; }
                    "column" => { taffy_style.flex_direction = FlexDirection::Column; current_style.flex_direction = FlexDirection::Column; }
                    "row-reverse" => { taffy_style.flex_direction = FlexDirection::RowReverse; current_style.flex_direction = FlexDirection::RowReverse; }
                    "column-reverse" => { taffy_style.flex_direction = FlexDirection::ColumnReverse; current_style.flex_direction = FlexDirection::ColumnReverse; }
                    "nowrap" => { taffy_style.flex_wrap = FlexWrap::NoWrap; current_style.flex_wrap = FlexWrap::NoWrap; }
                    "wrap" => { taffy_style.flex_wrap = FlexWrap::Wrap; current_style.flex_wrap = FlexWrap::Wrap; }
                    "wrap-reverse" => { taffy_style.flex_wrap = FlexWrap::WrapReverse; current_style.flex_wrap = FlexWrap::WrapReverse; }
                    _ => {}
                }
            }
        }
        "justify-content" => {
             match val {
                "flex-start" => {
                    taffy_style.justify_content = Some(AlignContent::FlexStart);
                    current_style.justify_content = Some(MyJustifyContent::FlexStart);
                }
                "flex-end" => {
                    taffy_style.justify_content = Some(AlignContent::FlexEnd);
                    current_style.justify_content = Some(MyJustifyContent::FlexEnd);
                }
                "center" => {
                    taffy_style.justify_content = Some(AlignContent::Center);
                    current_style.justify_content = Some(MyJustifyContent::Center);
                }
                "space-between" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceBetween);
                    current_style.justify_content = Some(MyJustifyContent::SpaceBetween);
                }
                "space-around" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceAround);
                    current_style.justify_content = Some(MyJustifyContent::SpaceAround);
                }
                "space-evenly" => {
                    taffy_style.justify_content = Some(AlignContent::SpaceEvenly);
                    current_style.justify_content = Some(MyJustifyContent::SpaceEvenly);
                }
                "start" => {
                    taffy_style.justify_content = Some(AlignContent::Start);
                    current_style.justify_content = Some(MyJustifyContent::Start);
                }
                "end" => {
                    taffy_style.justify_content = Some(AlignContent::End);
                    current_style.justify_content = Some(MyJustifyContent::End);
                }
                "left" => {
                    taffy_style.justify_content = Some(AlignContent::Start);
                    current_style.justify_content = Some(MyJustifyContent::Left);
                }
                "right" => {
                    taffy_style.justify_content = Some(AlignContent::End);
                    current_style.justify_content = Some(MyJustifyContent::Right);
                }
                _ => {}
            }
        }
         "align-items" => {
               match val {
                  "flex-start" => taffy_style.align_items = Some(AlignItems::FlexStart),
                  "flex-end" => taffy_style.align_items = Some(AlignItems::FlexEnd),
                  "center" => taffy_style.align_items = Some(AlignItems::Center),
                  "baseline" => taffy_style.align_items = Some(AlignItems::Baseline),
                  "stretch" => taffy_style.align_items = Some(AlignItems::Stretch),
                  "start" | "left" => taffy_style.align_items = Some(AlignItems::Start),
                  "end" | "right" => taffy_style.align_items = Some(AlignItems::End),
                  _ => {}
              }
              current_style.align_items = taffy_style.align_items;
          }
          "align-self" => {
               match val {
                  "flex-start" => taffy_style.align_self = Some(AlignSelf::FlexStart),
                  "flex-end" => taffy_style.align_self = Some(AlignSelf::FlexEnd),
                  "center" => taffy_style.align_self = Some(AlignSelf::Center),
                  "baseline" => taffy_style.align_self = Some(AlignSelf::Baseline),
                  "stretch" => taffy_style.align_self = Some(AlignSelf::Stretch),
                  "start" | "left" => taffy_style.align_self = Some(AlignSelf::Start),
                  "end" | "right" => taffy_style.align_self = Some(AlignSelf::End),
                  _ => {}
              }
              current_style.align_self = taffy_style.align_self;
          }
          "align-content" => {
               match val {
                  "flex-start" => taffy_style.align_content = Some(AlignContent::FlexStart),
                  "flex-end" => taffy_style.align_content = Some(AlignContent::FlexEnd),
                  "center" => taffy_style.align_content = Some(AlignContent::Center),
                  "space-between" => taffy_style.align_content = Some(AlignContent::SpaceBetween),
                  "space-around" => taffy_style.align_content = Some(AlignContent::SpaceAround),
                  "space-evenly" => taffy_style.align_content = Some(AlignContent::SpaceEvenly),
                  "stretch" => taffy_style.align_content = Some(AlignContent::Stretch),
                  "start" | "left" => taffy_style.align_content = Some(AlignContent::Start),
                  "end" | "right" => taffy_style.align_content = Some(AlignContent::End),
                  _ => {}
              }
          }
          "flex-grow" => {
               if let Ok(f) = val.parse::<f32>() {
                   taffy_style.flex_grow = f;
               }
          }
          "flex-shrink" => {
               if let Ok(f) = val.parse::<f32>() {
                   taffy_style.flex_shrink = f;
               }
          }
          "flex-basis" => {
              if let Some(d) = parse_dimension(val) {
                  taffy_style.flex_basis = d;
              } else if val == "auto" {
                  taffy_style.flex_basis = taffy::style::Dimension::auto();
              }
          }
          "flex" => {
              let parts: Vec<&str> = val.split_whitespace().collect();
              match parts.len() {
                  1 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = taffy::style::Dimension::percent(0.0);
                      } else if parts[0] == "auto" {
                          taffy_style.flex_grow = 1.0;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      } else if parts[0] == "none" {
                          taffy_style.flex_grow = 0.0;
                          taffy_style.flex_shrink = 0.0;
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      } else if let Some(d) = parse_dimension(parts[0]) {
                          taffy_style.flex_grow = 1.0;
                          taffy_style.flex_shrink = 1.0;
                          taffy_style.flex_basis = d;
                      }
                  }
                  2 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                          if let Ok(s) = parts[1].parse::<f32>() {
                              taffy_style.flex_shrink = s;
                              taffy_style.flex_basis = taffy::style::Dimension::percent(0.0);
                          } else if let Some(d) = parse_dimension(parts[1]) {
                              taffy_style.flex_shrink = 1.0;
                              taffy_style.flex_basis = d;
                          }
                      }
                  }
                  3 => {
                      if let Ok(g) = parts[0].parse::<f32>() {
                          taffy_style.flex_grow = g;
                      }
                      if let Ok(s) = parts[1].parse::<f32>() {
                          taffy_style.flex_shrink = s;
                      }
                      if let Some(d) = parse_dimension(parts[2]) {
                          taffy_style.flex_basis = d;
                      } else if parts[2] == "auto" {
                          taffy_style.flex_basis = taffy::style::Dimension::auto();
                      }
                  }
                  _ => {}
              }
          }
         "overflow" => {
             match val {
                 "hidden" => current_style.overflow = crate::Overflow::Hidden,
                 "scroll" => current_style.overflow = crate::Overflow::Scroll,
                 "auto" => current_style.overflow = crate::Overflow::Scroll,
                 "visible" => current_style.overflow = crate::Overflow::Visible,
                 _ => {}
             }
         }
         "order" => {
             if let Ok(v) = val.trim().parse::<i32>() {
                 current_style.order = v;
             }
         }
         "position" => {
             match val {
                 "absolute" => {
                     taffy_style.position = Position::Absolute;
                     current_style.position = crate::Position::Absolute;
                 }
                 "relative" => {
                     taffy_style.position = Position::Relative;
                     current_style.position = crate::Position::Relative;
                 }
                 "static" => {
                     taffy_style.position = Position::Relative;
                     current_style.position = crate::Position::Static;
                 }
                 _ => {}
             }
         }
          "left" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.left = v;
             }
         }
         "right" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.right = v;
             }
         }
         "top" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.top = v;
             }
         }
         "bottom" => {
             if let Some(v) = parse_length_percentage_auto(val) {
                 taffy_style.inset.bottom = v;
             }
         }
         "float" => {
             if val == "left" || val == "right" {
                 current_style.is_floated = true;
                 if current_style.display == Display::Block {
                     current_style.display = Display::InlineBlock;
                 }
             }
         }
         "box-sizing" => {
             match val {
                 "border-box" => current_style.box_sizing = BoxSizing::BorderBox,
                 "content-box" => current_style.box_sizing = BoxSizing::ContentBox,
                 _ => {}
             }
         }
         "row-gap" => {
             if let Some(lp) = parse_length_percentage(val) {
                 taffy_style.gap.height = lp;
             }
         }
         "column-gap" => {
             if let Some(lp) = parse_length_percentage(val) {
                 taffy_style.gap.width = lp;
             }
         }
         "gap" => {
             let parts: Vec<&str> = val.split_whitespace().collect();
             match parts.len() {
                 1 => {
                     if let Some(lp) = parse_length_percentage(parts[0]) {
                         taffy_style.gap.width = lp;
                         taffy_style.gap.height = lp;
                     }
                 }
                 2 => {
                     if let Some(lp_y) = parse_length_percentage(parts[0]) {
                         taffy_style.gap.height = lp_y;
                     }
                     if let Some(lp_x) = parse_length_percentage(parts[1]) {
                         taffy_style.gap.width = lp_x;
                     }
                 }
                 _ => {}
             }
         }
         "direction" => {
             match val {
                 "rtl" => current_style.direction = Direction::Rtl,
                 "ltr" => current_style.direction = Direction::Ltr,
                 _ => {}
             }
         }
         "writing-mode" => {
             match val {
                 "horizontal-tb" => current_style.writing_mode = WritingMode::HorizontalTb,
                 _ => {}
             }
         }
         "animation-name" => {
             current_style.animation_name = Some(std::sync::Arc::from(val.trim()));
         }
         "animation-duration" => {
             current_style.animation_duration = parse_duration_sec(val);
         }
         "animation-timing-function" => {
             current_style.animation_timing_function = std::sync::Arc::from(val.trim());
         }
         "animation-delay" => {
             current_style.animation_delay = parse_duration_sec(val);
         }
         "animation-iteration-count" => {
             current_style.animation_iteration_count = if val.trim() == "infinite" {
                 AnimationIterationCount::Infinite
             } else {
                 AnimationIterationCount::Count(val.trim().parse::<f32>().unwrap_or(1.0))
             };
         }
         "animation-direction" => {
             current_style.animation_direction = std::sync::Arc::from(val.trim().to_lowercase());
         }
         "animation-fill-mode" => {
             current_style.animation_fill_mode = std::sync::Arc::from(val.trim().to_lowercase());
         }
         "animation-play-state" => {
             current_style.animation_play_state = std::sync::Arc::from(val.trim().to_lowercase());
         }
         "animation" => {
             parse_animation_shorthand(val, current_style);
         }
         _ => {
             log::warn!("Unsupported CSS property: {}", prop);
         }
     }
}
