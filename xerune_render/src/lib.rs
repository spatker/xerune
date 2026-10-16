#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod blitter;
pub mod gradient;
pub mod rounded_rect;

#[cfg(feature = "std")]
use fontdue::Font;
use xerune::{DrawCommand, Pixels, Rect, RenderResources, Renderer, Rotation, Surface, TextMeasurer};
use xerune::alloc_prelude::*;
use xerune::text::{LineMetrics, ShapedGlyph, ShapedText};

use blitter::{pack_color, blend_solid_rect, blend_pixel, blend_glyph_span, div_255, calc_pixel_index};
use rounded_rect::{draw_rounded_rect, draw_rounded_border, draw_border_sides, draw_box_shadow};

#[cfg(feature = "profile")]
macro_rules! profile {
    ($($tt:tt)*) => { coarse_prof::profile!($($tt)*); };
}

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

/// The fonts of a [`Fonts`], borrowed for shaping or one render call.
#[derive(Clone, Copy)]
enum FontSource<'a> {
    #[cfg(feature = "std")]
    Ttf(&'a [Font]),
    Bitmap(&'a [xerune::font::BitmapFont]),
}

/// The fonts text is shaped and drawn with: TrueType/OpenType faces (regular
/// and bold, with `std`) or prerasterized bitmap fonts. Shared by the
/// runtime, which measures text with it, and the [`SoftwareRenderer`], which
/// draws it; clones are cheap.
#[derive(Clone)]
pub struct Fonts(Arc<FontSet>);

#[derive(Clone)]
struct FontSet {
    /// Distinguishes font sets in the thread-wide shaping caches (std only).
    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    id: u32,
    kind: FontKind,
}

#[derive(Clone)]
enum FontKind {
    #[cfg(feature = "std")]
    Ttf { faces: Vec<Font>, color_emoji: Option<Cow<'static, [u8]>> },
    Bitmap(&'static [xerune::font::BitmapFont]),
}

/// A font file that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontError(pub &'static str);

impl core::fmt::Display for FontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid font: {}", self.0)
    }
}

#[cfg(feature = "std")]
impl std::error::Error for FontError {}

impl Fonts {
    fn new(kind: FontKind) -> Self {
        static NEXT_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Self(Arc::new(FontSet { id, kind }))
    }

    /// TrueType/OpenType fonts: `regular` for normal text and `bold` for
    /// bold text (`regular` if `None`).
    #[cfg(feature = "std")]
    pub fn ttf(regular: &[u8], bold: Option<&[u8]>) -> Result<Self, FontError> {
        let face = |bytes: &[u8]| Font::from_bytes(bytes, fontdue::FontSettings::default()).map_err(FontError);
        let mut faces = vec![face(regular)?];
        if let Some(bold) = bold {
            faces.push(face(bold)?);
        }
        Ok(Self::new(FontKind::Ttf { faces, color_emoji: None }))
    }

    /// Draws characters the fonts lack from this color emoji font (bitmap
    /// glyphs, like NotoColorEmoji).
    #[cfg(feature = "std")]
    pub fn with_color_emoji(self, font: impl Into<Cow<'static, [u8]>>) -> Self {
        let mut set = Arc::unwrap_or_clone(self.0);
        if let FontKind::Ttf { color_emoji, .. } = &mut set.kind {
            *color_emoji = Some(font.into());
        }
        Self(Arc::new(set))
    }

    /// Prerasterized bitmap fonts (no TrueType parsing or rasterizing), as
    /// [`xerune::font::DEFAULT_ROBOTO`]. Text uses the font of its weight
    /// (1 for bold) and size, or else the first of its weight.
    pub fn bitmap(fonts: &'static [xerune::font::BitmapFont]) -> Self {
        Self::new(FontKind::Bitmap(fonts))
    }

    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    fn id(&self) -> u32 {
        self.0.id
    }

    fn source(&self) -> FontSource<'_> {
        match &self.0.kind {
            #[cfg(feature = "std")]
            FontKind::Ttf { faces, .. } => FontSource::Ttf(faces),
            FontKind::Bitmap(fonts) => FontSource::Bitmap(fonts),
        }
    }

    fn color_emoji(&self) -> Option<&[u8]> {
        match &self.0.kind {
            #[cfg(feature = "std")]
            FontKind::Ttf { color_emoji, .. } => color_emoji.as_deref(),
            FontKind::Bitmap(_) => None,
        }
    }
}

pub fn find_best_bitmap_font<'a>(fonts: &'a [xerune::font::BitmapFont], target_size: f32, target_weight: u16) -> Option<&'a xerune::font::BitmapFont> {
    if fonts.is_empty() {
        return None;
    }
    // Try to find exact size and weight
    if let Some(f) = fonts.iter().find(|f| (f.size - target_size).abs() < 0.1 && f.weight == target_weight) {
        return Some(f);
    }
    // Try to find matching weight
    if let Some(f) = fonts.iter().find(|f| f.weight == target_weight) {
        return Some(f);
    }
    // Fallback to first
    Some(&fonts[0])
}

#[cfg(feature = "std")]
const fn parse_usize_opt(opt: Option<&'static str>, default: usize) -> usize {
    match opt {
        Some(s) => {
            let bytes = s.as_bytes();
            let mut val = 0;
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b >= b'0' && b <= b'9' {
                    val = val * 10 + (b - b'0') as usize;
                } else {
                    return default;
                }
                i += 1;
            }
            val
        }
        None => default,
    }
}

/// Compile-time cache capacity limits for text measurement in embedded environments.
///
/// Can be configured at compile time via `XERUNE_RENDER_MAX_MEASURE_CACHE` (default: 64).
/// Set to 0 to disable text measurement caching.
#[cfg(feature = "std")]
pub const MAX_MEASURE_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("XERUNE_RENDER_MAX_MEASURE_CACHE"), 64);

/// How many rasterized glyphs (font, glyph, size) the glyph cache keeps
/// before it is emptied. Every distinct font size adds entries, so without a
/// limit an animated `font-size` grows the cache forever.
///
/// Configurable at compile time via `XERUNE_RENDER_MAX_GLYPH_CACHE`
/// (default: 2048, about 0.5 MB of 16 px glyphs).
#[cfg(feature = "std")]
pub const MAX_GLYPH_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("XERUNE_RENDER_MAX_GLYPH_CACHE"), 2048);

#[cfg(feature = "std")]
struct BoundedMeasureCache {
    entries: HashMap<String, Vec<(u32, u32, u16, f32, f32)>>,
    order: std::collections::VecDeque<String>,
    max_entries: usize,
}

#[cfg(feature = "std")]
impl BoundedMeasureCache {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: HashMap::with_capacity(max_entries),
            order: std::collections::VecDeque::with_capacity(max_entries),
            max_entries,
        }
    }

    fn get(&self, text: &str, font: u32, font_size_bits: u32, weight: u16) -> Option<(f32, f32)> {
        if self.max_entries == 0 {
            return None;
        }
        if let Some(entries) = self.entries.get(text) {
            for &(f, sz, wt, w, h) in entries {
                if f == font && sz == font_size_bits && wt == weight {
                    return Some((w, h));
                }
            }
        }
        None
    }

    fn insert(&mut self, text: &str, font: u32, font_size_bits: u32, weight: u16, dims: (f32, f32)) {
        if self.max_entries == 0 {
            return;
        }
        if let Some(list) = self.entries.get_mut(text) {
            if !list.iter().any(|&(f, sz, wt, _, _)| f == font && sz == font_size_bits && wt == weight) {
                list.push((font, font_size_bits, weight, dims.0, dims.1));
            }
            return;
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(text.to_string());
        self.entries.insert(
            text.to_string(),
            vec![(font, font_size_bits, weight, dims.0, dims.1)],
        );
    }
}

#[cfg(feature = "std")]
thread_local! {
    static MEASURE_CACHE: std::cell::RefCell<BoundedMeasureCache> =
        std::cell::RefCell::new(BoundedMeasureCache::new(MAX_MEASURE_CACHE_ENTRIES));
    static SHAPE_CACHE: std::cell::RefCell<ShapeCache> =
        std::cell::RefCell::new(ShapeCache::new(MAX_SHAPE_CACHE_ENTRIES));
}

/// How many shaped runs [`Fonts`] keep (thread-wide), oldest evicted first.
/// Every text run of a view is shaped on each rebuild, so this should exceed
/// the number of distinct runs on screen.
///
/// Configurable at compile time via `XERUNE_RENDER_MAX_SHAPE_CACHE` (default:
/// 1024). Set to 0 to disable.
#[cfg(feature = "std")]
pub const MAX_SHAPE_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("XERUNE_RENDER_MAX_SHAPE_CACHE"), 1024);

