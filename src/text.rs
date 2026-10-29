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

/// OpenType features to shape with on top of the font's defaults, as CSS
/// `font-variant-numeric` asks: a set of the constants below. Fonts without
/// a feature ignore it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FontFeatures(u8);

impl FontFeatures {
    /// Only the font's defaults.
    pub const NONE: Self = Self(0);
    /// `lining-nums`: digits as tall as capitals (`lnum`).
    pub const LINING_NUMS: Self = Self(1 << 0);
    /// `oldstyle-nums`: digits with ascenders and descenders (`onum`).
    pub const OLDSTYLE_NUMS: Self = Self(1 << 1);
    /// `proportional-nums`: digits as wide as their shapes (`pnum`).
    pub const PROPORTIONAL_NUMS: Self = Self(1 << 2);
    /// `tabular-nums`: digits all equally wide (`tnum`).
    pub const TABULAR_NUMS: Self = Self(1 << 3);
    /// `diagonal-fractions`: `1/2` as a fraction with a slash (`frac`).
    pub const DIAGONAL_FRACTIONS: Self = Self(1 << 4);
    /// `stacked-fractions`: `1/2` with a horizontal bar (`afrc`).
    pub const STACKED_FRACTIONS: Self = Self(1 << 5);
    /// `ordinal`: the letters of `1st` as ordinals (`ordn`).
    pub const ORDINAL: Self = Self(1 << 6);
    /// `slashed-zero`: a zero with a slash (`zero`).
    pub const SLASHED_ZERO: Self = Self(1 << 7);

    const TAGS: [[u8; 4]; 8] = [*b"lnum", *b"onum", *b"pnum", *b"tnum", *b"frac", *b"afrc", *b"ordn", *b"zero"];

    /// Whether every feature of `other` is in the set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether the set is [`NONE`](Self::NONE).
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The OpenType tags of the features in the set.
    pub fn tags(self) -> impl Iterator<Item = [u8; 4]> {
        Self::TAGS.into_iter().enumerate().filter(move |(i, _)| self.0 & (1 << i) != 0).map(|(_, tag)| tag)
    }
}

impl core::ops::BitOr for FontFeatures {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
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
    /// Per glyph: [`GLYPH_SPACE`] and [`GLYPH_JOINED`] bits.
    flags: Vec<u8>,
    min_content: f32,
    /// Widest grapheme: min-content when words may break anywhere.
    min_content_anywhere: f32,
    /// Widest piece between breaks that aren't between two letters.
    min_content_keep_all: f32,
    max_content: f32,
}

/// The glyph is whitespace.
const GLYPH_SPACE: u8 = 1;
/// The glyph continues the grapheme of the previous one (a combining mark,
/// a joined emoji, another glyph of the same character): no break before it.
const GLYPH_JOINED: u8 = 2;

