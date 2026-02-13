use taffy::prelude::*;
use crate::TextStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementType {
    Container,
    Image,
    Checkbox,
    Slider,
}

pub struct StyleBundle {
    pub style: Style,
    pub text_style: TextStyle,
    pub element_type: ElementType,
}

impl Default for StyleBundle {
    fn default() -> Self {
        Self {
            style: Style::default(),
            text_style: TextStyle::default(),
            element_type: ElementType::Container,
        }
    }
}

pub fn get_default_style(tag: &str, parent_style: &TextStyle) -> StyleBundle {
    let mut bundle = StyleBundle::default();
    bundle.text_style = parent_style.clone();

    match tag {
        "h1" => {
            bundle.text_style.font_size = 32.0;
            bundle.style.flex_direction = FlexDirection::Column;
            bundle.style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(20.0), bottom: length(20.0)
            };
        }
        "h2" => {
            bundle.text_style.font_size = 24.0;
            bundle.style.flex_direction = FlexDirection::Column;
            bundle.style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(15.0), bottom: length(15.0)
            };
        }
        "p" => {
            bundle.style.flex_direction = FlexDirection::Column;
            bundle.style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(10.0), bottom: length(10.0)
            };
        }
        "ul" => {
            bundle.style.flex_direction = FlexDirection::Column;
            bundle.style.padding = taffy::geometry::Rect {
                left: length(20.0), right: length(0.0),
                top: length(0.0), bottom: length(0.0)
            };
        }
        "li" => {
            bundle.style.flex_direction = FlexDirection::Column;
            bundle.style.margin = taffy::geometry::Rect {
                left: length(0.0), right: length(0.0),
                top: length(2.0), bottom: length(2.0)
            };
        }
        "div" | "body" => {
            bundle.style.flex_direction = FlexDirection::Column;
        }
        "img" => {
            bundle.element_type = ElementType::Image;
            bundle.style.size = Size { width: length(100.0), height: length(100.0) };
        }
        "strong" | "b" => {
             bundle.text_style.weight = 1; // Bold
        }
        "checkbox" => {
            bundle.element_type = ElementType::Checkbox;
            bundle.style.size = Size { width: length(20.0), height: length(20.0) };
            bundle.style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
        }
        _ => {}
    }
    
    bundle
}