/// Font source marker for color emoji glyphs in a [`ShapedText`]; their id is
/// meaningless and the renderer looks the character up by cluster instead.
pub const EMOJI_FONT: u16 = u16::MAX;

#[cfg(feature = "std")]
struct ShapeCache {
    // Looked up for every text node on every rebuild: ahash rather than
    // the slower DoS-resistant default.
    entries: hashbrown::HashMap<String, Vec<(u32, u32, u16, Arc<ShapedText>)>>,
    order: std::collections::VecDeque<String>,
    max_entries: usize,
}

#[cfg(feature = "std")]
impl ShapeCache {
    fn new(max_entries: usize) -> Self {
        Self { entries: hashbrown::HashMap::new(), order: std::collections::VecDeque::new(), max_entries }
    }

    fn get(&self, text: &str, font: u32, font_size_bits: u32, weight: u16) -> Option<Arc<ShapedText>> {
        self.entries
            .get(text)?
            .iter()
            .find(|(f, sz, wt, _)| *f == font && *sz == font_size_bits && *wt == weight)
            .map(|(_, _, _, shaped)| shaped.clone())
    }

    fn insert(&mut self, text: &str, font: u32, font_size_bits: u32, weight: u16, shaped: Arc<ShapedText>) {
        if self.max_entries == 0 {
            return;
        }
        if let Some(list) = self.entries.get_mut(text) {
            list.push((font, font_size_bits, weight, shaped));
            return;
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(text.to_string());
        self.entries.insert(text.to_string(), vec![(font, font_size_bits, weight, shaped)]);
    }
}

#[cfg(feature = "std")]
pub fn shape_cache_len() -> usize {
    SHAPE_CACHE.with(|cache| cache.borrow().entries.len())
}

/// Font metrics rounded to whole pixels, as browsers do, so lines start on
/// pixel boundaries.
#[cfg(feature = "std")]
fn rounded_metrics(ascent: f32, descent: f32, line_gap: f32) -> LineMetrics {
    LineMetrics { ascent: ascent.round(), descent: descent.round(), line_gap: line_gap.round() }
}

impl Fonts {
    // Bitmap fonts set the metrics themselves; only TrueType reads the default.
    #[cfg_attr(not(feature = "std"), allow(unused_assignments))]
    fn shape_uncached(&self, text: &str, font_size: f32, weight: u16) -> ShapedText {
        let mut glyphs = Vec::with_capacity(text.len());
        let mut metrics = LineMetrics::default();
        match &self.source() {
            #[cfg(feature = "std")]
            FontSource::Ttf(ttf_fonts) => {
                let Some(font_index) = ttf_font_index(ttf_fonts, weight) else {
                    return ShapedText::default();
                };
                let font = &ttf_fonts[font_index];
                if let Some(m) = font.horizontal_line_metrics(font_size) {
                    metrics = rounded_metrics(m.ascent, -m.descent, m.line_gap);
                }
                for (i, c) in text.char_indices() {
                    // Newlines are line breaks, not glyphs.
                    if c == '\u{fe0f}' || c.is_control() {
                        continue;
                    }
                    let id = font.lookup_glyph_index(c);
                    let advance = font.metrics_indexed(id, font_size).advance_width;
                    // Characters the font lacks are drawn as color emoji when
                    // an emoji font is available (see the renderer), a bit
                    // wider than the font's missing-glyph box.
                    let (font, advance) = if is_emoji(c, id) {
                        (EMOJI_FONT, advance + font_size * 0.85)
                    } else {
                        (font_index as u16, advance)
                    };
                    glyphs.push(ShapedGlyph { cluster: i as u32, font, id, advance });
                }
            }
            FontSource::Bitmap(bitmap_fonts) => {
                let Some(font) = find_best_bitmap_font(bitmap_fonts, font_size, weight) else {
                    return ShapedText::default();
                };
                metrics = LineMetrics { ascent: font.line_height, descent: 0.0, line_gap: 0.0 };
                for (i, c) in text.char_indices().filter(|(_, c)| !c.is_control()) {
                    let (id, advance) = match font.glyphs.binary_search_by_key(&c, |g| g.character) {
                        Ok(idx) => (idx as u16, font.glyphs[idx].x_advance as f32),
                        Err(_) => (u16::MAX, 0.0),
                    };
                    glyphs.push(ShapedGlyph { cluster: i as u32, font: 0, id, advance });
                }
            }
        }
        ShapedText::new(text, glyphs, metrics)
    }
}

#[cfg(feature = "std")]
fn ttf_font_index(fonts: &[Font], weight: u16) -> Option<usize> {
    match fonts.len() {
        0 => None,
        1 => Some(0),
        _ => Some(if weight > 0 { 1 } else { 0 }),
    }
}

/// Whether `c` is drawn from a color emoji font: emoji-only planes, and
/// anything the text font lacks.
#[cfg(feature = "std")]
fn is_emoji(c: char, glyph_index: u16) -> bool {
    (c as u32) >= 0x1F000 || glyph_index == 0
}

#[cfg(feature = "std")]
pub fn measure_cache_len() -> usize {
    MEASURE_CACHE.with(|cache| cache.borrow().entries.len())
}

impl TextMeasurer for Fonts {
    fn shape(&self, text: &str, font_size: f32, weight: u16) -> Arc<ShapedText> {
        profile!("text_shape");
        #[cfg(feature = "std")]
        {
            let size_bits = font_size.to_bits();
            if let Some(shaped) = SHAPE_CACHE.with(|cache| cache.borrow().get(text, self.id(), size_bits, weight)) {
                return shaped;
            }
            let shaped = Arc::new(self.shape_uncached(text, font_size, weight));
            SHAPE_CACHE.with(|cache| cache.borrow_mut().insert(text, self.id(), size_bits, weight, shaped.clone()));
            shaped
        }
        #[cfg(not(feature = "std"))]
        Arc::new(self.shape_uncached(text, font_size, weight))
    }

    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        profile!("text_measure");
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        #[cfg(feature = "std")]
        let font_size_bits = font_size.to_bits();
        #[cfg(feature = "std")]
        let cached = MEASURE_CACHE.with(|cache| cache.borrow().get(text, self.id(), font_size_bits, weight));
        #[cfg(not(feature = "std"))]
        let cached = None;

        if let Some(dims) = cached {
            return dims;
        }

        let result = match &self.source() {
            #[cfg(feature = "std")]
            FontSource::Ttf(ttf_fonts) => {
                if ttf_fonts.is_empty() {
                    return (0.0, 0.0);
                }
                let font_index = if weight > 0 && ttf_fonts.len() > 1 { 1 } else { 0 };

                let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                layout.reset(&fontdue::layout::LayoutSettings::default());
                layout.append(ttf_fonts, &fontdue::layout::TextStyle::new(text, font_size, font_index));

                let mut min_x = f32::MAX;
                let mut min_y = f32::MAX;
                let mut max_x = f32::MIN;
                let mut max_y = f32::MIN;

                let mut extra_x = 0.0f32;
                for glyph in layout.glyphs() {
                    if glyph.parent == '\u{fe0f}' {
                        continue;
                    }
                    let is_emoji = is_emoji(glyph.parent, glyph.key.glyph_index);
                    let gx = glyph.x + extra_x;
                    let gy = glyph.y;
                    let mut gw = glyph.width as f32;
                    let mut gh = glyph.height as f32;
                    if is_emoji {
                        let target_sz = glyph.key.px;
                        gw = target_sz;
                        gh = target_sz;
                        extra_x += target_sz * 0.85;
                    }

                    if gx < min_x { min_x = gx; }
                    if gy < min_y { min_y = gy; }
                    if gx + gw > max_x { max_x = gx + gw; }
                    if gy + gh > max_y { max_y = gy + gh; }
                }

                let mut width = if max_x > min_x { max_x - min_x } else { 0.0 };
                // Ink bounds ignore a trailing space, but it must still
                // separate this text from whatever box follows.
                if text.ends_with(' ') {
                    width += ttf_fonts[font_index].metrics(' ', font_size).advance_width;
                }

                let height = if let Some(metrics) = ttf_fonts[font_index].horizontal_line_metrics(font_size) {
                    metrics.new_line_size
                } else {
                    if max_y > min_y { max_y - min_y } else { 20.0 }
                };

                (width, height)
            }
            FontSource::Bitmap(bitmap_fonts) => {
                let font = match find_best_bitmap_font(bitmap_fonts, font_size, weight) {
                    Some(f) => f,
                    None => return (0.0, 0.0),
                };

                let mut max_w: f32 = 0.0;
                let mut current_w: f32 = 0.0;
                let mut lines = 1;
                for c in text.chars() {
                    if c == '\n' {
                        max_w = max_w.max(current_w);
                        current_w = 0.0;
                        lines += 1;
                        continue;
                    }
                    if let Some(glyph) = font.lookup_glyph(c) {
                        current_w += glyph.x_advance as f32;
                    }
                }
                max_w = max_w.max(current_w);
                let height = lines as f32 * font.line_height;
                (max_w, height)
            }
        };