/// Where lines may break inside words (CSS `overflow-wrap` and
/// `word-break`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WordBreak {
    /// Only at break opportunities; longer words overflow.
    #[default]
    Normal,
    /// A word that doesn't fit on a line of its own breaks anywhere
    /// (`overflow-wrap: break-word`).
    BreakWord,
    /// Like `BreakWord`, and the text's min-content width accounts for it
    /// (`overflow-wrap: anywhere`).
    Anywhere,
    /// Lines break between any two letters (`word-break: break-all`).
    BreakAll,
    /// Lines don't break between two letters, only at spaces and
    /// punctuation: Korean (and Chinese and Japanese) text keeps its words
    /// whole (`word-break: keep-all`).
    KeepAll,
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
    /// The break after this segment is between two letters, which
    /// [`WordBreak::KeepAll`] doesn't break.
    between_letters: bool,
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
    /// The line was cut short and an ellipsis follows it (see
    /// [`ShapedText::truncate`]).
    pub ellipsis: bool,
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
            let mandatory = opportunity == unicode_linebreak::BreakOpportunity::Mandatory;
            // An empty segment only matters as a forced break (a blank line).
            if g == seg_start && offset < text.len() && !mandatory {
                continue;
            }
            let forced = mandatory && offset < text.len();
            let before = text[..offset].chars().next_back();
            let after = text[offset..].chars().next();
            let between_letters = !mandatory && before.is_some_and(is_letter) && after.is_some_and(is_letter);
            segments.push(Segment { end: g as u32, width, trailing, forced, between_letters });
            width = 0.0;
            trailing = 0.0;
        }
        let mut flags = Vec::with_capacity(glyphs.len());
        let mut previous: Option<(u32, char)> = None;
        let mut grapheme = 0.0f32;
        let mut min_content_anywhere = 0.0f32;
        for glyph in &glyphs {
            let c = text[glyph.cluster as usize..].chars().next().unwrap_or(' ');
            let joined = previous.is_some_and(|(cluster, p)| cluster == glyph.cluster || p == '\u{200d}' || continues_grapheme(c));
            let mut f = if joined { GLYPH_JOINED } else { 0 };
            if c.is_whitespace() {
                f |= GLYPH_SPACE;
            }
            flags.push(f);
            grapheme = if joined { grapheme + glyph.advance } else { glyph.advance };
            if !c.is_whitespace() {
                min_content_anywhere = min_content_anywhere.max(grapheme);
            }
            previous = Some((glyph.cluster, c));
        }
        let mut shaped = Self { glyphs, metrics, segments, flags, min_content: 0.0, min_content_anywhere, min_content_keep_all: 0.0, max_content: 0.0 };
        shaped.min_content = shaped.segments.iter().fold(0.0, |w, s| w.max(s.width - s.trailing));
        let mut keep_all = 0.0f32;
        shaped.keep_all_segments(|seg| keep_all = keep_all.max(seg.width - seg.trailing));
        shaped.min_content_keep_all = keep_all;
        let mut max_content = 0.0f32;
        shaped.break_lines(f32::INFINITY, WordBreak::Normal, |line| max_content = max_content.max(line.width - line.hanging));
        shaped.max_content = max_content;
        shaped
    }

    /// Sum of all advances: the width of the run on one line.
    pub fn width(&self) -> f32 {
        self.glyphs.iter().map(|g| g.advance).sum()
    }

    /// Width of the widest unbreakable piece: the narrowest the text can
    /// get without overflowing (CSS min-content).
    pub fn min_content_width(&self, word_break: WordBreak) -> f32 {
        match word_break {
            WordBreak::Normal | WordBreak::BreakWord => self.min_content,
            WordBreak::Anywhere | WordBreak::BreakAll => self.min_content_anywhere,
            WordBreak::KeepAll => self.min_content_keep_all,
        }
    }

    /// The segments with those after which [`WordBreak::KeepAll`] doesn't
    /// break joined to the next.
    fn keep_all_segments(&self, mut each: impl FnMut(Segment)) {
        let mut joined: Option<Segment> = None;
        for seg in &self.segments {
            let seg = match joined.take() {
                Some(j) => Segment { width: j.width + seg.width, ..*seg },
                None => *seg,
            };
            if seg.between_letters {
                joined = Some(seg);
            } else {
                each(seg);
            }
        }
        if let Some(seg) = joined {
            each(seg);
        }
    }

    /// Width with lines broken only where they must be (CSS max-content).
    pub fn max_content_width(&self) -> f32 {
        self.max_content
    }

    /// Breaks the text greedily into lines no wider than `max_width` where
    /// possible. A piece that alone is wider than `max_width` overflows,
    /// unless `word_break` allows breaking it.
    pub fn lines(&self, max_width: f32, word_break: WordBreak) -> Vec<Line> {
        let mut lines = Vec::new();
        self.break_lines(max_width, word_break, |line| lines.push(self.finish_line(line)));
        lines
    }

    /// Number of lines at `max_width`, see [`lines`](Self::lines).
    pub fn line_count(&self, max_width: f32, word_break: WordBreak) -> usize {
        let mut count = 0;
        self.break_lines(max_width, word_break, |_| count += 1);
        count
    }

    /// Breaks into lines, emitting each as it ends. Counting lines is the hot
    /// path (taffy measures often), so lines come out raw: see
    /// [`finish_line`](Self::finish_line).
    fn break_lines(&self, max_width: f32, word_break: WordBreak, mut emit: impl FnMut(RawLine)) {
        let mut line = LineState::default();
        if word_break == WordBreak::KeepAll {
            self.keep_all_segments(|seg| line.place(&seg, max_width, &mut emit));
            line.finish(&mut emit);
            return;
        }
        if word_break == WordBreak::Normal {
            for seg in &self.segments {
                line.place(seg, max_width, &mut emit);
            }
            line.finish(&mut emit);
            return;
        }
        let mut seg_start = 0u32;
        for seg in &self.segments {
            let split = match word_break {
                WordBreak::Normal | WordBreak::KeepAll => false,
                WordBreak::BreakAll => true,
                // Only a word too long for a line of its own; it starts on
                // a fresh line.
                WordBreak::BreakWord | WordBreak::Anywhere => seg.width - seg.trailing > max_width + EPSILON,
            };
            if split {
                if word_break != WordBreak::BreakAll && line.end > line.start {
                    line.wrap(&mut emit);
                }
                let mut piece_start = seg_start;
                while piece_start < seg.end {
                    let piece = self.grapheme(piece_start, seg);
                    line.place(&piece, max_width, &mut emit);
                    piece_start = piece.end;
                }
            } else {
                line.place(seg, max_width, &mut emit);
            }
            seg_start = seg.end;
        }
        line.finish(&mut emit);
    }

    /// The [`Line`] of a raw line: hanging spaces neither count nor get
    /// drawn.
    fn finish_line(&self, raw: RawLine) -> Line {
        let mut end = raw.end;
        let mut hanging = raw.hanging;
        while end > raw.start && hanging > 0.0 {
            end -= 1;
            hanging -= self.glyphs[end as usize].advance;
        }
        Line { start: raw.start, end, width: raw.width - raw.hanging, ellipsis: false }
    }

    /// The grapheme of `seg` starting at glyph `start`, as a segment of its
    /// own; it inherits `seg`'s forced break if it ends `seg`.
    fn grapheme(&self, start: u32, seg: &Segment) -> Segment {
        let mut end = start + 1;
        while end < seg.end && self.flags[end as usize] & GLYPH_JOINED != 0 {
            end += 1;
        }
        let glyphs = &self.glyphs[start as usize..end as usize];
        let width = glyphs.iter().map(|g| g.advance).sum();
        let space = self.flags[start as usize] & GLYPH_SPACE != 0;
        Segment { end, width, trailing: if space { width } else { 0.0 }, forced: seg.forced && end == seg.end, between_letters: false }
    }

    /// `line` if it fits in `max_width`; otherwise its longest start that
    /// fits together with an ellipsis `ellipsis_width` wide, cut between
    /// graphemes (CSS `text-overflow: ellipsis`).
    pub fn truncate(&self, line: Line, max_width: f32, ellipsis_width: f32) -> Line {
        if line.width <= max_width + EPSILON {
            return line;
        }
        let budget = max_width - ellipsis_width;
        let (mut end, mut width) = (line.start, 0.0);
        while end < line.end {
            let mut next = end + 1;
            while next < line.end && self.flags[next as usize] & GLYPH_JOINED != 0 {
                next += 1;
            }
            let w: f32 = self.glyphs[end as usize..next as usize].iter().map(|g| g.advance).sum();
            if width + w > budget + EPSILON {
                break;
            }
            width += w;
            end = next;
        }
        Line { start: line.start, end, width, ellipsis: true }
    }

}

