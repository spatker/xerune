//! Form controls (`<input type="checkbox">`, `<input type="range">`,
//! `<progress>`) drawn as Chromium draws them with `appearance: auto`: the
//! same geometry and colors, tinted by `accent-color`. Measured from
//! Chromium screenshots at device scale 1. With `appearance: none` a control
//! is its CSS box, plus what Chromium still draws then.

use crate::alloc_prelude::*;
use crate::graphics::{Color, DrawCommand, Rect};
use crate::style::{BorderSide, Borders, ContainerStyle};

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

/// A range input: a thin track across the control, accent-filled up to the
/// thumb, and a round thumb that travels within the track's ends.
/// `fraction` is the value's position between `min` and `max`. Without
/// its appearance only the thumb is left.
pub(crate) fn slider(commands: &mut Vec<DrawCommand>, c: Rect, fraction: f32, style: &ContainerStyle) {
    let accent = accent(style);
    // The native track is inset by a pixel at each end.
    let inset = if style.appearance_none { 0.0 } else { 1.0 };
    let track = Rect { x: c.x + inset, y: c.y + (c.height - RANGE_TRACK) / 2.0, width: (c.width - 2.0 * inset).max(0.0), height: RANGE_TRACK };
    let thumb_x = track.x + fraction.clamp(0.0, 1.0) * (track.width - RANGE_THUMB).max(0.0);
    if !style.appearance_none {
        rect(commands, track, TRACK, Some(TRACK_BORDER), RANGE_TRACK / 2.0);
        let fill = Rect { width: thumb_x + RANGE_THUMB / 2.0 - track.x, ..track };
        rect(commands, fill, accent, Some(fill_border(accent)), RANGE_TRACK / 2.0);
    }

    let thumb = Rect { x: thumb_x, y: c.y + (c.height - RANGE_THUMB) / 2.0, width: RANGE_THUMB, height: RANGE_THUMB };
    rect(commands, thumb, accent, None, RANGE_THUMB / 2.0);
}

/// A progress bar: a rounded track half as tall as the control, filled
/// with the accent color up to `fraction` and cut off square there.
/// Without its appearance, html.css's grey bar and green value.
pub(crate) fn progress(commands: &mut Vec<DrawCommand>, c: Rect, fraction: f32, style: &ContainerStyle) {
    let fraction = fraction.clamp(0.0, 1.0);
    if style.appearance_none {
        let grey = Color { r: 0x80, g: 0x80, b: 0x80, a: 255 };
        let green = Color { r: 0x00, g: 0x80, b: 0x00, a: 255 };
        rect(commands, c, grey, None, 0.0);
        rect(commands, Rect { width: c.width * fraction, ..c }, green, None, 0.0);
        return;
    }
    let accent = accent(style);
    let height = c.height / 2.0;
    let track = Rect { x: c.x, y: c.y + (c.height - height) / 2.0, width: c.width, height };
    rect(commands, track, TRACK, Some(TRACK_BORDER), height / 2.0);

    if fraction > 0.0 {
        commands.push(DrawCommand::Clip { rect: Rect { width: c.width * fraction, ..track } });
        rect(commands, track, accent, Some(fill_border(accent)), height / 2.0);
        commands.push(DrawCommand::PopClip);
    }
}
