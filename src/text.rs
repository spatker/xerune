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

/// A run of text shaped with one font size and weight, split at its line
/// break opportunities (Unicode UAX #14) so it can be broken into lines at
/// any width with arithmetic only.
///
/// Scripts that need a dictionary to find word boundaries (Thai, Lao, Khmer,
/// Myanmar) only break at spaces.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShapedText {
    /// Glyphs in logical order.
    pub glyphs: Vec<ShapedGlyph>,
    /// Font metrics of the run.
    pub metrics: LineMetrics,
    segments: Vec<Segment>,
    min_content: f32,
    max_content: f32,
}

/// The glyphs of one line of a [`ShapedText`], as drawn by
/// [`DrawCommand::DrawText`](crate::DrawCommand::DrawText).
#[derive(Clone, Debug)]
pub struct GlyphRun {
    /// The whole shaped text.
    pub shaped: Arc<ShapedText>,
    /// First glyph of the line.
    pub start: u32,
    /// One past the last glyph of the line.
    pub end: u32,
    /// Byte offset of the command's `text` in the shaped source text, to map
    /// glyph clusters back to characters.
    pub text_start: u32,
    /// Distance from the top of the line box to the baseline.
    pub baseline: f32,
}

impl PartialEq for GlyphRun {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start
            && self.end == other.end
            && self.text_start == other.text_start
            && self.baseline == other.baseline
            && (Arc::ptr_eq(&self.shaped, &other.shaped) || self.shaped == other.shaped)
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

/// One laid out line: a range of glyphs of a [`ShapedText`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    /// First glyph of the line.
    pub start: u32,
    /// One past the last glyph to draw; spaces at the end of a wrapped line
    /// are excluded.
    pub end: u32,
    /// Advance of the visible glyphs.
    pub width: f32,
}

impl ShapedText {
    /// Wraps the `glyphs` shaped from `text` and finds the text's break
    /// opportunities.
    pub fn new(text: &str, glyphs: Vec<ShapedGlyph>, metrics: LineMetrics) -> Self {
        let mut segments = Vec::new();
        let mut g = 0usize;
        let mut width = 0.0;
        let mut trailing = 0.0;
        for (offset, opportunity) in unicode_linebreak::linebreaks(text) {
            // Glyphs whose cluster starts before the break belong to the segment.
            let seg_start = g;
            while g < glyphs.len() && (glyphs[g].cluster as usize) < offset {
                let glyph = &glyphs[g];
                width += glyph.advance;
                let c = text[glyph.cluster as usize..].chars().next().unwrap_or(' ');
                if c.is_whitespace() {
                    trailing += glyph.advance;
                } else {
                    trailing = 0.0;
                }
                g += 1;
            }
            if g == seg_start && offset < text.len() {
                continue;
            }
            let forced = opportunity == unicode_linebreak::BreakOpportunity::Mandatory && offset < text.len();
            segments.push(Segment { end: g as u32, width, trailing, forced });
            width = 0.0;
            trailing = 0.0;
        }
        let mut shaped = Self { glyphs, metrics, segments, min_content: 0.0, max_content: 0.0 };
        shaped.min_content = shaped.segments.iter().fold(0.0, |w, s| w.max(s.width - s.trailing));
        let mut max_content = 0.0f32;
        shaped.break_lines(f32::INFINITY, |line| max_content = max_content.max(line.width));
        shaped.max_content = max_content;
        shaped
    }

    /// Sum of all advances: the width of the run on one line.
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// Width of the widest unbreakable piece: the narrowest the text can
    /// get without overflowing (CSS min-content).
    pub fn min_content_width(&self) -> f32 {
        self.min_content
    }

    /// Width with lines broken only where they must be (CSS max-content).
    pub fn max_content_width(&self) -> f32 {
        self.max_content
    }

    /// Breaks the text greedily into lines no wider than `max_width` where
    /// possible. A piece that alone is wider than `max_width` overflows.
    pub fn lines(&self, max_width: f32) -> Vec<Line> {
        let mut lines = Vec::new();
        self.break_lines(max_width, |line| lines.push(line));
        lines
    }

    /// Number of lines at `max_width`, see [`lines`](Self::lines).
    pub fn line_count(&self, max_width: f32) -> usize {
        let mut count = 0;
        self.break_lines(max_width, |_| count += 1);
        count
    }

    fn break_lines(&self, max_width: f32, mut emit: impl FnMut(Line)) {
        let mut start = 0u32;
        let mut end = 0u32;
        // Width of the current line, and its last segment's trailing spaces.
        let mut width = 0.0f32;
        let mut trailing = 0.0f32;
        let mut emitted = false;
        for seg in &self.segments {
            // The previous segment's spaces are mid-line once `seg` joins
            // it; only `seg`'s own would hang. The tolerance absorbs float
            // error between the width text was measured at and the width
            // layout hands back.
            if end > start && width + seg.width - seg.trailing > max_width + 0.01 {
                emit(self.wrapped_line(start, end, width, trailing));
                emitted = true;
                start = end;
                width = 0.0;
            }
            width += seg.width;
            trailing = seg.trailing;
            end = seg.end;
            if seg.forced {
                emit(self.wrapped_line(start, end, width, trailing));
                emitted = true;
                start = end;
                width = 0.0;
                trailing = 0.0;
            }
        }
        // Spaces at the very end of the text only hang when a line wraps
        // there: otherwise they separate it from whatever comes next on the
        // line (`core {{ n }}`).
        if end > start || !emitted {
            emit(Line { start, end, width });
        }
    }

    /// A line ending at a break: its trailing spaces hang, so they neither
    /// count nor get drawn.
    fn wrapped_line(&self, start: u32, end: u32, width: f32, trailing: f32) -> Line {
        let mut visible_end = end;
        let mut hanging = trailing;
        while visible_end > start && hanging > 0.0 {
            visible_end -= 1;
            hanging -= self.glyphs[visible_end as usize].advance;
        }
        Line { start, end: visible_end, width: width - trailing }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every character 10px wide.
    fn paragraph(text: &str) -> ShapedText {
        let glyphs = text
            .char_indices()
            .map(|(i, _)| ShapedGlyph { cluster: i as u32, font: 0, id: 0, advance: 10.0 })
            .collect();
        ShapedText::new(text, glyphs, LineMetrics::default())
    }

    fn line_texts<'a>(text: &'a str, p: &ShapedText, max_width: f32) -> Vec<&'a str> {
        let g = &p.glyphs;
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
    fn spaces_between_words_count() {
        // "aa bb" is 50px: it must not fit in 45px.
        let text = "aa bb";
        assert_eq!(line_texts(text, &paragraph(text), 45.0), ["aa", "bb"]);
        assert_eq!(line_texts(text, &paragraph(text), 50.0), ["aa bb"]);
    }

    #[test]
    fn trailing_spaces_hang() {
        // "aa " fits in 20px: the space doesn't count.
        let text = "aa bb";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 20.0), ["aa", "bb"]);
    }

    #[test]
    fn spaces_at_the_end_of_the_text_count() {
        let text = "core ";
        let p = paragraph(text);
        assert_eq!(p.max_content_width(), 50.0);
        assert_eq!(line_texts(text, &p, 1000.0), ["core "]);
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
