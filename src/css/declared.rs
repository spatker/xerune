//! Declarations parsed ahead of applying them.
//!
//! The derive emits a template's matched declarations as string pairs and
//! the view is rebuilt on every change, so parsing them on every build
//! (colors, lengths, gradients) was a large part of building a view.
//! [`Declarations`] parses the declarations of one place in a template
//! once, into [`Declared`] values, and applies those on every build.
//! [`Declared`] is also how the string path applies these properties, so
//! both give the same style.

use taffy::geometry::Rect;
use taffy::prelude::*;
use taffy::style::Style;

use super::parser::{self, parse_padding, parse_margin};
use super::{gradient, parse_dimension, parse_hex_color, parse_length_percentage, parse_length_percentage_auto, parse_opacity, parse_px};
use crate::alloc_prelude::*;
use crate::{Color, ContainerStyle, CssJustifyContent, Display, Gradient};

/// One side of a box.
#[derive(Clone, Copy, Debug)]
pub enum BoxSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl BoxSide {
    fn of<T>(self, rect: &mut Rect<T>) -> &mut T {
        match self {
            BoxSide::Top => &mut rect.top,
            BoxSide::Right => &mut rect.right,
            BoxSide::Bottom => &mut rect.bottom,
            BoxSide::Left => &mut rect.left,
        }
    }
}

/// A declaration of one of the common properties, parsed.
#[derive(Clone, Debug)]
pub enum Declared {
    /// A value the property doesn't take: the declaration does nothing.
    Nothing,
    Display(Display),
    Color(Color),
    /// `background-color`, or `background` with a color.
    BackgroundColor(Color),
    /// `background` / `background-image` with a gradient.
    BackgroundGradient(Gradient),
    /// `font-size` in `px`.
    FontSize(f32),
    /// `font-weight`: 1 for bold, else 0.
    FontWeight(u16),
    BorderRadius(f32),
    /// `border`, all sides.
    Border(crate::style::BorderSide),
    BoxShadow(Option<crate::style::BoxShadow>),
    /// `opacity`, clamped to 0..1.
    Opacity(f32),
    /// `padding`: the layout's, and the `px` sides the style keeps.
    Padding(Option<Rect<LengthPercentage>>, Option<Rect<f32>>),
    PaddingSide(BoxSide, Option<LengthPercentage>, Option<f32>),
    Margin(Rect<LengthPercentageAuto>),
    MarginSide(BoxSide, LengthPercentageAuto),
    /// `width`: the layout's, and the `px` width the style keeps.
    Width(Option<Dimension>, Option<f32>),
    Height(Option<Dimension>, Option<f32>),
    MinWidth(Dimension),
    MinHeight(Dimension),
    MaxWidth(Dimension),
    MaxHeight(Dimension),
    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    JustifyContent(AlignContent, CssJustifyContent),
    /// `align-items`; the style copies the layout's value even when the
    /// declaration's isn't one.
    AlignItems(Option<AlignItems>),
    AlignSelf(Option<AlignSelf>),
    AlignContent(AlignContent),
    FlexGrow(f32),
    FlexShrink(f32),
    FlexBasis(Dimension),
    Position(Position, crate::Position),
    Inset(BoxSide, LengthPercentageAuto),
    Overflow(crate::Overflow),
    /// `gap`: row gap (height) and column gap (width).
    Gap(Option<LengthPercentage>, Option<LengthPercentage>),
    ZIndex(Option<i32>),
    /// A declaration kept as written, applied as text: one of another
    /// property, or one whose value depends on where it applies (font
    /// relative or viewport lengths).
    Text(&'static str, &'static str),
}

