
/// Metadata and pixel alpha coverage data for a precompiled bitmap glyph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitmapGlyph {
    /// The character represented by this glyph.
    pub character: char,
    /// Width of the glyph bitmap.
    pub width: u8,
    /// Height of the glyph bitmap.
    pub height: u8,
    /// X coordinate offset when drawing.
    pub x_offset: i8,
    /// Y coordinate offset when drawing.
    pub y_offset: i8,
    /// Horizontal offset advance after drawing.
    pub x_advance: u8,
    /// Alpha coverage bitmap data (8-bit grayscale).
    pub bitmap: &'static [u8],
}

/// A precompiled bitmap font representing a specific size and weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitmapFont {
    /// Font name family.
    pub name: &'static str,
    /// Pixel size this font was precompiled at.
    pub size: f32,
    /// Font weight (0 for Regular, 1 for Bold).
    pub weight: u16,
    /// Total line height spacing.
    pub line_height: f32,
    /// Array of glyph mappings sorted by character key.
    pub glyphs: &'static [BitmapGlyph],
}

impl BitmapFont {
    /// Look up character glyph metadata.
    pub fn lookup_glyph(&self, c: char) -> Option<&BitmapGlyph> {
        self.glyphs.binary_search_by_key(&c, |g| g.character)
            .ok()
            .map(|idx| &self.glyphs[idx])
    }
}



include!("roboto_regular.rs");
include!("roboto_bold.rs");

/// The default precompiled regular Roboto font.
pub static DEFAULT_ROBOTO_REGULAR: BitmapFont = ROBOTO_REGULAR_REGULAR;
/// The default precompiled bold Roboto font.
pub static DEFAULT_ROBOTO_BOLD: BitmapFont = ROBOTO_BOLD_BOLD;
