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

/// A run between two line break opportunities: a word plus the spaces after
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Segment {
    /// Glyph index one past the segment's last glyph.
    end: u32,
    /// Advance of the whole segment, trailing spaces included.
    width: f32,
    /// Advance of the trailing spaces, which hang past the line end.
    trailing: f32,
    /// A line must end after this segment (a preserved newline).
    forced: bool,
}

/// One laid out line: a range of glyphs of a [`Paragraph`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// First glyph of the line.
    pub start: u32,
    /// One past the last visible glyph; spaces at the end of a wrapped
    /// line are excluded.
    pub end: u32,
    /// Advance of the visible glyphs.
    pub width: f32,
}

/// Shaped text split at its line break opportunities (Unicode UAX #14), ready
/// to be broken into lines at any width.
///
/// Scripts that need a dictionary to find word boundaries (Thai, Lao, Khmer,
/// Myanmar) only break at spaces.
#[derive(Clone, Debug, PartialEq)]
pub struct Paragraph {
    /// The shaped glyphs.
    pub shaped: Arc<ShapedText>,
    segments: Vec<Segment>,
}

impl Paragraph {
    /// Finds the break opportunities of `text`, which `shaped` was shaped
    /// from.
    pub fn new(text: &str, shaped: Arc<ShapedText>) -> Self {
        let glyphs = &shaped.glyphs;
        let mut segments = Vec::new();
        let mut g = 0usize;
        let mut start = 0usize;
        for (offset, opportunity) in unicode_linebreak::linebreaks(text) {
            // Glyphs whose cluster starts before the break belong to the segment.
            while g < glyphs.len() && (glyphs[g].cluster as usize) < offset {
                g += 1;
            }
            if g == start && offset < text.len() {
                continue;
            }
            let mut width = 0.0;
            let mut trailing = 0.0;
            for glyph in &glyphs[start..g] {
                width += glyph.advance;
                let c = text[glyph.cluster as usize..].chars().next().unwrap_or(' ');
                if c.is_whitespace() {
                    trailing += glyph.advance;
                } else {
                    trailing = 0.0;
                }
            }
            let forced = opportunity == unicode_linebreak::BreakOpportunity::Mandatory && offset < text.len();
            segments.push(Segment { end: g as u32, width, trailing, forced });
            start = g;
        }
        Self { shaped, segments }
    }

    /// Font metrics of the text.
    pub fn metrics(&self) -> LineMetrics {
        self.shaped.metrics
    }

    /// Width of the widest unbreakable piece: the narrowest the text can
    /// get without overflowing (CSS min-content).
    pub fn min_content_width(&self) -> f32 {
        self.segments.iter().fold(0.0, |w, s| w.max(s.width - s.trailing))
    }

    /// Width with lines broken only where they must be (CSS max-content).
    pub fn max_content_width(&self) -> f32 {
        self.lines(f32::INFINITY).iter().fold(0.0, |w, l| w.max(l.width))
    }

    /// Breaks the text greedily into lines no wider than `max_width` where
    /// possible. A piece that alone is wider than `max_width` overflows.
    pub fn lines(&self, max_width: f32) -> Vec<Line> {
        let mut lines = Vec::new();
        let mut start = 0u32;
        let mut end = 0u32;
        // Width up to `end`, with and without the last segment's spaces.
        let mut width = 0.0f32;
        let mut visible = 0.0f32;
        let mut visible_end = 0u32;
        for seg in &self.segments {
            let seg_start = end;
            if end > start && width + seg.width - seg.trailing > max_width {
                lines.push(Line { start, end: visible_end, width: visible });
                start = seg_start;
                width = 0.0;
            }
            visible = width + seg.width - seg.trailing;
            width += seg.width;
            end = seg.end;
            visible_end = self.visible_end(seg_start, seg.end);
            if seg.forced {
                lines.push(Line { start, end: visible_end, width: visible });
                start = end;
                width = 0.0;
                visible = 0.0;
                visible_end = end;
            }
        }
        if end > start || lines.is_empty() {
            lines.push(Line { start, end: visible_end, width: visible });
        }
        lines
    }

    /// One past the last glyph of `start..end` that has a non-zero advance or
    /// is not trailing whitespace: trailing spaces hang and aren't drawn.
    fn visible_end(&self, start: u32, end: u32) -> u32 {
        let seg = self.segments.iter().find(|s| s.end == end);
        let mut trailing = seg.map_or(0.0, |s| s.trailing);
        let mut i = end;
        while i > start && trailing > 0.0 {
            i -= 1;
            trailing -= self.shaped.glyphs[i as usize].advance;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10px wide.
    fn paragraph(text: &str) -> Paragraph {
        let glyphs = text
            .char_indices()
            .map(|(i, _)| ShapedGlyph { cluster: i as u32, font: 0, id: 0, advance: 10.0 })
            .collect();
        Paragraph::new(text, Arc::new(ShapedText { glyphs, metrics: LineMetrics::default() }))
    }

    fn line_texts<'a>(text: &'a str, p: &Paragraph, max_width: f32) -> Vec<&'a str> {
        let g = &p.shaped.glyphs;
        let byte = |i: u32| g.get(i as usize).map_or(text.len(), |g| g.cluster as usize);
        p.lines(max_width).iter().map(|l| &text[byte(l.start)..byte(l.end)]).collect()
    }

    #[test]
    fn breaks_greedily_at_spaces() {
        let text = "aa bb cc dddd";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 55.0), ["aa bb", "cc", "dddd"]);
        assert_eq!(line_texts(text, &p, 1000.0), [text]);
        let widths: Vec<f32> = p.lines(55.0).iter().map(|l| l.width).collect();
        assert_eq!(widths, [50.0, 20.0, 40.0]);
    }

    #[test]
    fn trailing_spaces_hang() {
        // "aa " fits in 20px: the space doesn't count.
        let text = "aa bb";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 20.0), ["aa", "bb"]);
    }

    #[test]
    fn content_sizes() {
        let p = paragraph("aa bbbb c");
        assert_eq!(p.min_content_width(), 40.0);
        assert_eq!(p.max_content_width(), 90.0);
    }

    #[test]
    fn overlong_word_overflows() {
        let text = "a bbbbbbbb c";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 30.0), ["a", "bbbbbbbb", "c"]);
    }

    #[test]
    fn breaks_after_hyphens_and_between_ideographs() {
        let text = "well-known";
        assert_eq!(line_texts(text, &paragraph(text), 60.0), ["well-", "known"]);
        let text = "日本語の文章";
        assert_eq!(line_texts(text, &paragraph(text), 30.0), ["日本語", "の文章"]);
    }

    #[test]
    fn newlines_force_breaks() {
        let text = "aa\nbb";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 1000.0), ["aa", "bb"]);
        assert_eq!(p.max_content_width(), 20.0);
    }

    #[test]
    fn empty_text_has_one_empty_line() {
        let p = paragraph("");
        assert_eq!(p.lines(100.0), [Line { start: 0, end: 0, width: 0.0 }]);
    }
}
