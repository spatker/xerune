use crate::graphics::{Color, Gradient};
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
    /// A Flexbox container placed like an inline-block.
    InlineFlex,
    /// Hide element and discard from layout generation.
    None,
}

impl Display {
    /// Lays out its children as a flex container (`flex`, `inline-flex`).
    pub fn is_flex(self) -> bool {
        matches!(self, Display::Flex | Display::InlineFlex)
    }

    /// Placed in its parent's line like a word (`inline-block`, `inline-flex`).
    pub fn is_inline_level(self) -> bool {
        matches!(self, Display::InlineBlock | Display::InlineFlex)
    }
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

/// CSS `line-height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeight {
    /// From the font's metrics.
    Normal,
    /// A multiple of the font size; inherited as the multiple.
    Number(f32),
    /// An absolute height in pixels.
    Length(f32),
    /// A percentage of the font size, as a fraction. Inherited as the
    /// resulting length (see [`LineHeight::inherited`]).
    Percent(f32),
}

impl LineHeight {
    /// The value a child inherits from an element with `font_size`:
    /// percentages compute to a length on the element that sets them.
    pub fn inherited(self, font_size: f32) -> Self {
        match self {
            LineHeight::Percent(p) => LineHeight::Length(p * font_size),
            other => other,
        }
    }

    /// Line height in pixels, given the font's `normal` line height.
    pub fn resolve(self, font_size: f32, normal: f32) -> f32 {
        match self {
            LineHeight::Normal => normal,
            LineHeight::Number(n) => n * font_size,
            LineHeight::Length(px) => px,
            LineHeight::Percent(p) => p * font_size,
        }
    }
}

/// CSS `white-space`: whether spaces and newlines collapse, and whether
/// lines wrap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WhiteSpace {
    /// Collapse whitespace, wrap lines.
    #[default]
    Normal,
    /// Collapse whitespace, don't wrap.
    Nowrap,
    /// Keep whitespace, break only at newlines.
    Pre,
    /// Keep whitespace, wrap lines.
    PreWrap,
    /// Collapse spaces but keep newlines, wrap lines.
    PreLine,
}

impl ContainerStyle {
    /// Where lines of this style's text may break inside words.
    pub fn word_break(&self) -> crate::text::WordBreak {
        if self.word_break_all {
            crate::text::WordBreak::BreakAll
        } else if self.word_keep_all {
            crate::text::WordBreak::KeepAll
        } else {
            self.overflow_wrap
        }
    }
}

impl WhiteSpace {
    /// Whether lines may break at soft wrap opportunities.
    pub fn wraps(self) -> bool {
        matches!(self, WhiteSpace::Normal | WhiteSpace::PreWrap | WhiteSpace::PreLine)
    }
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

/// One side of a border, as specified.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BorderSide {
    /// Specified width in pixels; see [`BorderSide::used_width`].
    pub width: f32,
    /// `None` is `currentColor`: the element's text color.
    pub color: Option<Color>,
    /// Line style.
    pub style: BorderStyle,
}

impl BorderSide {
    /// CSS initial values: `medium none currentColor`.
    pub const INITIAL: Self = Self { width: 3.0, color: None, style: BorderStyle::None };

    /// A solid border.
    pub const fn solid(width: f32, color: Color) -> Self {
        Self { width, color: Some(color), style: BorderStyle::Solid }
    }

    /// The width that takes up space and is painted: 0 when the style is `none`.
    pub fn used_width(&self) -> f32 {
        if self.style == BorderStyle::None { 0.0 } else { self.width }
    }
}

/// The four sides of a border.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Borders {
    /// Top side.
    pub top: BorderSide,
    /// Right side.
    pub right: BorderSide,
    /// Bottom side.
    pub bottom: BorderSide,
    /// Left side.
    pub left: BorderSide,
}

impl Borders {
    /// No border.
    pub const NONE: Self = Self::all(BorderSide::INITIAL);

    /// The same border on every side.
    pub const fn all(side: BorderSide) -> Self {
        Self { top: side, right: side, bottom: side, left: side }
    }

    /// Sides in CSS order: top, right, bottom, left.
    pub fn sides_mut(&mut self) -> [&mut BorderSide; 4] {
        [&mut self.top, &mut self.right, &mut self.bottom, &mut self.left]
    }

    /// Used widths of the four sides.
    pub fn widths(&self) -> taffy::geometry::Rect<f32> {
        taffy::geometry::Rect {
            left: self.left.used_width(),
            right: self.right.used_width(),
            top: self.top.used_width(),
            bottom: self.bottom.used_width(),
        }
    }

    /// Used width of the left and right sides together.
    pub fn horizontal_width(&self) -> f32 {
        self.left.used_width() + self.right.used_width()
    }

    /// Used width of the top and bottom sides together.
    pub fn vertical_width(&self) -> f32 {
        self.top.used_width() + self.bottom.used_width()
    }

    /// Whether any side is painted.
    pub fn is_visible(&self) -> bool {
        [self.top, self.right, self.bottom, self.left].iter().any(|s| s.used_width() > 0.0)
    }