/// Tolerance for float error between the width text was measured at and
/// the width layout hands back.
const EPSILON: f32 = 0.01;

/// A line as [`ShapedText::break_lines`] finds it.
#[derive(Clone, Copy)]
struct RawLine {
    start: u32,
    end: u32,
    /// Advance of the glyphs, hanging spaces included.
    width: f32,
    /// Advance of the spaces at a wrapped line's end, which hang.
    hanging: f32,
}

/// The line being filled by [`ShapedText::break_lines`].
#[derive(Default)]
struct LineState {
    start: u32,
    end: u32,
    /// Advance of the line so far, trailing spaces included.
    width: f32,
    /// Advance of the spaces ending the line, which hang if it wraps there.
    trailing: f32,
    emitted: bool,
}

impl LineState {
    /// Adds `seg` to the line, wrapping first if it doesn't fit.
    #[inline]
    fn place(&mut self, seg: &Segment, max_width: f32, emit: &mut impl FnMut(RawLine)) {
        // The line's spaces are mid-line once `seg` joins it; only `seg`'s
        // own would hang.
        if self.end > self.start && self.width + seg.width - seg.trailing > max_width + EPSILON {
            self.wrap(emit);
        }
        self.width += seg.width;
        self.trailing = if seg.trailing < seg.width { seg.trailing } else { self.trailing + seg.trailing };
        self.end = seg.end;
        if seg.forced {
            self.wrap(emit);
        }
    }

