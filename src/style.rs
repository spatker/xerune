use crate::graphics::{Color, LinearGradient};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
}

#[derive(Debug, Clone)]
pub struct ContainerStyle {
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

impl Default for ContainerStyle {
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
    Container(ContainerStyle),
    Text(String, ContainerStyle),
    Image(String, ContainerStyle),
    Checkbox(bool, ContainerStyle),
    Slider(f32, ContainerStyle),
    Progress(f32, f32, ContainerStyle), // value, max, style
    Canvas(String, ContainerStyle),
}
