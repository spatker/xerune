use taffy::prelude::*;
use crate::style::{ContainerStyle, Overflow, RenderData, BoxSizing, Display, MyJustifyContent, TextAlign, Direction, AlignItems, AlignSelf, AlignContent};
use crate::graphics::TextMeasurer;
use crate::css;
use crate::defaults;
use super::node_map::NodeMap;
use super::metadata::NodeMetadata;
use super::Interaction;
use super::builder::TaffyElementWrapper;
use std::collections::HashMap;

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct StyleCacheKey {
    pub tag: std::borrow::Cow<'static, str>,
    pub class: Option<std::borrow::Cow<'static, str>>,
    pub id: Option<std::borrow::Cow<'static, str>>,
    pub other_attrs: Option<Vec<(String, String)>>,
    pub parent_font_size_bits: u32,
    pub parent_weight: u16,
    pub parent_color_u32: u32,
}

pub(crate) struct CachedStyles {
    pub(crate) stylesheet: simplecss::StyleSheet<'static>,
    pub(crate) keyframes: HashMap<String, css::KeyframesAnimation>,
    pub(crate) has_nth_or_last_child: bool,
    pub(crate) style_cache: std::cell::RefCell<HashMap<StyleCacheKey, (Style, ContainerStyle)>>,
}

thread_local! {
    pub(crate) static STYLESHEET_CACHE: std::cell::RefCell<HashMap<&'static str, &'static CachedStyles>> = std::cell::RefCell::new(HashMap::new());
    pub(crate) static CACHE_STATS: std::cell::Cell<(usize, usize)> = std::cell::Cell::new((0, 0));
}

pub(crate) fn to_border_box(dim: taffy::style::Dimension, add: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length(dim.value() + add)
    } else {
        dim
    }
}

pub(crate) fn to_content_box(dim: taffy::style::Dimension, sub: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length((dim.value() - sub).max(0.0))
    } else {
        dim
    }
}