    /// Ends the line at its current end.
    #[inline]
    fn wrap(&mut self, emit: &mut impl FnMut(RawLine)) {
        emit(RawLine { start: self.start, end: self.end, width: self.width, hanging: self.trailing });
        self.emitted = true;
        self.start = self.end;
        self.width = 0.0;
        self.trailing = 0.0;
    }

    /// Emits the last line. Spaces at the very end of the text only hang
    /// when a line wraps there: otherwise they separate it from whatever
    /// comes next on the line (`core {{ n }}`).
    fn finish(&mut self, emit: &mut impl FnMut(RawLine)) {
        if self.end > self.start || !self.emitted {
            emit(RawLine { start: self.start, end: self.end, width: self.width, hanging: 0.0 });
        }
    }
}

/// Whether `c` attaches to the character before it: combining marks,
/// variation selectors and emoji modifiers.
fn continues_grapheme(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F | 0x1AB0..=0x1AFF | 0x1DC0..=0x1DFF | 0x20D0..=0x20FF | 0xFE20..=0xFE2F
        | 0xFE00..=0xFE0F | 0x200D | 0x1F3FB..=0x1F3FF | 0xE0020..=0xE007F)
}

/// Whether `c` is a letter for [`WordBreak::KeepAll`]: CSS Text's
/// typographic letter units (line breaking classes AL, HL, NU, AI and ID),
/// and Hangul syllables and jamo.
fn is_letter(c: char) -> bool {
    use unicode_linebreak::BreakClass::*;
    matches!(
        unicode_linebreak::break_property(c as u32),
        Alphabetic | HebrewLetter | Numeric | Ambiguous | Ideographic | HangulLvSyllable | HangulLvtSyllable | HangulLJamo | HangulVJamo | HangulTJamo
    )
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
        p.lines(max_width, WordBreak::Normal).iter().map(|l| &text[byte(l.start)..byte(l.end)]).collect()
    }

    #[test]
    fn truncate_makes_room_for_the_ellipsis() {
        let p = paragraph("abcdefgh");
        let line = p.lines(f32::INFINITY, WordBreak::Normal)[0];
        assert_eq!(p.truncate(line, 100.0, 10.0), line, "fits: unchanged");
        // 50px: four letters and a 10px ellipsis.
        let cut = p.truncate(line, 50.0, 10.0);
        assert_eq!((cut.start, cut.end, cut.width, cut.ellipsis), (0, 4, 40.0, true));
        // Not even the ellipsis fits: nothing of the text is left.
        assert_eq!(p.truncate(line, 5.0, 10.0).end, 0);
    }

    #[test]
    fn breaks_greedily_at_spaces() {
        let text = "aa bb cc dddd";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 55.0), ["aa bb", "cc", "dddd"]);
        assert_eq!(line_texts(text, &p, 1000.0), [text]);
        let widths: Vec<f32> = p.lines(55.0, WordBreak::Normal).iter().map(|l| l.width).collect();
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
        assert_eq!(p.min_content_width(WordBreak::Normal), 40.0);
        assert_eq!(p.max_content_width(), 90.0);
    }

    #[test]
    fn overlong_word_overflows() {
        let text = "a bbbbbbbb c";
        let p = paragraph(text);
        assert_eq!(line_texts(text, &p, 30.0), ["a", "bbbbbbbb", "c"]);
    }

    fn texts_in<'a>(text: &'a str, max_width: f32, word_break: WordBreak) -> Vec<&'a str> {
        let p = paragraph(text);
        let byte = |i: u32| p.glyphs.get(i as usize).map_or(text.len(), |g| g.cluster as usize);
        p.lines(max_width, word_break).iter().map(|l| &text[byte(l.start)..byte(l.end)]).collect()
    }

    #[test]
    fn break_word_splits_only_words_too_long_for_a_line() {
        let text = "a bbbbbbbb c";
        // The long word starts a fresh line, then breaks where it must.
        assert_eq!(texts_in(text, 30.0, WordBreak::BreakWord), ["a", "bbb", "bbb", "bb", "c"]);
        assert_eq!(texts_in(text, 30.0, WordBreak::Anywhere), ["a", "bbb", "bbb", "bb", "c"]);
        // Words that fit are left alone.
        assert_eq!(texts_in("aa bb", 30.0, WordBreak::BreakWord), ["aa", "bb"]);
    }

    #[test]
    fn break_all_fills_lines() {
        assert_eq!(texts_in("aa bbbbbb", 40.0, WordBreak::BreakAll), ["aa b", "bbbb", "b"]);
    }

    #[test]
    fn keep_all_keeps_korean_words_whole() {
        // Each syllable is a break opportunity; keep-all leaves only the spaces and punctuation.
        let text = "다람쥐 헌 쳇바퀴에, 타고파";
        assert_eq!(texts_in(text, 40.0, WordBreak::Normal), ["다람쥐", "헌 쳇바", "퀴에,", "타고파"]);
        assert_eq!(texts_in(text, 40.0, WordBreak::KeepAll), ["다람쥐", "헌", "쳇바퀴에,", "타고파"]);
        assert_eq!(paragraph(text).min_content_width(WordBreak::KeepAll), 50.0);
        assert_eq!(paragraph(text).min_content_width(WordBreak::Normal), 20.0);
    }

    #[test]
    fn min_content_with_word_breaking() {
        let p = paragraph("aa bbbbbbbb");
        assert_eq!(p.min_content_width(WordBreak::Normal), 80.0);
        // break-word doesn't change min-content, anywhere does.
        assert_eq!(p.min_content_width(WordBreak::BreakWord), 80.0);
        assert_eq!(p.min_content_width(WordBreak::Anywhere), 10.0);
        assert_eq!(p.min_content_width(WordBreak::BreakAll), 10.0);
    }

    #[test]
    fn graphemes_stay_together() {
        // e + combining acute: never split between them.
        let text = "ae\u{301}e\u{301}e\u{301}";
        let lines = texts_in(text, 15.0, WordBreak::BreakAll);
        assert!(lines.iter().all(|l| !l.starts_with('\u{301}')), "{lines:?}");
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
        assert_eq!(p.lines(100.0, WordBreak::Normal), [Line { start: 0, end: 0, width: 0.0, ellipsis: false }]);
    }
}