        #[cfg(feature = "std")]
        MEASURE_CACHE.with(|cache| {
            cache.borrow_mut().insert(text, self.id(), font_size_bits, weight, result);
        });

        result
    }
}

pub struct CachedGlyph {
    pub width: u32,
    pub height: u32,
    /// Offset of the bitmap's left edge from the pen position.
    pub xmin: i32,
    /// Offset of the bitmap's bottom edge above the baseline.
    pub ymin: i32,
    pub bitmap: Vec<u8>,
    pub rgba_bitmap: Option<Vec<u32>>,
    pub is_color: bool,
}

#[cfg(feature = "std")]
fn try_find_color_emoji_glyph(font_bytes_slice: &[&[u8]], c: char, target_size: f32) -> Option<CachedGlyph> {
    for fb in font_bytes_slice.iter().copied() {
        let face = match ttf_parser::Face::parse(fb, 0) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let glyph_id = match face.glyph_index(c) {
            Some(gid) => gid,
            None => continue,
        };
        let img = match face.glyph_raster_image(glyph_id, (target_size * 2.0) as u16)
            .or_else(|| face.glyph_raster_image(glyph_id, 0)) {
            Some(i) => i,
            None => continue,
        };
        let mut decoder = png::Decoder::new(img.data);
        decoder.set_transformations(png::Transformations::EXPAND);
        let mut reader = match decoder.read_info() {
            Ok(r) => r,
            Err(_) => continue,
        };
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = match reader.next_frame(&mut buf) {
            Ok(i) => i,
            Err(_) => continue,
        };

        let img_w = info.width;
        let img_h = info.height;
        if img_w == 0 || img_h == 0 {
            continue;
        }

        let mut rgba_pixels = Vec::with_capacity((img_w * img_h) as usize);
        let bytes = &buf[..info.buffer_size()];

        match info.color_type {
            png::ColorType::Rgba => {
                for chunk in bytes.chunks_exact(4) {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    let a = chunk[3] as u32;
                    let argb = (a << 24) | (r << 16) | (g << 8) | b;
                    rgba_pixels.push(argb);
                }
            }
            png::ColorType::Rgb => {
                for chunk in bytes.chunks_exact(3) {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    let argb = (255 << 24) | (r << 16) | (g << 8) | b;
                    rgba_pixels.push(argb);
                }
            }
            _ => continue,
        }

        return Some(CachedGlyph {
            width: img_w,
            height: img_h,
            xmin: 0,
            ymin: 0,
            bitmap: Vec::new(),
            rgba_bitmap: Some(rgba_pixels),
            is_color: true,
        });
    }
    None
}

/// Where and how [`blit_glyph`] draws.
#[cfg(feature = "std")]
struct GlyphTarget {
    /// Clip rectangle as (x1, y1, x2, y2) in buffer pixels.
    clip: (i32, i32, i32, i32),
    packed_color: u32,
    swap_rb: bool,
    rotation: u32,
    physical_width: u32,
    width: u32,
    height: u32,
}

#[cfg(feature = "std")]
fn emoji_cache_key(c: char, sub_px: u32) -> (usize, u16, u32) {
    let emoji_id = ((c as u32) ^ ((c as u32) >> 16)) as u16;
    (0xffff_usize, emoji_id, sub_px)
}

/// Looks up a glyph in the cache, rasterizing it first if needed: a color
/// emoji for `emoji` when an emoji font has it, else glyph `id` of
/// `fonts[font_index]`. None for glyphs without pixels (spaces).
#[cfg(feature = "std")]
#[allow(clippy::too_many_arguments)]
fn cached_glyph<'c>(
    cache: &'c mut HashMap<(usize, u16, u32), CachedGlyph>,
    fonts: &[Font],
    color_emoji: Option<&[u8]>,
    key: (usize, u16, u32),
    font_index: usize,
    id: u16,
    px: f32,
    emoji: Option<char>,
) -> Option<&'c CachedGlyph> {
    if !cache.contains_key(&key) {
        if cache.len() >= MAX_GLYPH_CACHE_ENTRIES {
            cache.clear();
        }
        let color = emoji.zip(color_emoji).and_then(|(c, font)| try_find_color_emoji_glyph(&[font], c, px));
        if let Some(cg) = color {
            cache.insert(key, cg);
        } else if let Some(font) = fonts.get(font_index) {
            let (metrics, bitmap) = font.rasterize_indexed(id, px);
            if metrics.width > 0 && metrics.height > 0 {
                cache.insert(
                    key,
                    CachedGlyph {
                        width: metrics.width as u32,
                        height: metrics.height as u32,
                        xmin: metrics.xmin,
                        ymin: metrics.ymin,
                        bitmap,
                        rgba_bitmap: None,
                        is_color: false,
                    },
                );
            }
        }
    }
    cache.get(&key)
}

/// Blends one cached glyph with its top-left corner at (gx, gy); color
/// glyphs are scaled to `px` square.
#[cfg(feature = "std")]
fn blit_glyph(buffer: &mut [u32], cached: &CachedGlyph, gx: i32, gy: i32, px: f32, t: &GlyphTarget) {
    let (clip_x1, clip_y1, clip_x2, clip_y2) = t.clip;
    let target_sz = px.round().max(1.0) as i32;
    let gw = if cached.is_color { target_sz } else { cached.width as i32 };
    let gh = if cached.is_color { target_sz } else { cached.height as i32 };
    let color_a = (t.packed_color >> 24) & 0xff;
    let r = (t.packed_color >> 16) & 0xff;
    let g = (t.packed_color >> 8) & 0xff;
    let b = t.packed_color & 0xff;

    let start_x = gx.max(clip_x1);
    let start_y = gy.max(clip_y1);
    let end_x = (gx + gw).min(clip_x2);
    let end_y = (gy + gh).min(clip_y2);

    if start_x < end_x && start_y < end_y {
        if cached.is_color {
            if let Some(ref rgba) = cached.rgba_bitmap {
                for y in start_y..end_y {
                    let src_y = (((y - gy) as usize * cached.height as usize) / target_sz as usize).min(cached.height as usize - 1);
                    let src_row_offset = src_y * cached.width as usize;
                    for x in start_x..end_x {
                        let src_x = (((x - gx) as usize * cached.width as usize) / target_sz as usize).min(cached.width as usize - 1);
                        let color_pixel = rgba[src_row_offset + src_x];
                        let sa = (color_pixel >> 24) & 0xff;
                        if sa > 0 {
                            let idx = calc_pixel_index(x, y, t.rotation, t.physical_width, t.width, t.height);
                            if idx < buffer.len() {
                                let final_color = if t.swap_rb {
                                    let sr = (color_pixel >> 16) & 0xff;
                                    let sg = (color_pixel >> 8) & 0xff;
                                    let sb = color_pixel & 0xff;
                                    (sa << 24) | (sb << 16) | (sg << 8) | sr
                                } else {
                                    color_pixel
                                };
                                blend_pixel(&mut buffer[idx], final_color);
                            }
                        }
                    }
                }
            }
        } else if t.rotation != 0 {
            for y in start_y..end_y {
                let src_y = (y - gy) as usize;
                let src_row_offset = src_y * cached.width as usize;
                for x in start_x..end_x {
                    let src_x = (x - gx) as usize;
                    let cov = cached.bitmap[src_row_offset + src_x];
                    if cov > 0 {
                        let a = div_255(color_a * cov as u32);
                        if a > 0 {
                            let idx = calc_pixel_index(x, y, t.rotation, t.physical_width, t.width, t.height);
                            if idx < buffer.len() {
                                let inv_a = 255 - a;
                                let d = buffer[idx];
                                let dst_a = (d >> 24) & 0xff;
                                let dst_r = (d >> 16) & 0xff;
                                let dst_g = (d >> 8) & 0xff;
                                let dst_b = d & 0xff;

                                let res_r = div_255(r * a + dst_r * inv_a);
                                let res_g = div_255(g * a + dst_g * inv_a);
                                let res_b = div_255(b * a + dst_b * inv_a);
                                let res_a = a + div_255(dst_a * inv_a);
                                buffer[idx] = (res_a << 24) | (res_r << 16) | (res_g << 8) | res_b;
                            }
                        }
                    }
                }
            }
        } else {
            for y in start_y..end_y {
                let src_y = (y - gy) as usize;
                let dst_row_start = (y * t.physical_width as i32 + start_x) as usize;
                let draw_w = (end_x - start_x) as usize;

                let src_x_start = (start_x - gx) as usize;
                let glyph_span = &cached.bitmap[src_y * cached.width as usize + src_x_start..src_y * cached.width as usize + src_x_start + draw_w];
                let dst_span = &mut buffer[dst_row_start..dst_row_start + draw_w];
                blend_glyph_span(dst_span, glyph_span, t.packed_color);
            }
        }
    }
}

