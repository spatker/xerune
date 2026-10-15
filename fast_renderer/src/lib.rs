#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod blitter;
pub mod gradient;
pub mod rounded_rect;

#[cfg(feature = "std")]
use fontdue::Font;
use xerune::{DrawCommand, Rect, RenderResources, Renderer, TextMeasurer};
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

#[derive(Clone, Copy)]
pub enum FontSource<'a> {
    #[cfg(feature = "std")]
    Ttf(&'a [Font]),
    Bitmap(&'a [xerune::font::BitmapFont]),
}

#[cfg(feature = "std")]
impl<'a> From<&'a [Font]> for FontSource<'a> {
    fn from(fonts: &'a [Font]) -> Self {
        FontSource::Ttf(fonts)
    }
}

#[cfg(feature = "std")]
impl<'a> From<&'a Vec<Font>> for FontSource<'a> {
    fn from(fonts: &'a Vec<Font>) -> Self {
        FontSource::Ttf(fonts.as_slice())
    }
}

impl<'a> From<&'a [xerune::font::BitmapFont]> for FontSource<'a> {
    fn from(fonts: &'a [xerune::font::BitmapFont]) -> Self {
        FontSource::Bitmap(fonts)
    }
}

impl<'a> From<&'a Vec<xerune::font::BitmapFont>> for FontSource<'a> {
    fn from(fonts: &'a Vec<xerune::font::BitmapFont>) -> Self {
        FontSource::Bitmap(fonts.as_slice())
    }
}

pub struct FastMeasurer<'a> {
    pub fonts: FontSource<'a>,
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
/// Can be configured at compile time via `FAST_RENDERER_MAX_MEASURE_CACHE` (default: 64).
/// Set to 0 to disable text measurement caching.
#[cfg(feature = "std")]
pub const MAX_MEASURE_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("FAST_RENDERER_MAX_MEASURE_CACHE"), 64);

#[cfg(feature = "std")]
struct BoundedMeasureCache {
    entries: HashMap<String, Vec<(u32, u16, f32, f32)>>,
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

    fn get(&self, text: &str, font_size_bits: u32, weight: u16) -> Option<(f32, f32)> {
        if self.max_entries == 0 {
            return None;
        }
        if let Some(entries) = self.entries.get(text) {
            for &(sz, wt, w, h) in entries {
                if sz == font_size_bits && wt == weight {
                    return Some((w, h));
                }
            }
        }
        None
    }

