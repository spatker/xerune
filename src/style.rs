use crate::graphics::{Color, LinearGradient};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Block,
    InlineBlock,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    Visible,
    Hidden,
    Scroll,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritingMode {
    HorizontalTb,
}


pub use taffy::prelude::{FlexDirection, FlexWrap, AlignContent, AlignItems, AlignSelf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    Static,
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MyJustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Start,
    End,
    Left,
    Right,
}

pub use taffy::BoxSizing;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationIterationCount {
    Infinite,
    Count(f32),
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
    pub display: Display,
    pub text_align: Option<TextAlign>,
    pub order: i32,
    pub direction: Direction,
    pub writing_mode: WritingMode,
    pub flex_direction: FlexDirection,
    pub flex_wrap: FlexWrap,
    pub justify_content: Option<MyJustifyContent>,
    pub align_items: Option<AlignItems>,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub padding_left: f32,
    pub padding_right: f32,
    pub padding_top: f32,
    pub padding_bottom: f32,
    pub inline_size: Option<taffy::style::Dimension>,
    pub block_size: Option<taffy::style::Dimension>,
    pub min_inline_size: Option<taffy::style::Dimension>,
    pub max_inline_size: Option<taffy::style::Dimension>,
    pub min_block_size: Option<taffy::style::Dimension>,
    pub max_block_size: Option<taffy::style::Dimension>,
    pub align_self: Option<AlignSelf>,
    pub position: Position,
    pub is_floated: bool,
    pub box_sizing: BoxSizing,
    // Animation properties
    pub animation_name: Option<std::sync::Arc<str>>,
    pub animation_duration: f32, // in seconds
    pub animation_timing_function: std::sync::Arc<str>,
    pub animation_delay: f32, // in seconds
    pub animation_iteration_count: AnimationIterationCount,
    pub animation_direction: std::sync::Arc<str>,
    pub animation_fill_mode: std::sync::Arc<str>,
    pub animation_play_state: std::sync::Arc<str>,
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
            display: Display::Block,
            text_align: None,
            order: 0,
            direction: Direction::Ltr,
            writing_mode: WritingMode::HorizontalTb,
            flex_direction: FlexDirection::Row,
            flex_wrap: FlexWrap::NoWrap,
            justify_content: None,
            align_items: None,
            width: None,
            height: None,
            padding_left: 0.0,
            padding_right: 0.0,
            padding_top: 0.0,
            padding_bottom: 0.0,
            inline_size: None,
            block_size: None,
            min_inline_size: None,
            max_inline_size: None,
            min_block_size: None,
            max_block_size: None,
            align_self: None,
            position: Position::Static,
            is_floated: false,
            box_sizing: BoxSizing::ContentBox,
            animation_name: None,
            animation_duration: 0.0,
            animation_timing_function: std::sync::Arc::from("ease"),
            animation_delay: 0.0,
            animation_iteration_count: AnimationIterationCount::Count(1.0),
            animation_direction: std::sync::Arc::from("normal"),
            animation_fill_mode: std::sync::Arc::from("none"),
            animation_play_state: std::sync::Arc::from("running"),
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
    TextInput(String, Option<String>, ContainerStyle), // id, text value, style
}

impl RenderData {
    pub fn style(&self) -> &ContainerStyle {
        match self {
            RenderData::Container(style) => style,
            RenderData::Text(_, style) => style,
            RenderData::Image(_, style) => style,
            RenderData::Checkbox(_, style) => style,
            RenderData::Slider(_, style) => style,
            RenderData::Progress(_, _, style) => style,
            RenderData::Canvas(_, style) => style,
            RenderData::TextInput(_, _, style) => style,
        }
    }
}