impl Declared {
    /// `prop: val` parsed, if `prop` is one of the properties parsed here.
    pub(crate) fn parse(prop: &str, val: &str) -> Option<Self> {
        let side = |prop: &str, prefix: &str| -> Option<BoxSide> {
            Some(match prop.strip_prefix(prefix)? {
                "top" => BoxSide::Top,
                "right" => BoxSide::Right,
                "bottom" => BoxSide::Bottom,
                "left" => BoxSide::Left,
                _ => return None,
            })
        };
        let or_nothing = |declared: Option<Self>| Some(declared.unwrap_or(Declared::Nothing));
        match prop {
            "display" => or_nothing(
                match val {
                    "block" => Some(Display::Block),
                    "inline-block" | "inline" => Some(Display::InlineBlock),
                    "flex" => Some(Display::Flex),
                    "inline-flex" => Some(Display::InlineFlex),
                    "none" => Some(Display::None),
                    _ => None,
                }
                .map(Declared::Display),
            ),
            "color" => or_nothing(parse_hex_color(val).map(Declared::Color)),
            "background-color" => or_nothing(parse_hex_color(val).map(Declared::BackgroundColor)),
            "background" | "background-image" => or_nothing(if val.contains("gradient(") {
                gradient::parse_gradient(val).map(Declared::BackgroundGradient)
            } else {
                parse_hex_color(val).map(Declared::BackgroundColor)
            }),
            // A percentage is of the parent's size: applied as text.
            "font-size" => parse_px(val).map(Declared::FontSize),
            "font-weight" => Some(Declared::FontWeight(matches!(val, "bold" | "700" | "800" | "900") as u16)),
            "border-radius" => or_nothing(match parse_px(val) {
                Some(r) => Some(Declared::BorderRadius(r)),
                None => (val.ends_with('%') && val.trim() == "50%").then_some(Declared::BorderRadius(9999.0)),
            }),
            "border" => or_nothing(parser::parse_border_side(val).map(Declared::Border)),
            "box-shadow" => or_nothing(match parser::parse_box_shadow(val) {
                Some(shadow) => Some(Declared::BoxShadow(Some(shadow))),
                None => (val.trim() == "none").then_some(Declared::BoxShadow(None)),
            }),
            "opacity" => or_nothing(parse_opacity(val).map(Declared::Opacity)),
            "padding" => Some(Declared::Padding(parse_padding(val), parser::parse_sides(val, parse_px))),
            _ if prop.starts_with("padding-") => {
                Some(Declared::PaddingSide(side(prop, "padding-")?, parse_length_percentage(val), parse_px(val)))
            }
            "margin" => or_nothing(parse_margin(val).map(Declared::Margin)),
            _ if prop.starts_with("margin-") => {
                let side = side(prop, "margin-")?;
                or_nothing(parse_length_percentage_auto(val).map(|m| Declared::MarginSide(side, m)))
            }
            "width" => Some(Declared::Width(parse_dimension(val), parse_px(val))),
            "height" => Some(Declared::Height(parse_dimension(val), parse_px(val))),
            "min-width" => or_nothing(parse_dimension(val).map(Declared::MinWidth)),
            "min-height" => or_nothing(parse_dimension(val).map(Declared::MinHeight)),
            "max-width" => or_nothing(parse_dimension(val).map(Declared::MaxWidth)),
            "max-height" => or_nothing(parse_dimension(val).map(Declared::MaxHeight)),
            "flex-direction" => or_nothing(
                match val {
                    "row" => Some(FlexDirection::Row),
                    "column" => Some(FlexDirection::Column),
                    "row-reverse" => Some(FlexDirection::RowReverse),
                    "column-reverse" => Some(FlexDirection::ColumnReverse),
                    _ => None,
                }
                .map(Declared::FlexDirection),
            ),
            "flex-wrap" => or_nothing(
                match val {
                    "nowrap" => Some(FlexWrap::NoWrap),
                    "wrap" => Some(FlexWrap::Wrap),
                    "wrap-reverse" => Some(FlexWrap::WrapReverse),
                    _ => None,
                }
                .map(Declared::FlexWrap),
            ),
            "justify-content" => or_nothing(
                match val {
                    "flex-start" => Some((AlignContent::FlexStart, CssJustifyContent::FlexStart)),
                    "flex-end" => Some((AlignContent::FlexEnd, CssJustifyContent::FlexEnd)),
                    "center" => Some((AlignContent::Center, CssJustifyContent::Center)),
                    "space-between" => Some((AlignContent::SpaceBetween, CssJustifyContent::SpaceBetween)),
                    "space-around" => Some((AlignContent::SpaceAround, CssJustifyContent::SpaceAround)),
                    "space-evenly" => Some((AlignContent::SpaceEvenly, CssJustifyContent::SpaceEvenly)),
                    "start" => Some((AlignContent::Start, CssJustifyContent::Start)),
                    "end" => Some((AlignContent::End, CssJustifyContent::End)),
                    "left" => Some((AlignContent::Start, CssJustifyContent::Left)),
                    "right" => Some((AlignContent::End, CssJustifyContent::Right)),
                    _ => None,
                }
                .map(|(layout, style)| Declared::JustifyContent(layout, style)),
            ),
            "align-items" => Some(Declared::AlignItems(match val {
                "flex-start" => Some(AlignItems::FlexStart),
                "flex-end" => Some(AlignItems::FlexEnd),
                "center" => Some(AlignItems::Center),
                "baseline" => Some(AlignItems::Baseline),
                "stretch" => Some(AlignItems::Stretch),
                "start" | "left" => Some(AlignItems::Start),
                "end" | "right" => Some(AlignItems::End),
                _ => None,
            })),
            "align-self" => Some(Declared::AlignSelf(match val {
                "flex-start" => Some(AlignSelf::FlexStart),
                "flex-end" => Some(AlignSelf::FlexEnd),
                "center" => Some(AlignSelf::Center),
                "baseline" => Some(AlignSelf::Baseline),
                "stretch" => Some(AlignSelf::Stretch),
                "start" | "left" => Some(AlignSelf::Start),
                "end" | "right" => Some(AlignSelf::End),
                _ => None,
            })),
            "align-content" => or_nothing(
                match val {
                    "flex-start" => Some(AlignContent::FlexStart),
                    "flex-end" => Some(AlignContent::FlexEnd),
                    "center" => Some(AlignContent::Center),
                    "space-between" => Some(AlignContent::SpaceBetween),
                    "space-around" => Some(AlignContent::SpaceAround),
                    "space-evenly" => Some(AlignContent::SpaceEvenly),
                    "stretch" => Some(AlignContent::Stretch),
                    "start" | "left" => Some(AlignContent::Start),
                    "end" | "right" => Some(AlignContent::End),
                    _ => None,
                }
                .map(Declared::AlignContent),
            ),
            "flex-grow" => or_nothing(val.parse::<f32>().ok().map(Declared::FlexGrow)),
            "flex-shrink" => or_nothing(val.parse::<f32>().ok().map(Declared::FlexShrink)),
            "flex-basis" => or_nothing(match parse_dimension(val) {
                Some(d) => Some(Declared::FlexBasis(d)),
                None => (val == "auto").then(|| Declared::FlexBasis(Dimension::auto())),
            }),
            "position" => or_nothing(match val {
                "absolute" => Some(Declared::Position(Position::Absolute, crate::Position::Absolute)),
                "relative" => Some(Declared::Position(Position::Relative, crate::Position::Relative)),
                "static" => Some(Declared::Position(Position::Relative, crate::Position::Static)),
                _ => None,
            }),
            "top" | "right" | "bottom" | "left" => {
                let side = side(prop, "")?;
                or_nothing(parse_length_percentage_auto(val).map(|v| Declared::Inset(side, v)))
            }
            "overflow" => or_nothing(
                match val {
                    // `clip` also forbids programmatic scrolling, which Xerune has none of.
                    "hidden" | "clip" => Some(crate::Overflow::Hidden),
                    "scroll" | "auto" => Some(crate::Overflow::Scroll),
                    "visible" => Some(crate::Overflow::Visible),
                    _ => None,
                }
                .map(Declared::Overflow),
            ),
            "gap" => {
                let parts = val.split_whitespace().collect::<Vec<_>>();
                Some(match parts[..] {
                    [both] => match parse_length_percentage(both) {
                        Some(lp) => Declared::Gap(Some(lp), Some(lp)),
                        None => Declared::Nothing,
                    },
                    [row, column] => Declared::Gap(parse_length_percentage(row), parse_length_percentage(column)),
                    _ => Declared::Nothing,
                })
            }
            "z-index" => or_nothing(if val == "auto" {
                Some(Declared::ZIndex(None))
            } else {
                val.parse::<i32>().ok().map(|z| Declared::ZIndex(Some(z)))
            }),
            _ => None,
        }
    }

