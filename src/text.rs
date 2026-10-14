//! Text shaping results and line layout.
//!
//! A [`TextMeasurer`](crate::TextMeasurer) shapes a run of text once into a
//! [`ShapedText`]: glyph ids with advances, tagged with the byte offset of the
//! source text they came from. Line breaking and positioning only do
//! arithmetic on that; renderers draw the glyph ids. The shaper can therefore
//! grow (kerning, ligatures, complex scripts) without touching layout or the
//! renderer.

use crate::alloc_prelude::*;

/// One shaped glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShapedGlyph {
    /// Byte offset in the source text of the first character this glyph
    /// represents. Non-decreasing along the run.
    pub cluster: u32,
    /// Index of the font in the measurer's font list.
    pub font: u16,
    /// Glyph id within that font.
    pub id: u16,
    /// Horizontal advance in pixels.
    pub advance: f32,
}

/// Vertical font metrics of a shaped run, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineMetrics {
    /// Distance from the top of the line to the baseline.
    pub ascent: f32,
    /// Distance from the baseline to the bottom of the line (positive).
    pub descent: f32,
    /// Extra gap the font asks for between lines.
    pub line_gap: f32,
}

impl LineMetrics {
    /// Height of a line with `line-height: normal`.
    pub fn normal_line_height(&self) -> f32 {
        self.ascent + self.descent + self.line_gap
    }
}

/// A run of text shaped with one font size and weight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    /// Glyphs in logical order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Font metrics of the run.
    pub metrics: LineMetrics,
}

impl ShapedText {
    /// Sum of all advances: the width of the run on one line.
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }
}
