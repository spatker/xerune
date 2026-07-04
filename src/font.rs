
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitmapGlyph {
    pub character: char,
    pub width: u8,
    pub height: u8,
    pub x_offset: i8,
    pub y_offset: i8,
    pub x_advance: u8,
    pub bitmap: &'static [u8], // 8-bit alpha coverage
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BitmapFont {
    pub name: &'static str,
    pub size: f32,
    pub weight: u16, // 0 for Regular, 1 for Bold
    pub line_height: f32,
    pub glyphs: &'static [BitmapGlyph],
}

impl BitmapFont {
    pub fn lookup_glyph(&self, c: char) -> Option<&BitmapGlyph> {
        self.glyphs.binary_search_by_key(&c, |g| g.character)
            .ok()
            .map(|idx| &self.glyphs[idx])
    }
}



include!("roboto_regular.rs");
include!("roboto_bold.rs");

pub static DEFAULT_ROBOTO_REGULAR: BitmapFont = ROBOTO_REGULAR_REGULAR;
pub static DEFAULT_ROBOTO_BOLD: BitmapFont = ROBOTO_BOLD_BOLD;