    /// Sets the style the declaration gives.
    pub(crate) fn apply(&self, style: &mut ContainerStyle, layout: &mut Style) {
        match self {
            Declared::Nothing => {}
            &Declared::Display(display) => style.display = display,
            &Declared::Color(color) => style.color = color,
            &Declared::BackgroundColor(color) => {
                style.background_color = Some(color);
                style.background_gradient = None;
            }
            Declared::BackgroundGradient(gradient) => {
                style.background_gradient = Some(gradient.clone());
                style.background_color = None;
            }
            &Declared::FontSize(size) => style.font_size = size,
            &Declared::FontWeight(weight) => style.weight = weight,
            &Declared::BorderRadius(radius) => style.border_radius = radius,
            &Declared::Border(side) => style.border = crate::style::Borders::all(side),
            Declared::BoxShadow(shadow) => style.box_shadow = shadow.clone(),
            &Declared::Opacity(opacity) => style.opacity = opacity,
            &Declared::Padding(sides, px) => {
                if let Some(sides) = sides {
                    layout.padding = sides;
                }
                if let Some(px) = px {
                    (style.padding_top, style.padding_right, style.padding_bottom, style.padding_left) = (px.top, px.right, px.bottom, px.left);
                }
            }
            &Declared::PaddingSide(side, padding, px) => {
                if let Some(padding) = padding {
                    *side.of(&mut layout.padding) = padding;
                }
                if let Some(px) = px {
                    *match side {
                        BoxSide::Top => &mut style.padding_top,
                        BoxSide::Right => &mut style.padding_right,
                        BoxSide::Bottom => &mut style.padding_bottom,
                        BoxSide::Left => &mut style.padding_left,
                    } = px;
                }
            }
            &Declared::Margin(margin) => layout.margin = margin,
            &Declared::MarginSide(side, margin) => *side.of(&mut layout.margin) = margin,
            &Declared::Width(size, px) => {
                if let Some(size) = size {
                    layout.size.width = size;
                }
                if let Some(px) = px {
                    style.width = Some(px);
                }
            }
            &Declared::Height(size, px) => {
                if let Some(size) = size {
                    layout.size.height = size;
                }
                if let Some(px) = px {
                    style.height = Some(px);
                }
            }
            &Declared::MinWidth(d) => layout.min_size.width = d,
            &Declared::MinHeight(d) => layout.min_size.height = d,
            &Declared::MaxWidth(d) => layout.max_size.width = d,
            &Declared::MaxHeight(d) => layout.max_size.height = d,
            &Declared::FlexDirection(direction) => (layout.flex_direction, style.flex_direction) = (direction, direction),
            &Declared::FlexWrap(wrap) => (layout.flex_wrap, style.flex_wrap) = (wrap, wrap),
            &Declared::JustifyContent(content, justify) => {
                layout.justify_content = Some(content);
                style.justify_content = Some(justify);
            }
            &Declared::AlignItems(align) => {
                if align.is_some() {
                    layout.align_items = align;
                }
                style.align_items = layout.align_items;
            }
            &Declared::AlignSelf(align) => {
                if align.is_some() {
                    layout.align_self = align;
                }
                style.align_self = layout.align_self;
            }
            &Declared::AlignContent(align) => layout.align_content = Some(align),
            &Declared::FlexGrow(grow) => layout.flex_grow = grow,
            &Declared::FlexShrink(shrink) => layout.flex_shrink = shrink,
            &Declared::FlexBasis(basis) => layout.flex_basis = basis,
            &Declared::Position(position, css_position) => (layout.position, style.position) = (position, css_position),
            &Declared::Inset(side, inset) => *side.of(&mut layout.inset) = inset,
            &Declared::Overflow(overflow) => style.overflow = overflow,
            &Declared::Gap(row, column) => {
                if let Some(row) = row {
                    layout.gap.height = row;
                }
                if let Some(column) = column {
                    layout.gap.width = column;
                }
            }
            &Declared::ZIndex(z) => style.z_index = z,
            &Declared::Text(prop, val) => super::apply_declaration(prop, val, style, layout),
        }
    }
}