/// Draws [`DrawCommand`]s in software. Keeps the rasterized glyphs and
/// converted images between frames.
pub struct SoftwareRenderer {
    fonts: Fonts,
    image_cache: HashMap<String, (u32, u32, Vec<u32>)>, // src -> (width, height, packed pixels)
    glyph_cache: HashMap<(usize, u16, u32), CachedGlyph>,
    clip_stack: Vec<Rect>,
    #[cfg(feature = "std")]
    layout: fontdue::layout::Layout,
}

impl SoftwareRenderer {
    /// A renderer drawing text with `fonts`, normally the same the runtime
    /// measures text with.
    pub fn new(fonts: Fonts) -> Self {
        Self {
            fonts,
            image_cache: HashMap::new(),
            glyph_cache: HashMap::new(),
            clip_stack: Vec::new(),
            #[cfg(feature = "std")]
            layout: fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown),
        }
    }
}

impl Renderer for SoftwareRenderer {
    fn render(&mut self, target: &mut Surface<'_>, commands: &[DrawCommand], resources: &RenderResources<'_>, area: Rect) {
        // Follow the image store's evictions: the converted pixels of an image
        // that is no longer drawn would otherwise stay here forever.
        self.image_cache.retain(|src, _| resources.images.get(src).is_some());

        let (width, height, stride, origin, rotation) =
            (target.width(), target.height(), target.stride(), target.origin(), target.rotation());
        let (buffer, swap_rb) = match target.pixels_mut() {
            Pixels::Xrgb8888(pixels) => (&mut **pixels, false),
            Pixels::Xbgr8888(pixels) => (&mut **pixels, true),
            _ => unimplemented!("pixel format"),
        };
        let (physical_width, physical_height) = match rotation {
            Rotation::Deg90 | Rotation::Deg270 => (height, width),
            Rotation::Deg0 | Rotation::Deg180 => (stride as u32, height),
        };
        let mut frame = Frame {
            buffer,
            width,
            height,
            physical_width,
            physical_height,
            fonts: self.fonts.source(),
            color_emoji: self.fonts.color_emoji(),
            clip_stack: &mut self.clip_stack,
            swap_rb,
            rotate: rotation != Rotation::Deg0,
            rotation: rotation.degrees(),
            x_offset: origin.0,
            y_offset: origin.1,
            image_cache: &mut self.image_cache,
            glyph_cache: &mut self.glyph_cache,
            #[cfg(feature = "std")]
            layout: &mut self.layout,
        };
        frame.draw(commands, resources, area);
        self.clip_stack.clear();
    }
}

/// One render call: the target's pixels and the renderer's state, borrowed.
struct Frame<'a> {
    buffer: &'a mut [u32],
    width: u32,
    height: u32,
    physical_width: u32,
    #[allow(dead_code)]
    physical_height: u32,
    fonts: FontSource<'a>,
    // Color emoji and rasterized glyphs come from TrueType fonts (std).
    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    color_emoji: Option<&'a [u8]>,
    clip_stack: &'a mut Vec<Rect>,
    swap_rb: bool,
    #[allow(dead_code)]
    rotate: bool,
    rotation: u32,
    x_offset: i32,
    y_offset: i32,
    image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>,
    #[cfg_attr(not(feature = "std"), allow(dead_code))]
    glyph_cache: &'a mut HashMap<(usize, u16, u32), CachedGlyph>,
    #[cfg(feature = "std")]
    layout: &'a mut fontdue::layout::Layout,
}

impl Frame<'_> {
    fn get_clip_rect(&self) -> Option<Rect> {
        self.clip_stack.last().copied()
    }

    #[inline(always)]
    fn translate_rect(&self, r: &Rect) -> Rect {
        Rect {
            x: r.x - self.x_offset as f32,
            y: r.y - self.y_offset as f32,
            width: r.width,
            height: r.height,
        }
    }
}

