use taffy::prelude::*;
use crate::{ContainerStyle, Display};
use crate::alloc_prelude::*;

/// Representation tags for UI element types mapping to native rendering primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    /// Normal element container.
    Container,
    /// Image display element.
    Image,
    /// Checkbox control.
    Checkbox,
    /// Slider control.
    Slider,
    /// Progress bar display.
    Progress,
    /// Custom canvas pixel buffer view.
    Canvas,
    /// Text input editor field.
    TextInput,
}

/// A structure bundling taffy layout Style, ContainerStyle and ElementType.
pub struct StyleBundle {
    /// The Taffy layout style mapping.
    pub taffy_style: Style,
    /// The ContainerStyle resolved styling.
    pub container_style: ContainerStyle,
    /// The ElementType native widget type.
    pub element_type: ElementType,
}

impl Default for StyleBundle {
    fn default() -> Self {
        Self {
            taffy_style: Style::default(),
            container_style: ContainerStyle::default(),
            element_type: ElementType::Container,
        }
    }
}

/// Retrieves the default StyleBundle for a given HTML tag name, resetting non-inherited properties.
pub fn get_default_style(tag: &str, parent_style: &ContainerStyle) -> StyleBundle {
    let mut bundle = StyleBundle::default();
    bundle.container_style = parent_style.clone();

    // Reset non-inherited CSS properties
    bundle.container_style.background_color = None;
    bundle.container_style.background_gradient = None;
    bundle.container_style.box_shadow = None;
    bundle.container_style.border_radius = 0.0;
    bundle.container_style.border_width = 0.0;
    bundle.container_style.border_color = None;
    bundle.container_style.border_bottom_only = false;
    bundle.container_style.overflow = crate::Overflow::Visible;
    bundle.container_style.order = 0;
    bundle.container_style.flex_direction = FlexDirection::Row;
    bundle.container_style.flex_wrap = FlexWrap::NoWrap;
    bundle.container_style.justify_content = None;
    bundle.container_style.align_items = None;
    bundle.container_style.width = None;
    bundle.container_style.height = None;
    bundle.container_style.padding_left = 0.0;
    bundle.container_style.padding_right = 0.0;
    bundle.container_style.padding_top = 0.0;
    bundle.container_style.padding_bottom = 0.0;
    bundle.container_style.inline_size = None;
    bundle.container_style.block_size = None;
    bundle.container_style.min_inline_size = None;
    bundle.container_style.max_inline_size = None;
    bundle.container_style.min_block_size = None;
    bundle.container_style.max_block_size = None;
    bundle.container_style.align_self = None;
    bundle.container_style.position = crate::style::Position::Static;
    bundle.container_style.animation_name = None;
    bundle.container_style.animation_duration = 0.0;
    bundle.container_style.animation_timing_function = Arc::from("ease");
    bundle.container_style.animation_delay = 0.0;
    bundle.container_style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
    bundle.container_style.animation_direction = Arc::from("normal");
    bundle.container_style.animation_fill_mode = Arc::from("none");
    bundle.container_style.animation_play_state = Arc::from("running");

    bundle.container_style.display = match tag {
        "tr" => Display::Flex,
        "div" | "body" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "li" | "table" | "tbody" | "thead" | "tfoot" => Display::Block,
        _ => Display::InlineBlock,
    };

    match tag {
        "h1" => {
            bundle.container_style.font_size = 32.0;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(20.0), bottom: length(20.0)
            };
        }
        "h2" => {
            bundle.container_style.font_size = 24.0;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(15.0), bottom: length(15.0)
            };
        }
        "p" => {
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(10.0), bottom: length(10.0)
            };
        }
        "ul" => {
            bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(20.0), right: length(0.0),
                top: length(0.0), bottom: length(0.0)
            };
        }
        "li" => {
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(2.0), bottom: length(2.0)
            };
        }
        "div" => {
        }
        "body" => {
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(8.0), right: length(8.0),
                top: length(8.0), bottom: length(8.0)
            };
        }
        "img" => {
            bundle.element_type = ElementType::Image;
            bundle.taffy_style.size = Size { width: length(100.0), height: length(100.0) };
        }
        "strong" | "b" => {
             bundle.container_style.weight = 1; // Bold
        }
        "checkbox" => {
            bundle.element_type = ElementType::Checkbox;
            bundle.taffy_style.size = Size { width: length(20.0), height: length(20.0) };
            bundle.taffy_style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
        }
        "h3" => {
            bundle.container_style.font_size = 20.0;
            bundle.container_style.weight = 1;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(12.0), bottom: length(12.0)
            };
        }
        "h4" => {
            bundle.container_style.font_size = 18.0;
            bundle.container_style.weight = 1;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(10.0), bottom: length(10.0)
            };
        }
        "h5" => {
            bundle.container_style.font_size = 16.0;
            bundle.container_style.weight = 1;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(8.0), bottom: length(8.0)
            };
        }
        "h6" => {
            bundle.container_style.font_size = 14.0;
            bundle.container_style.weight = 1;
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(6.0), bottom: length(6.0)
            };
        }
        "table" | "tbody" | "thead" | "tfoot" => {
             bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(2.0), right: length(2.0),
                top: length(2.0), bottom: length(2.0)
            };
        }
        "tr" => {
             bundle.taffy_style.flex_direction = FlexDirection::Row;
             bundle.taffy_style.size.width = Dimension::percent(1.0);
        }
        "td" => {
            bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(5.0), right: length(5.0),
                top: length(2.0), bottom: length(2.0)
            };
        }
        "th" => {
            bundle.container_style.weight = 1;
             bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(5.0), right: length(5.0),
                top: length(2.0), bottom: length(2.0)
            };
            bundle.taffy_style.align_items = Some(AlignItems::Center);
            bundle.taffy_style.justify_content = Some(JustifyContent::Center);
        }
        "button" => {
            bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(10.0), right: length(10.0),
                top: length(5.0), bottom: length(5.0)
            };
             bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(2.0), right: length(2.0),
                top: length(2.0), bottom: length(2.0)
            };
            bundle.taffy_style.align_items = Some(AlignItems::Center);
            bundle.container_style.background_color = Some(crate::Color::from_rgba8(220, 220, 220, 255));
            bundle.container_style.border_radius = 4.0;
            bundle.container_style.border_width = 1.0;
            bundle.container_style.border_color = Some(crate::Color::from_rgba8(180, 180, 180, 255));
        }
        "progress" => {
             bundle.element_type = ElementType::Progress;
             bundle.taffy_style.size = Size { width: length(150.0), height: length(20.0) };
        }
        "input_text" => {
            bundle.element_type = ElementType::TextInput;
            bundle.taffy_style.size = Size { width: length(150.0), height: length(30.0) };
            bundle.taffy_style.padding = taffy::geometry::Rect {
                left: length(8.0), right: length(8.0),
                top: length(5.0), bottom: length(5.0)
            };
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(2.0), right: length(2.0),
                top: length(2.0), bottom: length(2.0)
            };
            bundle.taffy_style.align_items = Some(AlignItems::Center);
            bundle.container_style.background_color = Some(crate::Color::WHITE);
            bundle.container_style.border_radius = 4.0;
            bundle.container_style.border_width = 1.0;
            bundle.container_style.border_color = Some(crate::Color::from_rgba8(200, 200, 200, 255));
        }
        "canvas" => {
            bundle.element_type = ElementType::Canvas;
            bundle.taffy_style.size = Size { width: length(200.0), height: length(200.0) };
        }
        "br" => {
            bundle.taffy_style.size.width = Dimension::percent(1.0);
            bundle.taffy_style.size.height = Dimension::length(0.0);
        }
        _ => {
            log::warn!("Unsupported tag encountered: {}", tag);
        }
    }
    
    bundle
}
