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

/// The [`get_default_style`] key for `<input type="...">`, if the type has
/// its own widget.
pub fn input_defaults_key(input_type: &str) -> Option<&'static str> {
    match input_type {
        "text" => Some("input_text"),
        "checkbox" => Some("input_checkbox"),
        "range" => Some("input_range"),
        _ => None,
    }
}

/// The [`get_default_style`] key of an element: an `<input>` by its `type`,
/// so CSS applied on top of the defaults isn't overwritten by them later.
/// An `<input>` without a `type` is a text input, as in HTML.
pub fn element_defaults_key<'a>(tag: &'a str, input_type: Option<&str>) -> &'a str {
    match input_defaults_key(input_type.unwrap_or("text")) {
        Some(key) if tag == "input" => key,
        _ => tag,
    }
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

    bundle.container_style.line_height = parent_style.line_height.inherited(parent_style.font_size);
    bundle.container_style.parent_font_size = parent_style.font_size;
    if tag == "body" {
        // `<body>` is the root's child: `rem` is the root's font size from here on.
        bundle.container_style.root_font_size = parent_style.font_size;
    }

    // Reset non-inherited CSS properties
    bundle.container_style.background_color = None;
    bundle.container_style.background_gradient = None;
    bundle.container_style.box_shadow = None;
    bundle.container_style.opacity = 1.0;
    bundle.container_style.border_radius = 0.0;
    bundle.container_style.border = crate::style::Borders::NONE;
    bundle.container_style.overflow = crate::Overflow::Visible;
    bundle.container_style.z_index = None;
    bundle.container_style.text_overflow_ellipsis = false;
    bundle.container_style.order = 0;
    bundle.container_style.appearance_none = false;
    bundle.container_style.part_declarations = Vec::new();
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
    bundle.container_style.animation_timing_function = crate::css::animation::Easing::EASE;
    bundle.container_style.animation_delay = 0.0;
    bundle.container_style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
    bundle.container_style.animation_direction = "normal";
    bundle.container_style.animation_fill_mode = "none";
    bundle.container_style.animation_play_state = "running";
    bundle.container_style.transitions = None;

    // The HTML rendering section's default styles (html.css in browsers).
    bundle.container_style.display = match tag {
        "tr" => Display::Flex,
        "html" | "body" | "div" | "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "hgroup"
        | "header" | "footer" | "main" | "nav" | "section" | "article" | "aside" | "search"
        | "address" | "blockquote" | "center" | "figure" | "figcaption" | "form" | "fieldset"
        | "legend" | "details" | "summary" | "dialog" | "hr" | "pre" | "listing" | "xmp"
        | "plaintext" | "ul" | "ol" | "menu" | "dir" | "li" | "dl" | "dt" | "dd"
        | "table" | "tbody" | "thead" | "tfoot" => Display::Block,
        _ => Display::InlineBlock,
    };

    let em = parent_style.font_size;
    let mut margins = |block: f32, inline: f32, font_size: f32| {
        bundle.taffy_style.margin = taffy::geometry::Rect {
            left: length(inline), right: length(inline),
            top: length(block * font_size), bottom: length(block * font_size),
        };
    };
    // Headings: font size relative to the parent, margins relative to the heading.
    let heading = match tag {
        "h1" => Some((2.0, 0.67)),
        "h2" => Some((1.5, 0.83)),
        "h3" => Some((1.17, 1.0)),
        "h4" => Some((1.0, 1.33)),
        "h5" => Some((0.83, 1.67)),
        "h6" => Some((0.67, 2.33)),
        _ => None,
    };
    if let Some((size, margin)) = heading {
        margins(margin, 0.0, size * em);
        bundle.container_style.font_size = size * em;
        bundle.container_style.weight = 1;
    }
    match tag {
        "p" | "dl" | "listing" | "plaintext" | "xmp" => margins(1.0, 0.0, em),
        "blockquote" | "figure" => margins(1.0, 40.0, em),
        "pre" => {
            margins(1.0, 0.0, em);
            bundle.container_style.white_space = crate::style::WhiteSpace::Pre;
        }
        "ul" | "ol" | "menu" | "dir" => {
            margins(1.0, 0.0, em);
            bundle.taffy_style.padding.left = length(40.0);
        }
        "dd" => bundle.taffy_style.margin.left = length(40.0),
        _ => {}
    }

    match tag {
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
            // No default size: the runtime applies a placeholder box until the image has
            // loaded and its natural size afterwards (see `Runtime::apply_image_sizes`).
        }
        "strong" | "b" => {
             bundle.container_style.weight = 1; // Bold
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
            bundle.container_style.border = crate::style::Borders::all(crate::style::BorderSide::solid(1.0, crate::Color::from_rgba8(180, 180, 180, 255)));
        }
        // Sizes and margins of form controls are Chromium's (html.css and
        // the controls' intrinsic sizes).
        "progress" => {
             bundle.element_type = ElementType::Progress;
             bundle.taffy_style.size = Size { width: length(10.0 * em), height: length(em) };
             bundle.container_style.box_sizing = crate::style::BoxSizing::BorderBox;
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
            bundle.container_style.border = crate::style::Borders::all(crate::style::BorderSide::solid(1.0, crate::Color::from_rgba8(200, 200, 200, 255)));
        }
        "input_checkbox" | "checkbox" => {
            bundle.element_type = ElementType::Checkbox;
            bundle.taffy_style.size = Size { width: length(13.0), height: length(13.0) };
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(4.0), right: length(3.0),
                top: length(3.0), bottom: length(3.0)
            };
            bundle.container_style.box_sizing = crate::style::BoxSizing::BorderBox;
        }
        "input_range" => {
            bundle.element_type = ElementType::Slider;
            bundle.taffy_style.size = Size { width: length(129.0), height: length(16.0) };
            bundle.taffy_style.margin = taffy::geometry::Rect {
                left: length(2.0), right: length(2.0),
                top: length(2.0), bottom: length(2.0)
            };
        }
        "canvas" => {
            bundle.element_type = ElementType::Canvas;
            bundle.taffy_style.size = Size { width: length(200.0), height: length(200.0) };
        }
        "br" => {
            bundle.taffy_style.size.width = Dimension::percent(1.0);
            bundle.taffy_style.size.height = Dimension::length(0.0);
        }
        // Generic containers: the defaults above are all they need.
        "html" | "span" => {}
        // Restyled from its `type` attribute in `ui::attributes`.
        "input" => {}
        _ => {
            crate::diag::warn_once("Unsupported tag encountered", tag);
        }
    }
    
    bundle
}