/// The declarations a template applies at one place, as the derive emits
/// them: kept as written, parsed on first use.
pub struct Declarations {
    source: &'static [(&'static str, &'static str)],
    #[cfg(feature = "std")]
    parsed: std::sync::OnceLock<Box<[Declared]>>,
}

// SAFETY: Taffy's lengths hold a pointer only for `calc()` values, which
// makes them neither Send nor Sync; nothing here parses to one, so the
// parsed declarations are plain data, written once by the OnceLock and
// only read after.
unsafe impl Sync for Declarations {}

impl Declarations {
    /// The declarations `source`, not parsed yet.
    pub const fn new(source: &'static [(&'static str, &'static str)]) -> Self {
        Self {
            source,
            #[cfg(feature = "std")]
            parsed: std::sync::OnceLock::new(),
        }
    }

    /// Applies the declarations, as [`super::apply_declarations`] does:
    /// those that set the font size first, since `em` lengths in the
    /// others depend on it.
    pub(crate) fn apply(&self, style: &mut ContainerStyle, layout: &mut Style) {
        #[cfg(feature = "std")]
        for declared in self.parsed.get_or_init(|| parse_all(self.source)).iter() {
            declared.apply(style, layout);
        }
        #[cfg(not(feature = "std"))]
        super::apply_declarations(self.source, style, layout);
    }
}

/// `source` parsed, in the order [`super::apply_declarations`] applies it.
#[cfg(feature = "std")]
fn parse_all(source: &'static [(&'static str, &'static str)]) -> Box<[Declared]> {
    let mut parsed = Vec::with_capacity(source.len());
    for font_pass in [true, false] {
        for &(prop, val) in source {
            if super::sets_font_size(prop) == font_pass {
                parsed.push(if same_everywhere(val) { Declared::parse(prop, val) } else { None }.unwrap_or(Declared::Text(prop, val)));
            }
        }
    }
    parsed.into_boxed_slice()
}

/// Whether `val` means the same on every element and viewport: no font
/// relative (`em`, `rem`) or viewport (`vw`, `vh`, `vmin`, `vmax`)
/// lengths. Errs towards no: a word containing "em" counts.
#[cfg(feature = "std")]
fn same_everywhere(val: &str) -> bool {
    !["em", "vw", "vh", "vm"].iter().any(|unit| val.contains(unit))
}