pub(crate) fn resolve_styles(
    taffy: &mut TaffyTree,
    node: NodeId,
    text_measurer: &impl TextMeasurer,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &simplecss::StyleSheet<'_>,
    node_metadata: &NodeMap<NodeMetadata>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
    style_cache: &mut HashMap<StyleCacheKey, (Style, ContainerStyle)>,
) {
    let meta = match node_metadata.get(&node) {
        Some(m) => m,
        None => return,
    };

    if meta.tag == "#text" {
        let mut current_style = parent_style;
        current_style.background_color = None;
        current_style.background_gradient = None;
        current_style.border_width = 0.0;
        current_style.border_radius = 0.0;
        current_style.border_color = None;
        current_style.overflow = Overflow::Visible;
        current_style.animation_name = None;
        current_style.animation_duration = 0.0;
        current_style.animation_timing_function = std::sync::Arc::from("ease");
        current_style.animation_delay = 0.0;
        current_style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
        current_style.animation_direction = std::sync::Arc::from("normal");
        current_style.animation_fill_mode = std::sync::Arc::from("none");
        current_style.animation_play_state = std::sync::Arc::from("running");

        if let Some(ref text) = meta.text {
            let normalized = super::normalize_text(text);
            if !normalized.is_empty() {
                let (width, height) = text_measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                let text_layout_style = Style {
                    size: Size { width: length(width), height: length(height) },
                    ..Style::default()
                };
                let _ = taffy.set_style(node, text_layout_style.clone());
                render_data.insert(node, RenderData::Text(normalized.into_owned(), current_style.clone()));
                base_styles.insert(node, (text_layout_style, current_style));
            }
        }
        return;
    }

    let tag = &meta.tag;
    let element_type = defaults::get_default_style(tag, &ContainerStyle::default()).element_type;
    let mut parsed = super::attributes::ParsedAttributes::new(element_type);

    if let Some(checked) = meta.checked {
        parsed.checkbox_checked = checked;
    }
    if let Some(slider_value) = meta.slider_value {
        parsed.slider_value = slider_value;
    }
    if let Some(progress_value) = meta.progress_value {
        parsed.progress_value = progress_value;
    }
    if let Some(progress_max) = meta.progress_max {
        parsed.progress_max = progress_max;
    }
    if let Some(ref image_src) = meta.image_src {
        parsed.image_src = image_src.clone();
    }
    if let Some(ref canvas_id) = meta.canvas_id {
        parsed.canvas_id = canvas_id.clone();
    }
    if let Some(ref input_text) = meta.input_text {
        parsed.text_input_text = Some(input_text.clone());
    }

    let c = parent_style.color;
    let parent_color_u32 = ((c.r as u32) << 24) | ((c.g as u32) << 16) | ((c.b as u32) << 8) | (c.a as u32);
    let cache_key = StyleCacheKey {
        tag: meta.tag.clone(),
        class: meta.class.clone(),
        id: meta.id.clone(),
        other_attrs: meta.other_attrs.clone(),
        parent_font_size_bits: parent_style.font_size.to_bits(),
        parent_weight: parent_style.weight,
        parent_color_u32,
    };

    let (mut layout_style, mut current_style) = if let Some(cached_styles) = style_cache.get(&cache_key) {
        CACHE_STATS.with(|stats| {
            let (hits, misses) = stats.get();
            stats.set((hits + 1, misses));
        });
        cached_styles.clone()
    } else {
        CACHE_STATS.with(|stats| {
            let (hits, misses) = stats.get();
            stats.set((hits, misses + 1));
        });
        let defaults = defaults::get_default_style(tag, &parent_style);
        let mut l_style = defaults.taffy_style;
        let mut c_style = defaults.container_style;

        let el_wrapper = TaffyElementWrapper {
            node,
            taffy,
            metadata: node_metadata,
            meta,
        };
        for rule in &stylesheet.rules {
            if rule.selector.matches(&el_wrapper) {
                for decl in &rule.declarations {
                    let name_lower;
                    let name = if decl.name.chars().any(|c| c.is_ascii_uppercase()) {
                        name_lower = decl.name.to_lowercase();
                        &name_lower
                    } else {
                        decl.name
                    };
                    css::apply_declaration(name, decl.value, &mut c_style, &mut l_style);
                }
            }
        }
        let pair = (l_style, c_style);
        style_cache.insert(cache_key, pair.clone());
        pair
    };

    // 1. Process id attribute
    let id_str = meta.id.as_deref().or_else(|| {
        if !meta.attrs.is_empty() {
            for (k, v) in &meta.attrs {
                if k == "id" {
                    return Some(v.as_str());
                }
            }
        }
        None
    });
    if let Some(id_str) = id_str {
        parsed.canvas_id = id_str.to_string();
        parsed.element_id = Some(id_str.to_string());
    }

    // 2. Process inline styles
    let style_str = meta.style.as_deref().or_else(|| {
        if !meta.attrs.is_empty() {
            for (k, v) in &meta.attrs {
                if k == "style" {
                    return Some(v.as_str());
                }
            }
        }
        None
    });
    if let Some(style_str) = style_str {
        css::parse_inline_style(style_str, &mut current_style, &mut layout_style);
    }

    // 3. Process other attributes using generic parser
    if let Some(ref other) = meta.other_attrs {
        super::attributes::parse_attributes_generic(
            tag,
            other.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    } else if !meta.attrs.is_empty() {
        super::attributes::parse_attributes_generic(
            tag,
            meta.attrs.iter().filter(|(k, _)| k != "style" && k != "class" && k != "id").map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    }

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

    let children = &meta.children;
    for &child in children {
        resolve_styles(
            taffy,
            child,
            text_measurer,
            render_data,
            interactions,
            current_style.clone(),
            message_validator,
            stylesheet,
            node_metadata,
            base_styles,
            style_cache,
        );
    }

    if current_style.display == Display::Flex {
        let mut chs = meta.children.clone();
        let original = chs.clone();
        chs.sort_by_key(|child_id| {
            render_data.get(child_id).map(|data| data.style().order).unwrap_or(0)
        });
        if chs != original {
            let _ = taffy.set_children(node, &chs);
        }
    }

    if current_style.display == Display::None {
        layout_style.display = taffy::style::Display::None;
    } else if current_style.display != Display::Flex {
        let mut has_inline_child = false;
        let chs = &meta.children;
        for child_id in chs {
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

    let _ = taffy.set_style(node, layout_style.clone());

    super::attributes::process_element_type(node, &parsed, current_style.clone(), render_data);

    if let Some(interaction) = parsed.interaction_id {
        interactions.insert(node, interaction);
    }

    base_styles.insert(node, (layout_style, current_style));
}
