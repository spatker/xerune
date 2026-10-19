//! Form controls (`<input type="checkbox">`, `<input type="range">`,
//! `<progress>`) drawn as Chromium draws them with `appearance: auto`: the
//! same geometry and colors, tinted by `accent-color`. Measured from
//! Chromium screenshots at device scale 1. With `appearance: none` a control
//! is its CSS box, plus what Chromium still draws then.
//!
//! Parts with CSS (`::-webkit-slider-thumb { ... }`) are drawn as CSS boxes
//! where Chromium lays them out, starting from the native look's colors
//! (html.css's under `appearance: none`); unstyled parts stay native.

use crate::alloc_prelude::*;
use crate::graphics::{Color, DrawCommand, Rect};
use crate::style::{BorderSide, Borders, ContainerStyle, ControlPart, ControlParts, PartStyle};
use taffy::style::CompactLength;

/// `accent-color: auto`.
const DEFAULT_ACCENT: Color = Color { r: 0x00, g: 0x75, b: 0xff, a: 255 };
/// Unchecked checkbox border.
const CHECKBOX_BORDER: Color = Color { r: 0x76, g: 0x76, b: 0x76, a: 255 };
/// Range and progress tracks.
const TRACK: Color = Color { r: 0xef, g: 0xef, b: 0xef, a: 255 };
const TRACK_BORDER: Color = Color { r: 0xb2, g: 0xb2, b: 0xb2, a: 255 };
/// The filled part's border is the accent mixed half and half with this.
const FILL_BORDER_MIX: Color = Color { r: 0x75, g: 0x75, b: 0x75, a: 255 };

/// Range track height and thumb diameter; they don't scale with the control.
const RANGE_TRACK: f32 = 8.0;
const RANGE_THUMB: f32 = 16.0;

fn accent(style: &ContainerStyle) -> Color {
    style.accent_color.unwrap_or(DEFAULT_ACCENT)
}

fn fill_border(accent: Color) -> Color {
    let mix = |a: u8, b: u8| ((a as u16 + b as u16 + 1) / 2) as u8;
    Color {
        r: mix(accent.r, FILL_BORDER_MIX.r),
        g: mix(accent.g, FILL_BORDER_MIX.g),
        b: mix(accent.b, FILL_BORDER_MIX.b),
        a: accent.a,
    }
}

fn rect(commands: &mut Vec<DrawCommand>, rect: Rect, color: Color, border: Option<Color>, radius: f32) {
    commands.push(DrawCommand::DrawRect {
        rect,
        color: Some(color),
        gradient: None,
        border_radius: radius,
        border: match border {
            Some(c) => Borders::all(BorderSide::solid(1.0, c)),
            None => Borders::NONE,
        },
    });
}

/// The content box of a control whose border box is `r`.
pub(crate) fn content_box(r: Rect, style: &ContainerStyle) -> Rect {
    let b = style.border.widths();
    let left = b.left + style.padding_left;
    let top = b.top + style.padding_top;
    Rect {
        x: r.x + left,
        y: r.y + top,
        width: (r.width - left - b.right - style.padding_right).max(0.0),
        height: (r.height - top - b.bottom - style.padding_bottom).max(0.0),
    }
}

/// A checkbox fills its box: a white rounded square with a grey border, or
/// an accent-colored one with a white check mark.
pub(crate) fn checkbox(commands: &mut Vec<DrawCommand>, r: Rect, checked: bool, style: &ContainerStyle) {
    if style.appearance_none {
        return;
    }
    if checked {
        rect(commands, r, accent(style), None, 2.0);
        commands.push(DrawCommand::DrawCheckMark { rect: r, color: Color::WHITE });
    } else {
        rect(commands, r, Color::WHITE, Some(CHECKBOX_BORDER), 2.0);
    }
}

fn is_appearance_none(declaration: &crate::style::PartDeclaration) -> bool {
    matches!(declaration.name.as_str(), "appearance" | "-webkit-appearance" | "-moz-appearance") && declaration.value.trim() == "none"
}