impl Frame<'_> {
    /// Draws `commands`, changing only pixels inside `area` (screen coordinates).
    fn draw(&mut self, commands: &[DrawCommand], resources: &RenderResources<'_>, area: Rect) {
        profile!("render_full");
        let tile_rect = Rect {
            x: self.x_offset as f32,
            y: self.y_offset as f32,
            width: self.width as f32,
            height: self.height as f32,
        };

        let Some(active_clip) = tile_rect.intersect(&area) else {
            return;
        };

        let local_base_clip = Rect {
            x: active_clip.x - self.x_offset as f32,
            y: active_clip.y - self.y_offset as f32,
            width: active_clip.width,
            height: active_clip.height,
        };

        self.clip_stack.push(local_base_clip);

        for command in commands {
            // Clip operations must stay balanced even outside the dirty region.
            let cmd_bounds = match command {
                DrawCommand::Clip { .. } | DrawCommand::PopClip => None,
                _ => command.bounds(),
            };

            if let Some(cb) = cmd_bounds {
                if !cb.intersects(&active_clip) {
                    continue;
                }
            }

            match command {
                DrawCommand::Clip { rect } => {
                    profile!("render_clip");
                    let local_rect = self.translate_rect(rect);
                    let intersected = if let Some(top) = self.clip_stack.last() {
                        let x1 = top.x.max(local_rect.x);
                        let y1 = top.y.max(local_rect.y);
                        let x2 = (top.x + top.width).min(local_rect.x + local_rect.width);
                        let y2 = (top.y + top.height).min(local_rect.y + local_rect.height);
                        Rect {
                            x: x1,
                            y: y1,
                            width: (x2 - x1).max(0.0),
                            height: (y2 - y1).max(0.0),
                        }
                    } else {
                        local_rect
                    };
                    self.clip_stack.push(intersected);
                }
                DrawCommand::PopClip => {
                    profile!("render_pop_clip");
                    self.clip_stack.pop();
                }
                DrawCommand::DrawBoxShadow {
                    rect,
                    border_radius,
                    offset_x,
                    offset_y,
                    blur_radius,
                    spread_radius,
                    color,
                    inset,
                } => {
                    profile!("render_box_shadow");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();
                    draw_box_shadow(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        floor_i32(local_rect.x),
                        floor_i32(local_rect.y),
                        local_rect.width as i32,
                        local_rect.height as i32,
                        *border_radius,
                        *offset_x,
                        *offset_y,
                        *blur_radius,
                        *spread_radius,
                        *color,
                        *inset,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );
                }
                DrawCommand::DrawRect {
                    rect,
                    color,
                    gradient,
                    border_radius,
                    border,
                } => {
                    profile!("render_rect");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();

                    if color.is_some() || gradient.is_some() {
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            floor_i32(local_rect.x),
                            floor_i32(local_rect.y),
                            local_rect.width as i32,
                            local_rect.height as i32,
                            *border_radius,
                            *color,
                            gradient.as_ref(),
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }

                    // FIXME: dashed and dotted borders are drawn solid.
                    let sides = [border.top, border.right, border.bottom, border.left];
                    let uniform = sides.iter().all(|s| s.width == border.top.width && s.color == border.top.color);
                    if uniform {
                        if let (true, Some(bc)) = (border.top.width > 0.0, border.top.color) {
                            draw_rounded_border(
                                self.buffer,
                                self.width,
                                self.height,
                                self.physical_width,
                                floor_i32(local_rect.x),
                                floor_i32(local_rect.y),
                                local_rect.width as i32,
                                local_rect.height as i32,
                                *border_radius,
                                border.top.width,
                                bc,
                                self.swap_rb,
                                clip,
                                self.rotation,
                            );
                        }
                    } else {
                        draw_border_sides(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            floor_i32(local_rect.x),
                            floor_i32(local_rect.y),
                            local_rect.width as i32,
                            local_rect.height as i32,
                            *border_radius,
                            border,
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }
                }
                DrawCommand::DrawText {
                    text,
                    rect,
                    color,
                    font_size,
                    weight,
                    run,
                } => {
                    profile!("render_text");
                    // Bitmap fonts lay the text out themselves.
                    #[cfg(not(feature = "std"))]
                    let _ = run;
                    let local_rect = self.translate_rect(rect);
                    let packed_color = pack_color(*color, self.swap_rb);
                    let clip = self.get_clip_rect();
                    let (clip_x1, clip_y1, clip_x2, clip_y2) = if let Some(cr) = clip {
                        (
                            cr.x.max(0.0) as i32,
                            cr.y.max(0.0) as i32,
                            (cr.x + cr.width).min(self.width as f32) as i32,
                            (cr.y + cr.height).min(self.height as f32) as i32,
                        )
                    } else {
                        (0, 0, self.width as i32, self.height as i32)
                    };

                    match &self.fonts {
                        #[cfg(feature = "std")]
                        FontSource::Ttf(ttf_fonts) => {
                            let ttf_fonts: &[Font] = ttf_fonts;
                            let Some(font_index) = ttf_font_index(ttf_fonts, *weight) else {
                                continue;
                            };
                            let target = GlyphTarget {
                                clip: (clip_x1, clip_y1, clip_x2, clip_y2),
                                packed_color,
                                swap_rb: self.swap_rb,
                                rotation: self.rotation,
                                physical_width: self.physical_width,
                                width: self.width,
                                height: self.height,
                            };
                            let px = *font_size;
                            let sub_px = (px * 16.0) as u32;

                            if let Some(run) = run {
                                // Shaped glyphs: positions are the advances.
                                profile!("text_rasterize_blend");
                                let baseline = round_i32(local_rect.y + run.baseline);
                                let emoji_top = floor_i32(local_rect.y + run.baseline - run.shaped.metrics.ascent + px * 0.10);
                                let mut pen = local_rect.x;
                                let glyphs = run.shaped.glyphs.get(run.start as usize..run.end as usize).unwrap_or(&[]);
                                for glyph in glyphs {
                                    let x = round_i32(pen);
                                    pen += glyph.advance;
                                    let (key, fallback_font, emoji) = if glyph.font == EMOJI_FONT {
                                        let c = text
                                            .get(glyph.cluster.saturating_sub(run.text_start) as usize..)
                                            .and_then(|s| s.chars().next())
                                            .unwrap_or('\u{fffd}');
                                        (emoji_cache_key(c, sub_px), font_index, Some(c))
                                    } else {
                                        ((glyph.font as usize, glyph.id, sub_px), glyph.font as usize, None)
                                    };
                                    let Some(cached) = cached_glyph(self.glyph_cache, ttf_fonts, self.color_emoji, key, fallback_font, glyph.id, px, emoji) else {
                                        continue;
                                    };
                                    let (gx, gy) = if cached.is_color {
                                        (x, emoji_top)
                                    } else {
                                        (x + cached.xmin, baseline - cached.ymin - cached.height as i32)
                                    };
                                    blit_glyph(self.buffer, cached, gx, gy, px, &target);
                                }
                            } else {
                                {
                                    profile!("text_layout");
                                    self.layout.reset(&fontdue::layout::LayoutSettings::default());
                                    self.layout.append(ttf_fonts, &fontdue::layout::TextStyle::new(text, px, font_index));
                                }
                                profile!("text_rasterize_blend");
                                let mut extra_x: f32 = 0.0;
                                for glyph in self.layout.glyphs() {
                                    if glyph.parent == '\u{fe0f}' {
                                        continue;
                                    }
                                    let emoji = is_emoji(glyph.parent, glyph.key.glyph_index).then_some(glyph.parent);
                                    let key = match emoji {
                                        Some(c) => emoji_cache_key(c, sub_px),
                                        None => (glyph.font_index, glyph.key.glyph_index, sub_px),
                                    };
                                    let Some(cached) = cached_glyph(self.glyph_cache, ttf_fonts, self.color_emoji, key, glyph.font_index, glyph.key.glyph_index, glyph.key.px, emoji) else {
                                        continue;
                                    };
                                    let gx = floor_i32(local_rect.x + glyph.x + extra_x);
                                    let gy = if cached.is_color {
                                        floor_i32(local_rect.y + glyph.key.px * 0.10)
                                    } else {
                                        floor_i32(local_rect.y + glyph.y)
                                    };
                                    if emoji.is_some() {
                                        extra_x += glyph.key.px * 0.85;
                                    }
                                    blit_glyph(self.buffer, cached, gx, gy, glyph.key.px, &target);
                                }
                            }
                        }
                        FontSource::Bitmap(bitmap_fonts) => {
                            let font = match find_best_bitmap_font(bitmap_fonts, *font_size, *weight) {
                                Some(f) => f,
                                None => continue,
                            };

                            let color_a = (packed_color >> 24) & 0xff;
                            let r = (packed_color >> 16) & 0xff;
                            let g = (packed_color >> 8) & 0xff;
                            let b = packed_color & 0xff;

                            let mut pen_x = local_rect.x;
                            let mut pen_y = local_rect.y;

                            for c in text.chars() {
                                if c == '\n' {
                                    pen_x = local_rect.x;
                                    pen_y += font.line_height;
                                    continue;
                                }

                                if let Some(glyph) = font.lookup_glyph(c) {
                                    if glyph.width > 0 && glyph.height > 0 {
                                        let gx = floor_i32(pen_x + glyph.x_offset as f32);
                                        let gy = floor_i32(pen_y + glyph.y_offset as f32);
                                        let gw = glyph.width as i32;
                                        let gh = glyph.height as i32;

                                        let start_x = gx.max(clip_x1);
                                        let start_y = gy.max(clip_y1);
                                        let end_x = (gx + gw).min(clip_x2);
                                        let end_y = (gy + gh).min(clip_y2);

                                        if start_x < end_x && start_y < end_y {
                                            if self.rotation != 0 {
                                                for y in start_y..end_y {
                                                    let src_y = (y - gy) as usize;
                                                    let src_row_offset = src_y * glyph.width as usize;
                                                    for x in start_x..end_x {
                                                        let src_x = (x - gx) as usize;
                                                        let cov = glyph.bitmap[src_row_offset + src_x];
                                                        if cov > 0 {
                                                            let a = div_255(color_a * cov as u32);
                                                            if a > 0 {
                                                                let idx = calc_pixel_index(x, y, self.rotation, self.physical_width, self.width, self.height);
                                                                if idx < self.buffer.len() {
                                                                    let inv_a = 255 - a;
                                                                    let d = self.buffer[idx];
                                                                    let dst_a = (d >> 24) & 0xff;
                                                                    let dst_r = (d >> 16) & 0xff;
                                                                    let dst_g = (d >> 8) & 0xff;
                                                                    let dst_b = d & 0xff;
                                                                    
                                                                    let res_r = div_255(r * a + dst_r * inv_a);
                                                                    let res_g = div_255(g * a + dst_g * inv_a);
                                                                    let res_b = div_255(b * a + dst_b * inv_a);
                                                                    let res_a = a + div_255(dst_a * inv_a);
                                                                    self.buffer[idx] = (res_a << 24) | (res_r << 16) | (res_g << 8) | res_b;
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            } else {
                                                for y in start_y..end_y {
                                                    let src_y = (y - gy) as usize;
                                                    let dst_row_start = (y * self.physical_width as i32 + start_x) as usize;
                                                    let draw_w = (end_x - start_x) as usize;
                                                    
                                                    let src_x_start = (start_x - gx) as usize;
                                                    let glyph_span = &glyph.bitmap[src_y * glyph.width as usize + src_x_start..src_y * glyph.width as usize + src_x_start + draw_w];
                                                    let dst_span = &mut self.buffer[dst_row_start..dst_row_start + draw_w];
                                                    blend_glyph_span(dst_span, glyph_span, packed_color);
                                                }
                                            }
                                        }
                                    }
                                    pen_x += glyph.x_advance as f32;
                                }
                            }
                        }
                    }
                }
                DrawCommand::DrawImage {
                    src,
                    rect,
                    border_radius,
                } => {
                    profile!("render_image");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();

                    // Images are loaded and decoded by the runtime; here we only convert them to
                    // the framebuffer pixel format once and draw a placeholder while missing.
                    if let Some(img) = resources.images.get(src) {
                        let up_to_date = matches!(
                            self.image_cache.get(src),
                            Some((w, h, _)) if *w == img.width && *h == img.height
                        );
                        if !up_to_date {
                            let pixels: Vec<u32> = img
                                .data
                                .chunks_exact(4)
                                .map(|c| pack_color(xerune::Color::new(c[0], c[1], c[2], c[3]), self.swap_rb))
                                .collect();
                            self.image_cache.insert(src.clone(), (img.width, img.height, pixels));
                        }
                    }

                    if let Some(&(img_w, img_h, ref img_pixels)) = self.image_cache.get(src) {
                        blit_image(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            &local_rect,
                            *border_radius,
                            img_w,
                            img_h,
                            img_pixels,
                            clip,
                            self.rotation,
                        );
                    } else {
                        let grey = pack_color(xerune::Color::new(200, 200, 200, 255), self.swap_rb);
                        blend_solid_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            floor_i32(local_rect.x),
                            floor_i32(local_rect.y),
                            local_rect.width as i32,
                            local_rect.height as i32,
                            grey,
                            clip,
                            self.rotation,
                        );
                    }
                }
                DrawCommand::DrawCheckbox { rect, checked, color } => {
                    profile!("render_checkbox");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();
                    draw_rounded_border(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        floor_i32(local_rect.x),
                        floor_i32(local_rect.y),
                        local_rect.width as i32,
                        local_rect.height as i32,
                        0.0,
                        1.0,
                        *color,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );
                    if *checked {
                        let inset = 4;
                        let inner_x = floor_i32(local_rect.x) + inset;
                        let inner_y = floor_i32(local_rect.y) + inset;
                        let inner_w = local_rect.width as i32 - inset * 2;
                        let inner_h = local_rect.height as i32 - inset * 2;
                        let packed = pack_color(*color, self.swap_rb);
                        blend_solid_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            inner_x,
                            inner_y,
                            inner_w,
                            inner_h,
                            packed,
                            clip,
                            self.rotation,
                        );
                    }
                }
                DrawCommand::DrawSlider { rect, value, color } => {
                    profile!("render_slider");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();

                    let track_h = 6.0;
                    let track_y = local_rect.y + (local_rect.height - track_h) / 2.0;
                    let bg_color = xerune::Color::new(60, 60, 60, 255);
                    draw_rounded_rect(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        floor_i32(local_rect.x),
                        track_y as i32,
                        local_rect.width as i32,
                        track_h as i32,
                        track_h / 2.0,
                        Some(bg_color),
                        None,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );

                    if *value > 0.0 {
                        let active_w = local_rect.width * value;
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            floor_i32(local_rect.x),
                            track_y as i32,
                            active_w as i32,
                            track_h as i32,
                            track_h / 2.0,
                            Some(*color),
                            None,
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }

                    let thumb_r = 10.0;
                    let thumb_x = local_rect.x + local_rect.width * value;
                    let thumb_y = local_rect.y + local_rect.height / 2.0;
                    
                    let thumb_left = (thumb_x - thumb_r) as i32;
                    let thumb_top = (thumb_y - thumb_r) as i32;
                    let thumb_size = (thumb_r * 2.0) as i32;

                    draw_rounded_rect(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        thumb_left,
                        thumb_top,
                        thumb_size,
                        thumb_size,
                        thumb_r,
                        Some(xerune::Color::WHITE),
                        None,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );

                    let shadow_color = xerune::Color::new(0, 0, 0, 50);
                    draw_rounded_border(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        thumb_left,
                        thumb_top,
                        thumb_size,
                        thumb_size,
                        thumb_r,
                        2.0,
                        shadow_color,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );
                }
                DrawCommand::DrawProgress { rect, value, max, color } => {
                    profile!("render_progress");
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();

                    let bg_color = xerune::Color::new(200, 200, 200, 255);
                    draw_rounded_rect(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        floor_i32(local_rect.x),
                        floor_i32(local_rect.y),
                        local_rect.width as i32,
                        local_rect.height as i32,
                        local_rect.height / 2.0,
                        Some(bg_color),
                        None,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );

                    let progress = (value / max).clamp(0.0, 1.0);
                    if progress > 0.0 {
                        let active_w = local_rect.width * progress;
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            floor_i32(local_rect.x),
                            floor_i32(local_rect.y),
                            active_w as i32,
                            local_rect.height as i32,
                            local_rect.height / 2.0,
                            Some(*color),
                            None,
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }
                }
                DrawCommand::DrawCanvas { id, rect, border_radius } => {
                    profile!("render_canvas");
                    let local_rect = self.translate_rect(rect);
                    if let Some(canvas) = resources.canvases.get(id) {
                        let clip = self.get_clip_rect();
                        blit_image_rgba(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            &local_rect,
                            *border_radius,
                            canvas.width,
                            canvas.height,
                            &canvas.data,
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }
                }
                // A command added to xerune after this renderer: skipped.
                _ => {}
            }
        }

        self.clip_stack.pop();
    }
}

pub fn blit_image_rgba(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect: &Rect,
    border_radius: f32,
    img_w: u32,
    img_h: u32,
    img_rgba: &[u8],
    swap_rb: bool,
    clip: Option<Rect>,
    rotation: u32,
) {
    let (clip_x1, clip_y1, clip_x2, clip_y2) = if let Some(cr) = clip {
        (
            cr.x.max(0.0) as i32,
            cr.y.max(0.0) as i32,
            (cr.x + cr.width).min(logical_w as f32) as i32,
            (cr.y + cr.height).min(logical_h as f32) as i32,
        )
    } else {
        (0, 0, logical_w as i32, logical_h as i32)
    };

    let rx = floor_i32(rect.x);
    let ry = floor_i32(rect.y);
    let rw = rect.width as i32;
    let rh = rect.height as i32;

    let start_x = rx.max(clip_x1);
    let start_y = ry.max(clip_y1);
    let end_x = (rx + rw).min(clip_x2);
    let end_y = (ry + rh).min(clip_y2);

    if start_x >= end_x || start_y >= end_y || rw <= 0 || rh <= 0 || img_w == 0 || img_h == 0 || img_rgba.is_empty() {
        return;
    }

    let scale_x = img_w as f32 / rw as f32;
    let scale_y = img_h as f32 / rh as f32;

    let r_f32 = border_radius.min(rw as f32 / 2.0).min(rh as f32 / 2.0).max(0.0);

    for py in start_y..end_y {
        let dy_offset = py - ry;
        let src_y = ((dy_offset as f32 * scale_y) as u32).min(img_h.saturating_sub(1));
        let src_row_start = (src_y * img_w) as usize;

        for px in start_x..end_x {
            let dx_offset = px - rx;
            let src_x = ((dx_offset as f32 * scale_x) as u32).min(img_w.saturating_sub(1));
            let idx = (src_row_start + src_x as usize) * 4;
            if idx + 3 >= img_rgba.len() {
                continue;
            }
            let r = img_rgba[idx] as u32;
            let g = img_rgba[idx + 1] as u32;
            let b = img_rgba[idx + 2] as u32;
            let a = img_rgba[idx + 3] as u32;
            if a == 0 {
                continue;
            }
            let pixel = if swap_rb {
                (a << 24) | (b << 16) | (g << 8) | r
            } else {
                (a << 24) | (r << 16) | (g << 8) | b
            };

            let mut coverage = 1.0;
            if r_f32 > 0.0 {
                if dx_offset < r_f32 as i32 && dy_offset < r_f32 as i32 {
                    let cx = rx as f32 + r_f32;
                    let cy = ry as f32 + r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                } else if dx_offset >= rw - r_f32 as i32 && dy_offset < r_f32 as i32 {
                    let cx = rx as f32 + rw as f32 - r_f32;
                    let cy = ry as f32 + r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                } else if dx_offset < r_f32 as i32 && dy_offset >= rh - r_f32 as i32 {
                    let cx = rx as f32 + r_f32;
                    let cy = ry as f32 + rh as f32 - r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                } else if dx_offset >= rw - r_f32 as i32 && dy_offset >= rh - r_f32 as i32 {
                    let cx = rx as f32 + rw as f32 - r_f32;
                    let cy = ry as f32 + rh as f32 - r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
                }
            }

            if coverage > 0.0 {
                let blended_pixel = if coverage < 1.0 {
                    let a_cov = ((pixel >> 24) & 0xff) as f32 * coverage;
                    (pixel & 0x00ffffff) | ((a_cov.round() as u32) << 24)
                } else {
                    pixel
                };

                let out_idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                if out_idx < buffer.len() {
                    blend_pixel(&mut buffer[out_idx], blended_pixel);
                }
            }
        }
    }
}

pub fn blit_image(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect: &Rect,
    border_radius: f32,
    img_w: u32,
    img_h: u32,
    img_pixels: &[u32],
    clip: Option<Rect>,
    rotation: u32,
) {
    let (clip_x1, clip_y1, clip_x2, clip_y2) = if let Some(cr) = clip {
        (
            cr.x.max(0.0) as i32,
            cr.y.max(0.0) as i32,
            (cr.x + cr.width).min(logical_w as f32) as i32,
            (cr.y + cr.height).min(logical_h as f32) as i32,
        )
    } else {
        (0, 0, logical_w as i32, logical_h as i32)
    };

    let rx = floor_i32(rect.x);
    let ry = floor_i32(rect.y);
    let rw = rect.width as i32;
    let rh = rect.height as i32;

    let start_x = rx.max(clip_x1);
    let start_y = ry.max(clip_y1);
    let end_x = (rx + rw).min(clip_x2);
    let end_y = (ry + rh).min(clip_y2);

    if start_x >= end_x || start_y >= end_y || rw <= 0 || rh <= 0 || img_w == 0 || img_h == 0 || img_pixels.is_empty() {
        return;
    }

    let scale_x = img_w as f32 / rw as f32;
    let scale_y = img_h as f32 / rh as f32;

    let r_f32 = border_radius.min(rw as f32 / 2.0).min(rh as f32 / 2.0).max(0.0);

    for py in start_y..end_y {
        let dy_offset = py - ry;
        let src_y = ((dy_offset as f32 * scale_y) as u32).min(img_h.saturating_sub(1));
        let src_row_start = (src_y * img_w) as usize;

        for px in start_x..end_x {
            let dx_offset = px - rx;
            let src_x = ((dx_offset as f32 * scale_x) as u32).min(img_w.saturating_sub(1));
            let idx = src_row_start + src_x as usize;
            let pixel = match img_pixels.get(idx) {
                Some(&p) => p,
                None => continue,
            };

            let mut coverage = 1.0;
            if r_f32 > 0.0 {
                if dx_offset < r_f32 as i32 && dy_offset < r_f32 as i32 {
                    let cx = rx as f32 + r_f32;
                    let cy = ry as f32 + r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx*dx + dy*dy).sqrt()).clamp(0.0, 1.0);
                }
                else if dx_offset >= rw - r_f32 as i32 && dy_offset < r_f32 as i32 {
                    let cx = rx as f32 + rw as f32 - r_f32;
                    let cy = ry as f32 + r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx*dx + dy*dy).sqrt()).clamp(0.0, 1.0);
                }
                else if dx_offset < r_f32 as i32 && dy_offset >= rh - r_f32 as i32 {
                    let cx = rx as f32 + r_f32;
                    let cy = ry as f32 + rh as f32 - r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx*dx + dy*dy).sqrt()).clamp(0.0, 1.0);
                }
                else if dx_offset >= rw - r_f32 as i32 && dy_offset >= rh - r_f32 as i32 {
                    let cx = rx as f32 + rw as f32 - r_f32;
                    let cy = ry as f32 + rh as f32 - r_f32;
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    coverage = (r_f32 + 0.5 - (dx*dx + dy*dy).sqrt()).clamp(0.0, 1.0);
                }
            }

            if coverage > 0.0 {
                let blended_pixel = if coverage < 1.0 {
                    let a = ((pixel >> 24) & 0xff) as f32 * coverage;
                    (pixel & 0x00ffffff) | ((a.round() as u32) << 24)
                } else {
                    pixel
                };

                let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                if idx < buffer.len() {
                    blend_pixel(&mut buffer[idx], blended_pixel);
                }
            }
        }
    }
}