    fn insert(&mut self, text: &str, font_size_bits: u32, weight: u16, dims: (f32, f32)) {
        if self.max_entries == 0 {
            return;
        }
        if let Some(list) = self.entries.get_mut(text) {
            if !list.iter().any(|&(sz, wt, _, _)| sz == font_size_bits && wt == weight) {
                list.push((font_size_bits, weight, dims.0, dims.1));
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
            vec![(font_size_bits, weight, dims.0, dims.1)],
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

/// How many shaped runs [`FastMeasurer::shape`] keeps, oldest evicted first.
/// Every text run of a view is shaped on each rebuild, so this should exceed
/// the number of distinct runs on screen.
///
/// Configurable at compile time via `FAST_RENDERER_MAX_SHAPE_CACHE` (default:
/// 1024). Set to 0 to disable.
#[cfg(feature = "std")]
pub const MAX_SHAPE_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("FAST_RENDERER_MAX_SHAPE_CACHE"), 1024);

/// Font source marker for color emoji glyphs in a [`ShapedText`]; their id is
/// meaningless and the renderer looks the character up by cluster instead.
pub const EMOJI_FONT: u16 = u16::MAX;

#[cfg(feature = "std")]
struct ShapeCache {
    // Looked up for every text node on every rebuild: ahash rather than
    // the slower DoS-resistant default.
    entries: hashbrown::HashMap<String, Vec<(u32, u16, Arc<ShapedText>)>>,
    order: std::collections::VecDeque<String>,
    max_entries: usize,
}

#[cfg(feature = "std")]
impl ShapeCache {
    fn new(max_entries: usize) -> Self {
        Self { entries: hashbrown::HashMap::new(), order: std::collections::VecDeque::new(), max_entries }
    }

    fn get(&self, text: &str, font_size_bits: u32, weight: u16) -> Option<Arc<ShapedText>> {
        self.entries
            .get(text)?
            .iter()
            .find(|(sz, wt, _)| *sz == font_size_bits && *wt == weight)
            .map(|(_, _, shaped)| shaped.clone())
    }

    fn insert(&mut self, text: &str, font_size_bits: u32, weight: u16, shaped: Arc<ShapedText>) {
        if self.max_entries == 0 {
            return;
        }
        if let Some(list) = self.entries.get_mut(text) {
            list.push((font_size_bits, weight, shaped));
            return;
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(text.to_string());
        self.entries.insert(text.to_string(), vec![(font_size_bits, weight, shaped)]);
    }
}

#[cfg(feature = "std")]
pub fn shape_cache_len() -> usize {
    SHAPE_CACHE.with(|cache| cache.borrow().entries.len())
}

/// Font metrics rounded to whole pixels, as browsers do, so lines start on
/// pixel boundaries.
fn rounded_metrics(ascent: f32, descent: f32, line_gap: f32) -> LineMetrics {
    LineMetrics { ascent: ascent.round(), descent: descent.round(), line_gap: line_gap.round() }
}

impl<'a> FastMeasurer<'a> {
    fn shape_uncached(&self, text: &str, font_size: f32, weight: u16) -> ShapedText {
        let mut glyphs = Vec::with_capacity(text.len());
        let mut metrics = LineMetrics::default();
        match &self.fonts {
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

impl<'a> TextMeasurer for FastMeasurer<'a> {
    fn shape(&self, text: &str, font_size: f32, weight: u16) -> Arc<ShapedText> {
        profile!("text_shape");
        #[cfg(feature = "std")]
        {
            let size_bits = font_size.to_bits();
            if let Some(shaped) = SHAPE_CACHE.with(|cache| cache.borrow().get(text, size_bits, weight)) {
                return shaped;
            }
            let shaped = Arc::new(self.shape_uncached(text, font_size, weight));
            SHAPE_CACHE.with(|cache| cache.borrow_mut().insert(text, size_bits, weight, shaped.clone()));
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

        let font_size_bits = font_size.to_bits();
        #[cfg(feature = "std")]
        let cached = MEASURE_CACHE.with(|cache| cache.borrow().get(text, font_size_bits, weight));
        #[cfg(not(feature = "std"))]
        let cached = None;

        if let Some(dims) = cached {
            return dims;
        }

        let result = match &self.fonts {
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
            cache.borrow_mut().insert(text, font_size_bits, weight, result);
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
    for fb in font_bytes_slice {
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
    font_bytes: Option<&[&[u8]]>,
    key: (usize, u16, u32),
    font_index: usize,
    id: u16,
    px: f32,
    emoji: Option<char>,
) -> Option<&'c CachedGlyph> {
    if !cache.contains_key(&key) {
        let color = emoji.zip(font_bytes).and_then(|(c, fbs)| try_find_color_emoji_glyph(fbs, c, px));
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

pub struct FastRenderer<'a> {
    pub buffer: &'a mut [u32],
    pub width: u32,
    pub height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub fonts: FontSource<'a>,
    pub font_bytes: Option<&'a [&'a [u8]]>,
    pub clip_stack: Vec<Rect>,
    pub swap_rb: bool,
    pub rotate: bool,
    pub rotation: u32,
    pub x_offset: i32,
    pub y_offset: i32,
    pub image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>, // src -> (width, height, packed pixels)
    pub glyph_cache: &'a mut HashMap<(usize, u16, u32), CachedGlyph>,
    #[cfg(feature = "std")]
    pub layout: fontdue::layout::Layout,
}

impl<'a> FastRenderer<'a> {
    pub fn new<F: Into<FontSource<'a>>>(
        buffer: &'a mut [u32],
        width: u32,
        height: u32,
        fonts: F,
        image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>,
        glyph_cache: &'a mut HashMap<(usize, u16, u32), CachedGlyph>,
    ) -> Self {
        Self {
            buffer,
            width,
            height,
            physical_width: width,
            physical_height: height,
            fonts: fonts.into(),
            font_bytes: None,
            clip_stack: Vec::new(),
            swap_rb: false,
            rotate: false,
            rotation: 0,
            x_offset: 0,
            y_offset: 0,
            image_cache,
            glyph_cache,
            #[cfg(feature = "std")]
            layout: fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown),
        }
    }

    pub fn with_rotation(mut self, physical_width: u32, physical_height: u32, rotation: u32) -> Self {
        self.physical_width = physical_width;
        self.physical_height = physical_height;
        self.rotation = rotation;
        self.rotate = rotation != 0;
        self
    }

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

    pub fn render_tiled<F>(
        &mut self,
        commands: &[DrawCommand],
        resources: &RenderResources<'_>,
        dirty_rect: Option<Rect>,
        screen_height: u32,
        mut flush_cb: F,
    ) where
        F: FnMut(i32, i32, u32, u32, &[u32]),
    {
        let mut y = 0;
        let tile_h = self.height as i32;
        while y < screen_height as i32 {
            self.y_offset = y;
            self.x_offset = 0;

            let tile_rect = Rect {
                x: 0.0,
                y: y as f32,
                width: self.width as f32,
                height: self.height as f32,
            };

            let overlap = match dirty_rect {
                Some(dr) => tile_rect.intersects(&dr),
                None => true,
            };

            if overlap {
                self.buffer.fill(0);
                self.render(commands, resources, dirty_rect);
                let actual_h = (screen_height as i32 - y).min(tile_h) as u32;
                flush_cb(0, y, self.width, actual_h, &self.buffer[.. (self.width * actual_h) as usize]);
            }

            y += tile_h;
        }
    }
}

impl<'a> TextMeasurer for FastRenderer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        let measurer = FastMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size, weight)
    }

    fn shape(&self, text: &str, font_size: f32, weight: u16) -> Arc<ShapedText> {
        FastMeasurer { fonts: self.fonts }.shape(text, font_size, weight)
    }
}

impl<'a> Renderer for FastRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand], resources: &RenderResources<'_>, dirty_rect: Option<Rect>) {
        profile!("render_full");
        
        let tile_rect = Rect {
            x: self.x_offset as f32,
            y: self.y_offset as f32,
            width: self.width as f32,
            height: self.height as f32,
        };

        let active_clip = match dirty_rect {
            Some(dr) => match tile_rect.intersect(&dr) {
                Some(inter) => inter,
                None => return,
            },
            None => tile_rect,
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
                        local_rect.x as i32,
                        local_rect.y as i32,
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
                            local_rect.x as i32,
                            local_rect.y as i32,
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
                                local_rect.x as i32,
                                local_rect.y as i32,
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
                            local_rect.x as i32,
                            local_rect.y as i32,
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
                                let baseline = (local_rect.y + run.baseline).round() as i32;
                                let emoji_top = floor_i32(local_rect.y + run.baseline - run.shaped.metrics.ascent + px * 0.10);
                                let mut pen = local_rect.x;
                                let glyphs = run.shaped.glyphs.get(run.start as usize..run.end as usize).unwrap_or(&[]);
                                for glyph in glyphs {
                                    let x = pen.round() as i32;
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
                                    let Some(cached) = cached_glyph(self.glyph_cache, ttf_fonts, self.font_bytes, key, fallback_font, glyph.id, px, emoji) else {
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
                                    let Some(cached) = cached_glyph(self.glyph_cache, ttf_fonts, self.font_bytes, key, glyph.font_index, glyph.key.glyph_index, glyph.key.px, emoji) else {
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
                    match resources.images.get(src) {
                        Some(img) => {
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
                        None => {
                            self.image_cache.remove(src);
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
                            local_rect.x as i32,
                            local_rect.y as i32,
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
                        local_rect.x as i32,
                        local_rect.y as i32,
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
                        let inner_x = local_rect.x as i32 + inset;
                        let inner_y = local_rect.y as i32 + inset;
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
                        local_rect.x as i32,
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
                            local_rect.x as i32,
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
                        local_rect.x as i32,
                        local_rect.y as i32,
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
                            local_rect.x as i32,
                            local_rect.y as i32,
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

    let rx = rect.x as i32;
    let ry = rect.y as i32;
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

    let rx = rect.x as i32;
    let ry = rect.y as i32;
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

pub trait F32Ext {
    fn round(self) -> f32;
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

    #[test]
    fn dirty_culling_preserves_clip_stack() {
        let mut buffer = vec![0xff000000; 200 * 20];
        let mut images = HashMap::new();
        let mut glyphs = HashMap::new();
        let fonts: &[xerune::BitmapFont] = &[];
        let mut renderer = FastRenderer::new(
            &mut buffer, 200, 20, fonts, &mut images, &mut glyphs,
        );
        let rect = |x, width, color| DrawCommand::DrawRect {
            rect: Rect::new(x, 0.0, width, 10.0),
            color: Some(color),
            gradient: None,
            border_radius: 0.0,
            border: xerune::style::Borders::NONE,
        };
        let commands = [
            DrawCommand::Clip { rect: Rect::new(100.0, 0.0, 10.0, 10.0) },
            rect(0.0, 10.0, xerune::Color::new(255, 0, 0, 255)),
            DrawCommand::PopClip,
            rect(10.0, 100.0, xerune::Color::new(0, 255, 0, 255)),
        ];

        renderer.render(&commands, &xerune::RenderResources::new(&HashMap::new(), &xerune::ImageStore::new()), Some(Rect::new(0.0, 0.0, 20.0, 20.0)));
        assert!(renderer.clip_stack.is_empty());
        assert_eq!(buffer[0], 0xff000000, "Clipped child must remain invisible");
        assert_eq!(buffer[10], 0xff00ff00, "Sibling must render after PopClip");
        assert_eq!(buffer[20], 0xff000000, "Dirty-region clip must remain active");
    }

    #[test]
    fn border_sides_meet_on_the_corner_diagonal() {
        use xerune::style::{BorderSide, Borders};
        let mut buffer = vec![0xffffffff; 20 * 20];
        let (mut images, mut glyphs) = (HashMap::new(), HashMap::new());
        let fonts: &[xerune::BitmapFont] = &[];
        let mut renderer = FastRenderer::new(&mut buffer, 20, 20, fonts, &mut images, &mut glyphs);
        let (red, blue) = (xerune::Color::new(255, 0, 0, 255), xerune::Color::new(0, 0, 255, 255));
        let none = BorderSide { width: 0.0, ..BorderSide::solid(0.0, red) };
        let commands = [DrawCommand::DrawRect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            color: None,
            gradient: None,
            border_radius: 0.0,
            border: Borders { top: BorderSide::solid(4.0, red), left: BorderSide::solid(4.0, blue), right: none, bottom: none },
        }];
        renderer.render(&commands, &xerune::RenderResources::new(&HashMap::new(), &xerune::ImageStore::new()), None);
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
        let fonts: &[xerune::font::BitmapFont] = &[xerune::font::DEFAULT_ROBOTO_REGULAR];
        let measurer = FastMeasurer { fonts: fonts.into() };

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

    fn roboto() -> Vec<Font> {
        let regular = include_bytes!("../../resources/fonts/Roboto-Regular.ttf") as &[u8];
        let bold = include_bytes!("../../resources/fonts/Roboto-Bold.ttf") as &[u8];
        vec![
            Font::from_bytes(regular, fontdue::FontSettings::default()).unwrap(),
            Font::from_bytes(bold, fontdue::FontSettings::default()).unwrap(),
        ]
    }

    #[test]
    fn shape_uses_fractional_advances() {
        let fonts = roboto();
        let measurer = FastMeasurer { fonts: (&fonts).into() };
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
        let fonts = roboto();
        let measurer = FastMeasurer { fonts: (&fonts).into() };
        let a = measurer.shape("cached run", 15.0, 0);
        let b = measurer.shape("cached run", 15.0, 0);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &measurer.shape("cached run", 16.0, 0)));
    }

    #[test]
    fn shape_bitmap_fonts() {
        let fonts: &[xerune::font::BitmapFont] = &[xerune::font::DEFAULT_ROBOTO_REGULAR];
        let measurer = FastMeasurer { fonts: fonts.into() };
        let shaped = measurer.shape("Hi you", 14.0, 0);
        let font = &fonts[0];
        let expected: f32 = "Hi you".chars().map(|c| font.lookup_glyph(c).unwrap().x_advance as f32).sum();
        assert_eq!(shaped.width(), expected);
        assert_eq!(shaped.metrics.normal_line_height(), font.line_height);
    }
}