/// Resolves the CSS of `host`'s `parts`, over each part's defaults.
pub(crate) fn resolve_parts(host: &ContainerStyle, parts: &[ControlPart]) -> Option<Box<ControlParts>> {
    if host.part_declarations.is_empty() {
        return None;
    }
    let accent = accent(host);
    let mut resolved = Vec::new();
    for &part in parts {
        let mut declarations = host.part_declarations.iter().filter(|d| d.part == part).peekable();
        if declarations.peek().is_none() {
            continue;
        }
        let mut style = crate::defaults::get_default_style("div", host).container_style;
        let mut layout = taffy::Style::default();
        let px = |v: f32| taffy::style::Dimension::length(v);
        match part {
            ControlPart::ProgressBar => style.background_color = Some(if host.appearance_none { Color { r: 0x80, g: 0x80, b: 0x80, a: 255 } } else { TRACK }),
            ControlPart::ProgressValue => style.background_color = Some(if host.appearance_none { Color { r: 0x00, g: 0x80, b: 0x00, a: 255 } } else { accent }),
            ControlPart::SliderFill => style.background_color = Some(accent),
            // Chromium's thumb loses its look only with its own `appearance: none`.
            ControlPart::SliderThumb if !host.part_declarations.iter().any(|d| d.part == part && is_appearance_none(d)) => {
                style.background_color = Some(accent);
                style.border_radius = RANGE_THUMB / 2.0;
                layout.size = taffy::geometry::Size { width: px(RANGE_THUMB), height: px(RANGE_THUMB) };
            }
            _ => {}
        }
        for d in declarations {
            crate::css::apply_declaration(&d.name, &d.value, &mut style, &mut layout);
        }
        resolved.push((part, PartStyle { style, layout }));
    }
    Some(Box::new(ControlParts(resolved)))
}

/// The style of a text input's placeholder: its own text style in
/// Chromium's #757575, then its `::placeholder` CSS.
pub(crate) fn placeholder_style(host: &ContainerStyle) -> ContainerStyle {
    let mut style = super::text_style(host);
    style.color = Color { r: 0x75, g: 0x75, b: 0x75, a: 255 };
    let mut layout = taffy::Style::default();
    for d in host.part_declarations.iter().filter(|d| d.part == ControlPart::Placeholder) {
        crate::css::apply_declaration(&d.name, &d.value, &mut style, &mut layout);
    }
    style
}

/// A length or percentage of `base`; `None` for `auto`.
fn length(raw: CompactLength, base: f32) -> Option<f32> {
    match raw.tag() {
        CompactLength::LENGTH_TAG => Some(raw.value()),
        CompactLength::PERCENT_TAG => Some(raw.value() * base),
        _ => None,
    }
}

/// A part's border box, sized against `container` and placed at its
/// origin plus the part's margins (`auto` margins are 0).
fn part_rect(part: &PartStyle, container: Rect, width: f32, height: f32) -> Rect {
    let size = part.layout.size;
    let margin = part.layout.margin;
    Rect {
        x: container.x + length(margin.left.into_raw(), container.width).unwrap_or(0.0),
        y: container.y + length(margin.top.into_raw(), container.width).unwrap_or(0.0),
        width: length(size.width.into_raw(), container.width).unwrap_or(width).max(0.0),
        height: length(size.height.into_raw(), container.height).unwrap_or(height).max(0.0),
    }
}

/// A part's box like any element's: shadow, background and border.
fn paint_part(commands: &mut Vec<DrawCommand>, rect: Rect, part: &PartStyle) {
    let style = &part.style;
    if let Some(shadow) = &style.box_shadow {
        commands.push(DrawCommand::DrawBoxShadow {
            rect,
            border_radius: style.border_radius,
            offset_x: shadow.offset_x,
            offset_y: shadow.offset_y,
            blur_radius: shadow.blur_radius,
            spread_radius: shadow.spread_radius,
            color: shadow.color,
            inset: shadow.inset,
        });
    }
    commands.push(DrawCommand::DrawRect {
        rect,
        color: style.background_color,
        gradient: style.background_gradient.clone(),
        border_radius: style.border_radius,
        border: style.border.resolved(style.color),
    });
}