/// Floor to i32. `as i32` truncates toward zero, which places glyphs one
/// pixel off when a tile shifts their coordinates below zero.
#[inline(always)]
fn floor_i32(v: f32) -> i32 {
    let i = v as i32;
    if (i as f32) > v { i - 1 } else { i }
}

/// Nearest integer, halves rounding up. Unlike `f32::round` (halves away from
/// zero) it commutes with integer offsets, so a band or tile places things
/// where the full frame does.
#[cfg(feature = "std")]
fn round_i32(v: f32) -> i32 {
    floor_i32(v + 0.5)
}

pub trait F32Ext {
    fn round(self) -> f32;
    fn floor(self) -> f32;
    fn ceil(self) -> f32;
    fn sqrt(self) -> f32;
}

impl F32Ext for f32 {
    #[inline(always)]
    fn round(self) -> f32 {
        #[cfg(feature = "std")]
        {
            self.round()
        }
        #[cfg(not(feature = "std"))]
        {
            if self >= 0.0 {
                (self + 0.5) as i32 as f32
            } else {
                (self - 0.5) as i32 as f32
            }
        }
    }

    #[inline(always)]
    fn floor(self) -> f32 {
        #[cfg(feature = "std")]
        {
            self.floor()
        }
        #[cfg(not(feature = "std"))]
        {
            let i = self as i32;
            if self < i as f32 {
                (i - 1) as f32
            } else {
                i as f32
            }
        }
    }

