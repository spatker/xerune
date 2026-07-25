use crate::graphics::{Color, LinearGradient};
use crate::alloc_prelude::*;

/// Layout display type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    /// Render as a block-level container.
    Block,
    /// Render as an inline-block level container.
    InlineBlock,
    /// Render as a Flexbox layout container.
    Flex,
    /// Hide element and discard from layout generation.
    None,
}

/// Horizontal alignment of text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    /// Align text to the left boundary.
    Left,
    /// Center text.
    Center,
    /// Align text to the right boundary.
    Right,
}

/// Text direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Left-to-right text direction.
    Ltr,
    /// Right-to-left text direction.
    Rtl,
}

/// Bounding overflow behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    /// Overflow content is fully visible outside bounds.
    Visible,
    /// Overflow content is clipped.
    Hidden,
    /// Overflow content is clipped and allows scroll offsets.
    Scroll,
}

/// Writing mode orientation of text blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WritingMode {
    /// Horizontal top-to-bottom writing direction.
    HorizontalTb,
}


pub use taffy::prelude::{FlexDirection, FlexWrap, AlignContent, AlignItems, AlignSelf};

/// Position type of elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Position {
    /// Normal static flow position.
    Static,
    /// Relative offset position from its normal position.
    Relative,
    /// Absolute position relative to its closest positioned ancestor.
    Absolute,
}

/// Representation of CSS `justify-content` values mapping to layout engines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CssJustifyContent {
    /// Align items to the start of the line.
    FlexStart,
    /// Align items to the end of the line.
    FlexEnd,
    /// Align items to the center of the line.
    Center,
    /// Distribute space evenly between items.
    SpaceBetween,
    /// Distribute space evenly around items.
    SpaceAround,
    /// Distribute space evenly with equal margins.
    SpaceEvenly,
    /// Align to the start boundary.
    Start,
    /// Align to the end boundary.
    End,
    /// Align to the left boundary.
    Left,
    /// Align to the right boundary.
    Right,
}

pub use taffy::BoxSizing;

/// Iteration limit count of animations.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AnimationIterationCount {
    /// Infinite animation iterations.
    Infinite,
    /// Finite number of iterations.
    Count(f32),
}

/// Border style option.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BorderStyle {
    /// Solid line border.
    Solid,
    /// Dashed line border.
    Dashed,
    /// Dotted line border.
    Dotted,
    /// No border.
    None,
}

/// Representation of a CSS box shadow.
#[derive(Debug, Clone, PartialEq)]
pub struct BoxShadow {
    /// Horizontal offset in pixels.
    pub offset_x: f32,
    /// Vertical offset in pixels.
    pub offset_y: f32,
    /// Blur radius in pixels.
    pub blur_radius: f32,
    /// Spread radius in pixels.
    pub spread_radius: f32,
    /// Shadow color.
    pub color: Color,
    /// Inset shadow flag.
    pub inset: bool,
}

/// The resolved styling properties of an HTML container element.
#[derive(Debug, Clone)]
pub struct ContainerStyle {
    /// Foreground text color.
    pub color: Color,
    /// Font size in pixels.
    pub font_size: f32,
    /// Font weight (0 for Regular, 1 for Bold).
    pub weight: u16,
    /// Optional solid background fill color.
    pub background_color: Option<Color>,
    /// Border corner radius.
    pub border_radius: f32,
    /// Border stroke outline width.
    pub border_width: f32,
    /// Optional border stroke color.
    pub border_color: Option<Color>,
    /// Border line style.
    pub border_style: BorderStyle,
    /// Flag indicating whether only the bottom border should be rendered.
    pub border_bottom_only: bool,
    /// Optional CSS box shadow.
    pub box_shadow: Option<BoxShadow>,
    /// Optional linear background gradient.
    pub background_gradient: Option<LinearGradient>,
    /// Bounding box overflow behavior.
    pub overflow: Overflow,
    /// Layout display type.
    pub display: Display,
    /// Text horizontal alignment.
    pub text_align: Option<TextAlign>,
    /// Ordering index within a Flexbox layout.
    pub order: i32,
    /// Text writing direction.
    pub direction: Direction,
    /// Writing mode orientation.
    pub writing_mode: WritingMode,
    /// Flex direction layout.
    pub flex_direction: FlexDirection,
    /// Flex wrap behavior.
    pub flex_wrap: FlexWrap,
    /// CSS justify-content alignment.
    pub justify_content: Option<CssJustifyContent>,
    /// Flex alignment of line items.
    pub align_items: Option<AlignItems>,
    /// Optional fixed width.
    pub width: Option<f32>,
    /// Optional fixed height.
    pub height: Option<f32>,
    /// Left padding size.
    pub padding_left: f32,
    /// Right padding size.
    pub padding_right: f32,
    /// Top padding size.
    pub padding_top: f32,
    /// Bottom padding size.
    pub padding_bottom: f32,
    /// Layout inline size dimension constraint.
    pub inline_size: Option<taffy::style::Dimension>,
    /// Layout block size dimension constraint.
    pub block_size: Option<taffy::style::Dimension>,
    /// Minimum layout inline size.
    pub min_inline_size: Option<taffy::style::Dimension>,
    /// Maximum layout inline size.
    pub max_inline_size: Option<taffy::style::Dimension>,
    /// Minimum layout block size.
    pub min_block_size: Option<taffy::style::Dimension>,
    /// Maximum layout block size.
    pub max_block_size: Option<taffy::style::Dimension>,
    /// Flex alignment of the self container.
    pub align_self: Option<AlignSelf>,
    /// Position configuration.
    pub position: Position,
    /// Flags if this element has floated formatting.
    pub is_floated: bool,
    /// Box sizing policy.
    pub box_sizing: BoxSizing,
    /// Animation name.
    pub animation_name: Option<Arc<str>>,
    /// Animation duration in seconds.
    pub animation_duration: f32,
    /// Animation easing function name.
    pub animation_timing_function: Arc<str>,
    /// Animation startup delay in seconds.
    pub animation_delay: f32,
    /// Iteration count of the animation.
    pub animation_iteration_count: AnimationIterationCount,
    /// Direction direction of keyframes progression.
    pub animation_direction: Arc<str>,
    /// Animation fill mode.
    pub animation_fill_mode: Arc<str>,
    /// Animation play state control state.
    pub animation_play_state: Arc<str>,
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
            border_style: BorderStyle::Solid,
            border_bottom_only: false,
            box_shadow: None,
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
            animation_timing_function: Arc::from("ease"),
            animation_delay: 0.0,
            animation_iteration_count: AnimationIterationCount::Count(1.0),
            animation_direction: Arc::from("normal"),
            animation_fill_mode: Arc::from("none"),
            animation_play_state: Arc::from("running"),
        }
    }
}

/// Node representation containing resolved styling and DOM element metadata.
pub enum RenderData {
    /// Normal styled element container.
    Container(ContainerStyle),
    /// Styled text element.
    Text(String, ContainerStyle),
    /// Styled image element.
    Image(String, ContainerStyle),
    /// Styled checkbox control.
    Checkbox(bool, ContainerStyle),
    /// Styled slider control.
    Slider(f32, ContainerStyle),
    /// Styled progress bar control (value, max, style).
    Progress(f32, f32, ContainerStyle),
    /// Styled canvas viewport buffer mapping.
    Canvas(String, ContainerStyle),
    /// Styled text input field (id, text value, style).
    TextInput(String, Option<String>, ContainerStyle),
}

impl RenderData {
    /// Get a reference to the inner element style.
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