/// Where a range's parts go: see [`slider`].
struct SliderGeometry {
    /// The track's border box.
    track: Rect,
    /// The track's content box, which the thumb travels along.
    inner: Rect,
    thumb: Rect,
}

fn slider_parts(parts: Option<&ControlParts>) -> (Option<&PartStyle>, Option<&PartStyle>, Option<&PartStyle>) {
    match parts {
        Some(p) => (p.get(ControlPart::SliderTrack), p.get(ControlPart::SliderThumb), p.get(ControlPart::SliderFill)),
        None => (None, None, None),
    }
}

/// Lays out a range whose content box is `c` with its value at `fraction`.
fn slider_geometry(c: Rect, fraction: f32, style: &ContainerStyle, parts: Option<&ControlParts>) -> SliderGeometry {
    let fraction = fraction.clamp(0.0, 1.0);
    let (track_css, thumb_css, _) = slider_parts(parts);
    let native_track = track_css.is_none() && !style.appearance_none;

    let (thumb_w, thumb_h) = match thumb_css {
        Some(t) => {
            let r = part_rect(t, c, 0.0, 0.0);
            (r.width, r.height)
        }
        None => (RANGE_THUMB, RANGE_THUMB),
    };
    let track = match track_css {
        Some(t) => {
            let r = part_rect(t, c, c.width, thumb_h);
            Rect { y: c.y + (c.height - r.height) / 2.0, ..r }
        }
        // The native track is inset by a pixel at each end.
        None if native_track => Rect { x: c.x + 1.0, y: c.y + (c.height - RANGE_TRACK) / 2.0, width: (c.width - 2.0).max(0.0), height: RANGE_TRACK },
        None => Rect { y: c.y + (c.height - thumb_h) / 2.0, height: thumb_h, ..c },
    };
    let inner = match track_css {
        Some(t) => content_box(track, &t.style),
        None => track,
    };

    let thumb_margin = |side: fn(&taffy::Style) -> CompactLength| thumb_css.and_then(|t| length(side(&t.layout), c.width)).unwrap_or(0.0);
    let thumb_x = inner.x + fraction * (inner.width - thumb_w).max(0.0) + thumb_margin(|l| l.margin.left.into_raw());
    let thumb_y = match thumb_css {
        Some(t) if t.style.appearance_none => inner.y + thumb_margin(|l| l.margin.top.into_raw()),
        _ => inner.y + (inner.height - thumb_h) / 2.0,
    };
    // A CSS thumb is a layout box, snapped to whole pixels like Chromium's.
    let thumb_x = if thumb_css.is_some() { crate::graphics::floor(thumb_x + 0.5) } else { thumb_x };
    SliderGeometry { track, inner, thumb: Rect { x: thumb_x, y: thumb_y, width: thumb_w, height: thumb_h } }
}

/// The fraction (0 to 1) of a range whose content box is `c` that puts the
/// middle of its thumb at `x`, as pressing or dragging there does.
pub(crate) fn slider_fraction_at(c: Rect, x: f32, style: &ContainerStyle, parts: Option<&ControlParts>) -> f32 {
    let start = slider_geometry(c, 0.0, style, parts).thumb;
    let end = slider_geometry(c, 1.0, style, parts).thumb;
    let travel = end.x - start.x;
    if travel > 0.0 { ((x - start.x - start.width / 2.0) / travel).clamp(0.0, 1.0) } else { 0.0 }
}