    #[inline(always)]
    fn ceil(self) -> f32 {
        #[cfg(feature = "std")]
        {
            self.ceil()
        }
        #[cfg(not(feature = "std"))]
        {
            let i = self as i32;
            if self > i as f32 {
                (i + 1) as f32
            } else {
                i as f32
            }
        }
    }

    #[inline(always)]
    fn sqrt(self) -> f32 {
        #[cfg(feature = "std")]
        {
            self.sqrt()
        }
        #[cfg(not(feature = "std"))]
        {
            if self <= 0.0 {
                return 0.0;
            }
            let mut val = self;
            for _ in 0..6 {
                val = 0.5 * (val + self / val);
            }
            val
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_resources() -> (HashMap<String, xerune::Canvas>, xerune::ImageStore) {
        (HashMap::new(), xerune::ImageStore::new())
    }

    /// Draws `commands` into a fresh `width` x `height` buffer filled with
    /// `background`, changing only `area`.
    fn draw(width: u32, height: u32, background: u32, commands: &[DrawCommand], area: Rect) -> Vec<u32> {
        let mut buffer = vec![background; (width * height) as usize];
        let (canvases, images) = no_resources();
        let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), width, height);
        SoftwareRenderer::new(Fonts::bitmap(&[])).render(&mut target, commands, &RenderResources::new(&canvases, &images), area);
        buffer
    }

    fn rect(x: f32, y: f32, width: f32, height: f32, color: xerune::Color) -> DrawCommand {
        DrawCommand::DrawRect {
            rect: Rect::new(x, y, width, height),
            color: Some(color),
            gradient: None,
            border_radius: 0.0,
            border: xerune::style::Borders::NONE,
        }
    }

    /// A scene with gradients, rounded corners, borders, a shadow and clips.
    fn scene() -> Vec<DrawCommand> {
        use xerune::style::{BorderSide, Borders};
        let gradient = |angle| {
            Some(xerune::Gradient::Linear(xerune::LinearGradient {
                angle,
                stops: Arc::from([(xerune::Color::new(200, 40, 90, 255), 0.0), (xerune::Color::new(30, 60, 220, 255), 1.0)]),
            }))
        };
        vec![
            DrawCommand::DrawRect { rect: Rect::new(0.0, 0.0, 64.0, 48.0), color: None, gradient: gradient(160.0), border_radius: 0.0, border: Borders::NONE },
            DrawCommand::DrawBoxShadow { rect: Rect::new(8.0, 8.0, 30.0, 20.0), border_radius: 6.0, offset_x: 0.0, offset_y: 3.0, blur_radius: 6.0, spread_radius: 0.0, color: xerune::Color::new(0, 0, 0, 120), inset: false },
            DrawCommand::DrawRect { rect: Rect::new(8.0, 8.0, 30.0, 20.0), color: Some(xerune::Color::new(240, 240, 240, 230)), gradient: None, border_radius: 6.0, border: Borders { top: BorderSide::solid(2.0, xerune::Color::new(90, 90, 90, 255)), ..Borders::NONE } },
            DrawCommand::Clip { rect: Rect::new(30.0, 20.0, 30.0, 25.0) },
            DrawCommand::DrawRect { rect: Rect::new(20.0, 15.0, 40.0, 30.0), color: None, gradient: gradient(90.0), border_radius: 9.0, border: Borders::NONE },
            DrawCommand::PopClip,
            rect(2.5, 40.25, 50.0, 5.5, xerune::Color::new(20, 200, 120, 160)),
        ]
    }

    #[test]
    fn dirty_culling_preserves_clip_stack() {
        let commands = [
            DrawCommand::Clip { rect: Rect::new(100.0, 0.0, 10.0, 10.0) },
            rect(0.0, 0.0, 10.0, 10.0, xerune::Color::new(255, 0, 0, 255)),
            DrawCommand::PopClip,
            rect(10.0, 0.0, 100.0, 10.0, xerune::Color::new(0, 255, 0, 255)),
        ];
        let buffer = draw(200, 20, 0xff000000, &commands, Rect::new(0.0, 0.0, 20.0, 20.0));
        assert_eq!(buffer[0], 0xff000000, "Clipped child must remain invisible");
        assert_eq!(buffer[10], 0xff00ff00, "Sibling must render after PopClip");
        assert_eq!(buffer[20], 0xff000000, "Dirty-region clip must remain active");
    }

    #[test]
    fn bands_draw_the_same_pixels_as_a_full_frame() {
        let (width, height) = (64, 48);
        let commands = scene();
        let full = draw(width, height, 0, &commands, Rect::new(0.0, 0.0, width as f32, height as f32));

        // As on a microcontroller: one 7-row band buffer, flushed after each band.
        let mut renderer = SoftwareRenderer::new(Fonts::bitmap(&[]));
        let (canvases, images) = no_resources();
        let mut screen = vec![0u32; (width * height) as usize];
        let mut band = vec![0u32; (width * 7) as usize];
        for y in (0..height).step_by(7) {
            let rows = 7.min(height - y);
            band.fill(0);
            let mut target = Surface::new(Pixels::Xrgb8888(&mut band), width, rows).with_origin(0, y as i32);
            let area = target.area();
            renderer.render(&mut target, &commands, &RenderResources::new(&canvases, &images), area);
            let start = (y * width) as usize;
            screen[start..start + (rows * width) as usize].copy_from_slice(&band[..(rows * width) as usize]);
        }
        if let Some(i) = screen.iter().zip(&full).position(|(a, b)| a != b) {
            panic!("banded rendering differs at ({}, {}): {:08x} vs {:08x}", i as u32 % width, i as u32 / width, screen[i], full[i]);
        }
    }