    /// For painting: `none` sides get width 0 and `currentColor` becomes
    /// `text_color`.
    pub fn resolved(&self, text_color: Color) -> Self {
        let mut out = *self;
        for side in out.sides_mut() {
            side.width = side.used_width();
            side.color = Some(side.color.unwrap_or(text_color));
        }
        out
    }
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

pub use xerune_css::ControlPart;

/// A declaration for one of an element's [`ControlPart`]s.
#[derive(Debug, Clone, PartialEq)]
pub struct PartDeclaration {
    /// The part it styles.
    pub part: ControlPart,
    /// Property name.
    pub name: String,
    /// Property value.
    pub value: String,
}

/// A form control part with CSS, resolved like an element's style. Its
/// size and margins are in `layout`; sizes are border-box.
#[derive(Debug, Clone)]
pub struct PartStyle {
    /// Painted properties.
    pub style: ContainerStyle,
    /// Size and margins.
    pub layout: taffy::Style,
}

/// The parts of a range or progress bar that have CSS; the others keep
/// their native look.
#[derive(Debug, Clone, Default)]
pub struct ControlParts(pub Vec<(ControlPart, PartStyle)>);

impl ControlParts {
    /// The style of `part`, if it has CSS.
    pub fn get(&self, part: ControlPart) -> Option<&PartStyle> {
        self.0.iter().find(|(p, _)| *p == part).map(|(_, style)| style)
    }
}

/// The resolved styling properties of an HTML container element.
#[derive(Debug, Clone)]
pub struct ContainerStyle {
    /// Foreground text color.
    pub color: Color,
    /// Font size in pixels.
    pub font_size: f32,
    /// The parent's font size: what `em` means in `font-size` itself.
    pub parent_font_size: f32,
    /// The root element's font size, for `rem`.
    pub root_font_size: f32,
    /// Font weight (0 for Regular, 1 for Bold).
    pub weight: u16,
    /// Optional solid background fill color.
    pub background_color: Option<Color>,
    /// Border corner radius.
    pub border_radius: f32,
    /// Border of each side.
    pub border: Borders,
    /// Optional CSS box shadow.
    pub box_shadow: Option<BoxShadow>,
    /// Optional background gradient.
    pub background_gradient: Option<Gradient>,
    /// Bounding box overflow behavior.
    pub overflow: Overflow,
    /// Layout display type.
    pub display: Display,
    /// Text horizontal alignment.
    pub text_align: Option<TextAlign>,
    /// Height of a line of text.
    pub line_height: LineHeight,
    /// Whitespace collapsing and wrapping of text.
    pub white_space: WhiteSpace,
    /// `overflow-wrap`: `Normal`, `BreakWord` or `Anywhere`.
    pub overflow_wrap: crate::text::WordBreak,
    /// `word-break: break-all`.
    pub word_break_all: bool,
    /// `word-break: keep-all`: no breaks between letters.
    pub word_keep_all: bool,
    /// `text-overflow: ellipsis`. On a text node: its parent clips and
    /// asked for an ellipsis.
    pub text_overflow_ellipsis: bool,
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
    /// `z-index`; `None` is `auto`.
    pub z_index: Option<i32>,
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
    /// `transition-*`; `None` is the initial `all 0s`, which transitions
    /// nothing.
    pub transitions: Option<Arc<crate::css::transition::Transitions>>,
    /// `color-scheme: dark`: the default canvas color is dark instead of white.
    pub color_scheme_dark: bool,
    /// `accent-color` of form controls (inherited); `None` is `auto`.
    pub accent_color: Option<Color>,
    /// `appearance: none`: a form control is drawn as a plain CSS box
    /// instead of its native look.
    pub appearance_none: bool,
    /// Declarations for the element's form control parts, in cascade
    /// order (not inherited).
    pub part_declarations: Vec<PartDeclaration>,
}

impl Default for ContainerStyle {
    fn default() -> Self {
        Self {
            color: Color::from_rgba8(0, 0, 0, 255),
            font_size: 16.0,
            parent_font_size: 16.0,
            root_font_size: 16.0,
            weight: 0,
            background_color: None,
            border_radius: 0.0,
            border: Borders::NONE,
            box_shadow: None,
            background_gradient: None,
            overflow: Overflow::Visible,
            z_index: None,
            display: Display::Block,
            text_align: None,
            line_height: LineHeight::Normal,
            white_space: WhiteSpace::Normal,
            overflow_wrap: crate::text::WordBreak::Normal,
            word_break_all: false,
            word_keep_all: false,
            text_overflow_ellipsis: false,
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
            transitions: None,
            color_scheme_dark: false,
            accent_color: None,
            appearance_none: false,
            part_declarations: Vec::new(),
        }
    }
}

/// A node's resolved style and what it draws.
pub struct RenderData {
    /// The node's computed style.
    pub style: ContainerStyle,
    /// What the node is: a box, text or a replaced element or control.
    pub kind: RenderKind,
}

/// What a node draws besides its CSS box.
pub enum RenderKind {
    /// Normal element container.
    Container,
    /// A run of text.
    Text(String),
    /// An image, by source.
    Image(String),
    /// Checkbox control (checked).
    Checkbox(bool),
    /// Range input (position of the value from 0 to 1, styled parts).
    Slider(f32, Option<Box<ControlParts>>),
    /// Progress bar (value, max, styled parts).
    Progress(f32, f32, Option<Box<ControlParts>>),
    /// Canvas, by id.
    Canvas(String),
    /// Text input field (id, text value, placeholder text and its
    /// `::placeholder` style).
    TextInput(String, Option<String>, Option<Box<(String, ContainerStyle)>>),
}

impl RenderData {
    /// A `kind` node styled `style`.
    pub fn new(kind: RenderKind, style: ContainerStyle) -> Self {
        Self { style, kind }
    }

    /// The node's style if it is a plain container.
    pub fn container_style(&self) -> Option<&ContainerStyle> {
        matches!(self.kind, RenderKind::Container).then_some(&self.style)
    }
}