/// A range input: a thin track across the control, accent-filled up to the
/// thumb, and a round thumb that travels within the track's ends.
/// `fraction` is the value's position between `min` and `max`. Without
/// its appearance only the thumb is left.
///
/// Styled parts are laid out as in Chromium: the track spans the control,
/// centered, as tall as the thumb unless sized; the thumb travels along the
/// track's content box, centered on it, or at its top (plus `margin-top`)
/// once it has `appearance: none`; the fill runs from the track's start to
/// the thumb's middle.
pub(crate) fn slider(commands: &mut Vec<DrawCommand>, c: Rect, fraction: f32, style: &ContainerStyle, parts: Option<&ControlParts>) {
    let accent = accent(style);
    let (track_css, thumb_css, fill_css) = slider_parts(parts);
    let native_track = track_css.is_none() && !style.appearance_none;
    let SliderGeometry { track, inner, thumb } = slider_geometry(c, fraction, style, parts);

    match track_css {
        Some(t) => paint_part(commands, track, t),
        None if native_track => rect(commands, track, TRACK, Some(TRACK_BORDER), RANGE_TRACK / 2.0),
        None => {}
    }
    let fill_width = (thumb.x + thumb.width / 2.0 - inner.x).max(0.0);
    match fill_css {
        Some(f) => {
            let r = part_rect(f, inner, fill_width, inner.height);
            paint_part(commands, Rect { x: inner.x, y: inner.y + (inner.height - r.height) / 2.0, width: fill_width, height: r.height }, f);
        }
        None if native_track => rect(commands, Rect { width: fill_width, ..track }, accent, Some(fill_border(accent)), RANGE_TRACK / 2.0),
        None => {}
    }
    match thumb_css {
        Some(t) => paint_part(commands, thumb, t),
        None => rect(commands, thumb, accent, None, RANGE_THUMB / 2.0),
    }
}

/// A progress bar: a rounded track half as tall as the control, filled
/// with the accent color up to `fraction` and cut off square there.
/// Without its appearance, html.css's grey bar and green value.
///
/// A styled bar is a box at the top of the control, filling it unless
/// sized; a styled value starts at the bar's content box, `fraction` of its
/// width and as tall as it unless sized.
pub(crate) fn progress(commands: &mut Vec<DrawCommand>, c: Rect, fraction: f32, style: &ContainerStyle, parts: Option<&ControlParts>) {
    let fraction = fraction.clamp(0.0, 1.0);
    let (bar_css, value_css) = match parts {
        Some(p) => (p.get(ControlPart::ProgressBar), p.get(ControlPart::ProgressValue)),
        None => (None, None),
    };
    if style.appearance_none && bar_css.is_none() && value_css.is_none() {
        let grey = Color { r: 0x80, g: 0x80, b: 0x80, a: 255 };
        let green = Color { r: 0x00, g: 0x80, b: 0x00, a: 255 };
        rect(commands, c, grey, None, 0.0);
        rect(commands, Rect { width: c.width * fraction, ..c }, green, None, 0.0);
        return;
    }
    let accent = accent(style);
    let native_bar = bar_css.is_none() && !style.appearance_none;
    let height = c.height / 2.0;
    let bar = match bar_css {
        Some(b) => part_rect(b, c, c.width, c.height),
        None if native_bar => Rect { x: c.x, y: c.y + (c.height - height) / 2.0, width: c.width, height },
        None => c,
    };
    match bar_css {
        Some(b) => paint_part(commands, bar, b),
        None if native_bar => rect(commands, bar, TRACK, Some(TRACK_BORDER), height / 2.0),
        None => rect(commands, bar, Color { r: 0x80, g: 0x80, b: 0x80, a: 255 }, None, 0.0),
    }

    let inner = match bar_css {
        Some(b) => content_box(bar, &b.style),
        None => bar,
    };
    match value_css {
        Some(v) => {
            let r = part_rect(v, inner, inner.width, inner.height);
            paint_part(commands, Rect { width: inner.width * fraction, ..r }, v);
        }
        None if native_bar => {
            if fraction > 0.0 {
                commands.push(DrawCommand::Clip { rect: Rect { width: bar.width * fraction, ..bar } });
                rect(commands, bar, accent, Some(fill_border(accent)), height / 2.0);
                commands.push(DrawCommand::PopClip);
            }
        }
        None => {
            let color = if style.appearance_none { Color { r: 0x00, g: 0x80, b: 0x00, a: 255 } } else { accent };
            rect(commands, Rect { width: inner.width * fraction, ..inner }, color, None, 0.0);
        }
    }
}