    #[test]
    fn a_padded_stride_draws_the_same_image() {
        let (width, height, stride) = (64u32, 48u32, 80usize);
        let commands = scene();
        let full = draw(width, height, 0, &commands, Rect::new(0.0, 0.0, width as f32, height as f32));

        let (canvases, images) = no_resources();
        let pad = 0x12345678;
        let mut padded = vec![pad; stride * height as usize];
        let mut target = Surface::new(Pixels::Xrgb8888(&mut padded), width, height).with_stride(stride);
        let area = target.area();
        SoftwareRenderer::new(Fonts::bitmap(&[])).render(&mut target, &commands, &RenderResources::new(&canvases, &images), area);
        for y in 0..height as usize {
            assert_eq!(&padded[y * stride..y * stride + width as usize], &full[y * width as usize..(y + 1) * width as usize], "row {y}");
            assert!(padded[y * stride + width as usize..(y + 1) * stride].iter().all(|p| *p == pad), "padding of row {y} was written");
        }
    }

    #[test]
    fn image_cache_follows_image_store_evictions() {
        let mut renderer = SoftwareRenderer::new(Fonts::bitmap(&[]));
        renderer.image_cache.insert("gone.png".to_string(), (1, 1, vec![0xffffffff]));
        let mut buffer = vec![0; 4];
        let (canvases, images) = no_resources();
        let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), 2, 2);
        let area = target.area();
        renderer.render(&mut target, &[], &RenderResources::new(&canvases, &images), area);
        assert!(renderer.image_cache.is_empty(), "pixels of an evicted image must be dropped");
    }

    #[test]
    fn glyph_cache_stays_within_its_limit() {
        let font = Font::from_bytes(
            include_bytes!("../../resources/fonts/Roboto-Regular.ttf") as &[u8],
            fontdue::FontSettings::default(),
        )
        .unwrap();
        let fonts = [font];
        let id = fonts[0].lookup_glyph_index('a');
        let mut cache = HashMap::new();
        // Every font size is a new entry, like an animated `font-size`.
        for size in 0..MAX_GLYPH_CACHE_ENTRIES as u32 + 10 {
            let px = 8.0 + size as f32 / 16.0;
            assert!(cached_glyph(&mut cache, &fonts, None, (0, id, px.to_bits()), 0, id, px, None).is_some());
            assert!(cache.len() <= MAX_GLYPH_CACHE_ENTRIES);
        }
    }

    #[test]
    fn border_sides_meet_on_the_corner_diagonal() {
        use xerune::style::{BorderSide, Borders};
        let (red, blue) = (xerune::Color::new(255, 0, 0, 255), xerune::Color::new(0, 0, 255, 255));
        let none = BorderSide { width: 0.0, ..BorderSide::solid(0.0, red) };
        let commands = [DrawCommand::DrawRect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            color: None,
            gradient: None,
            border_radius: 0.0,
            border: Borders { top: BorderSide::solid(4.0, red), left: BorderSide::solid(4.0, blue), right: none, bottom: none },
        }];
        let buffer = draw(20, 20, 0xffffffff, &commands, Rect::new(0.0, 0.0, 20.0, 20.0));
        let at = |x: usize, y: usize| buffer[y * 20 + x];
        assert_eq!(at(10, 1), 0xffff0000, "top band");
        assert_eq!(at(1, 10), 0xff0000ff, "left band");
        assert_eq!(at(3, 0), 0xffff0000, "corner, above the diagonal");
        assert_eq!(at(0, 3), 0xff0000ff, "corner, below the diagonal");
        let diagonal = at(2, 2);
        assert!(diagonal & 0xff > 0 && (diagonal >> 16) & 0xff > 0, "the diagonal blends both: {diagonal:08x}");
        assert_eq!(at(10, 10), 0xffffffff, "inside");
        assert_eq!(at(19, 10), 0xffffffff, "no right border");
        assert_eq!(at(10, 19), 0xffffffff, "no bottom border");
    }

    #[test]
    fn test_emoji_decoding() {
        let emoji_bytes = include_bytes!("../../resources/fonts/NotoColorEmoji.ttf");
        let font_bytes = [emoji_bytes.as_slice()];
        let emojis = ['🚀', '🎨', '⭐', '💻', '👥', '🔥', '⚙', '🛠', '⚡', '➕', '🛡'];
        for e in emojis {
            let res = try_find_color_emoji_glyph(&font_bytes, e, 34.0);
            assert!(res.is_some(), "Emoji {} failed to decode!", e);
        }
    }

    #[test]
    fn test_blit_image_empty_buffer() {
        let mut buffer = vec![0u32; 100];
        let rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        // Must not panic on zero dimensions or empty pixel slice
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 0, &[], None, 0);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 10, 0, &[], None, 0);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 10, &[], None, 0);
    }

    #[test]
    fn test_measurement_cache_is_bounded_and_evicts_old_entries() {
        let measurer = Fonts::bitmap(&xerune::font::DEFAULT_ROBOTO);

        // Simulate a long-running dashboard with 2000 changing text strings
        for i in 0..2000 {
            let text = format!("Dashboard metric #{} is 42.0", i);
            let (w, h) = measurer.measure_text(&text, 14.0, 400);
            assert!(w > 0.0);
            assert!(h > 0.0);
        }

        // Must stay bounded to MAX_MEASURE_CACHE_ENTRIES
        assert_eq!(measure_cache_len(), MAX_MEASURE_CACHE_ENTRIES);

        // Verify that measuring an evicted string again works and succeeds
        let (w, h) = measurer.measure_text("Dashboard metric #0 is 42.0", 14.0, 400);
        assert!(w > 0.0);
        assert!(h > 0.0);
        assert_eq!(measure_cache_len(), MAX_MEASURE_CACHE_ENTRIES);
    }

    fn roboto() -> Fonts {
        Fonts::ttf(
            include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
            Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
        )
        .unwrap()
    }

    #[test]
    fn shape_uses_fractional_advances() {
        let measurer = roboto();
        let text = "The quick brown fox jumps over the lazy dog";
        let shaped = measurer.shape(text, 14.0, 0);
        assert_eq!(shaped.glyphs.len(), text.chars().count());
        // Chromium lays this out 276.36px wide with Roboto 14px; rounding
        // every advance up would make it 293px.
        assert!((shaped.width() - 276.4).abs() < 1.0, "width {}", shaped.width());
        let m = shaped.metrics;
        // Chromium: `line-height: normal` is 16px for Roboto 14px.
        assert_eq!((m.ascent, m.descent, m.line_gap), (13.0, 3.0, 0.0));
        assert!(shaped.glyphs.windows(2).all(|w| w[0].cluster < w[1].cluster));
        assert_ne!(measurer.shape(text, 14.0, 1).glyphs[0].font, shaped.glyphs[0].font);
    }

    #[test]
    fn shape_is_cached() {
        let measurer = roboto();
        let a = measurer.shape("cached run", 15.0, 0);
        let b = measurer.shape("cached run", 15.0, 0);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &measurer.shape("cached run", 16.0, 0)));
    }

    #[test]
    fn font_sets_do_not_share_cached_shapes() {
        static BITMAP: [xerune::font::BitmapFont; 1] = [xerune::font::DEFAULT_ROBOTO_REGULAR];
        let (ttf, bitmap) = (roboto(), Fonts::bitmap(&BITMAP));
        let a = ttf.shape("same text", 14.0, 0);
        let b = bitmap.shape("same text", 14.0, 0);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_ne!(a.width(), b.width());
        assert_ne!(ttf.measure_text("same text", 14.0, 0), bitmap.measure_text("same text", 14.0, 0));
    }

    #[test]
    fn shape_bitmap_fonts() {
        static FONTS: [xerune::font::BitmapFont; 1] = [xerune::font::DEFAULT_ROBOTO_REGULAR];
        let fonts = &FONTS;
        let measurer = Fonts::bitmap(fonts);
        let shaped = measurer.shape("Hi you", 14.0, 0);
        let font = &fonts[0];
        let expected: f32 = "Hi you".chars().map(|c| font.lookup_glyph(c).unwrap().x_advance as f32).sum();
        assert_eq!(shaped.width(), expected);
        assert_eq!(shaped.metrics.normal_line_height(), font.line_height);
    }
}
