//! Software rasterizer for [xerune](https://crates.io/crates/xerune): draws
//! its draw commands (rects, rounded corners, borders, box shadows, linear,
//! radial and conic gradients, images and text) into a pixel buffer on the
//! CPU, with SSE2 and NEON fast paths. Made for boards without a GPU.
//!
//! [`SoftwareRenderer`] is the [`xerune::Renderer`] and owns the glyph and
//! image caches; [`Fonts`] (TrueType/OpenType, read by read-fonts, shaped
//! by harfrust, outlines filled by [`path`]) is both its glyph source and the runtime's
//! [`xerune::TextMeasurer`], so text is measured and drawn with the same
//! fonts:
//!
//! ```no_run
//! let regular = std::fs::read("Roboto-Regular.ttf").unwrap();
//! let fonts = xerune_render::Fonts::ttf(&regular, None).expect("a font");
//! let renderer = xerune_render::SoftwareRenderer::new(fonts.clone());
//! // xerune::Runtime::new(model, fonts) ... xerune::App::new(runtime).run(renderer)
//! # let _ = renderer;
//! ```

pub mod blitter;
pub mod gradient;
pub mod path;
pub mod rounded_rect;

use path::{FillRule, Path};
use read_fonts::model::Font;
use read_fonts::model::glyph::outline::{DrawMemory, DrawOptions, PathSink};
use read_fonts::types::GlyphId;
use xerune::{DrawCommand, Pixels, Rect, RenderResources, Renderer, Rotation, Surface, TextMeasurer};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use xerune::text::{FontFeatures, LineMetrics, ShapedGlyph, ShapedText};

use blitter::{pack_color, blend_solid_rect, blend_pixel, blend_glyph_span, div_255, calc_pixel_index};
use rounded_rect::{draw_rounded_rect, draw_rounded_border, draw_border_sides, draw_box_shadow, draw_check_mark};

/// TrueType/OpenType faces: the text font's (regular, then bold if there
/// is one), then the fallback fonts'. A shaped glyph names its face by its
/// index here.
#[derive(Clone)]
struct TtfFaces {
    /// Character maps, metrics, outlines and the tables harfrust shapes
    /// text with.
    faces: Vec<Font>,
    /// How many of `faces` are the text font's (1 or 2).
    primary: usize,
    /// The fallback fonts, as their (regular, bold) faces' indices.
    fallbacks: Vec<(usize, usize)>,
}

/// The fonts text is shaped and drawn with: TrueType/OpenType faces (regular
/// and bold, and fallbacks). Shared by the runtime, which measures text with
/// it, and the [`SoftwareRenderer`], which draws it; clones are cheap.
#[derive(Clone)]
pub struct Fonts(Arc<FontSet>);

#[derive(Clone)]
struct FontSet {
    /// Distinguishes font sets in the thread-wide shaping caches.
    id: u32,
    faces: TtfFaces,
    color_emoji: Option<Font>,
}

/// A font file that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontError(pub &'static str);

impl core::fmt::Display for FontError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "invalid font: {}", self.0)
    }
}

impl std::error::Error for FontError {}

impl Fonts {
    fn new(faces: TtfFaces, color_emoji: Option<Font>) -> Self {
        static NEXT_ID: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let id = NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        Self(Arc::new(FontSet { id, faces, color_emoji }))
    }

    /// TrueType/OpenType fonts: `regular` for normal text and `bold` for
    /// bold text (`regular` if `None`).
    pub fn ttf(regular: &[u8], bold: Option<&[u8]>) -> Result<Self, FontError> {
        let mut faces = TtfFaces { faces: Vec::new(), primary: 0, fallbacks: Vec::new() };
        faces.push(regular)?;
        if let Some(bold) = bold {
            faces.push(bold)?;
        }
        faces.primary = faces.faces.len();
        Ok(Self::new(faces, None))
    }

    /// Draws characters the fonts so far lack from this font, `regular` for
    /// normal text and `bold` for bold (`regular` if `None`): for a script
    /// the text font doesn't cover, like Hangul with Roboto. Lines with such
    /// characters are as tall as this font needs, as in browsers.
    pub fn with_fallback(self, regular: &[u8], bold: Option<&[u8]>) -> Result<Self, FontError> {
        let FontSet { mut faces, color_emoji, .. } = Arc::unwrap_or_clone(self.0);
        let regular_index = faces.push(regular)?;
        let bold_index = match bold {
            Some(bold) => faces.push(bold)?,
            None => regular_index,
        };
        faces.fallbacks.push((regular_index, bold_index));
        // A new id: shaping caches are per font set.
        Ok(Self::new(faces, color_emoji))
    }

    /// Draws characters the fonts lack from this color emoji font (bitmap
    /// glyphs, like NotoColorEmoji). Borrowed bytes are not copied.
    pub fn with_color_emoji(self, font: impl Into<Cow<'static, [u8]>>) -> Self {
        let mut set = Arc::unwrap_or_clone(self.0);
        set.color_emoji = match font.into() {
            Cow::Borrowed(bytes) => Font::new(bytes, 0),
            Cow::Owned(bytes) => Font::new(bytes, 0),
        };
        if set.color_emoji.is_none() {
            log::warn!("the color emoji font is not an OpenType font");
        }
        Self(Arc::new(set))
    }

    fn id(&self) -> u32 {
        self.0.id
    }

    fn color_emoji(&self) -> Option<&Font> {
        self.0.color_emoji.as_ref()
    }
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

/// How many rasterized glyphs (font, glyph, size) the glyph cache keeps
/// before it is emptied. Every distinct font size adds entries, so without a
/// limit an animated `font-size` grows the cache forever.
///
/// Configurable at compile time via `XERUNE_RENDER_MAX_GLYPH_CACHE`
/// (default: 2048, about 0.5 MB of 16 px glyphs).
pub const MAX_GLYPH_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("XERUNE_RENDER_MAX_GLYPH_CACHE"), 2048);

thread_local! {
    static SHAPE_CACHE: std::cell::RefCell<ShapeCache> =
        std::cell::RefCell::new(ShapeCache::new(MAX_SHAPE_CACHE_ENTRIES));
    /// harfrust's compiled feature lookups.
    static SHAPE_PLANS: std::cell::RefCell<Vec<(PlanKey, harfrust::ShapePlan)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// What a shape plan is for: font set, face, script and features.
type PlanKey = (u32, usize, Option<harfrust::Script>, FontFeatures);

/// How many shape plans [`SHAPE_PLANS`] keeps before starting over: a few
/// faces times the scripts they show.
const MAX_SHAPE_PLANS: usize = 32;

/// How many shaped runs [`Fonts`] keep (thread-wide), oldest evicted first.
/// Every text run of a view is shaped on each rebuild, so this should exceed
/// the number of distinct runs on screen.
///
/// Configurable at compile time via `XERUNE_RENDER_MAX_SHAPE_CACHE` (default:
/// 1024). Set to 0 to disable.
pub const MAX_SHAPE_CACHE_ENTRIES: usize =
    parse_usize_opt(option_env!("XERUNE_RENDER_MAX_SHAPE_CACHE"), 1024);

/// Font source marker for color emoji glyphs in a [`ShapedText`]; their id is
/// meaningless and the renderer looks the character up by cluster instead.
pub const EMOJI_FONT: u16 = u16::MAX;

struct ShapeCache {
    // Looked up for every text node on every rebuild: ahash rather than
    // the slower DoS-resistant default.
    entries: hashbrown::HashMap<String, Vec<(u32, u32, u16, FontFeatures, Arc<ShapedText>)>>,
    order: std::collections::VecDeque<String>,
    max_entries: usize,
}

impl ShapeCache {
    fn new(max_entries: usize) -> Self {
        Self { entries: hashbrown::HashMap::new(), order: std::collections::VecDeque::new(), max_entries }
    }

    fn get(&self, text: &str, font: u32, font_size_bits: u32, weight: u16, features: FontFeatures) -> Option<Arc<ShapedText>> {
        self.entries
            .get(text)?
            .iter()
            .find(|(f, sz, wt, ft, _)| *f == font && *sz == font_size_bits && *wt == weight && *ft == features)
            .map(|(_, _, _, _, shaped)| shaped.clone())
    }

    fn insert(&mut self, text: &str, font: u32, font_size_bits: u32, weight: u16, features: FontFeatures, shaped: Arc<ShapedText>) {
        if self.max_entries == 0 {
            return;
        }
        if let Some(list) = self.entries.get_mut(text) {
            list.push((font, font_size_bits, weight, features, shaped));
            return;
        }
        if self.entries.len() >= self.max_entries {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
        self.order.push_back(text.to_string());
        self.entries.insert(text.to_string(), vec![(font, font_size_bits, weight, features, shaped)]);
    }
}

pub fn shape_cache_len() -> usize {
    SHAPE_CACHE.with(|cache| cache.borrow().entries.len())
}

/// Forgets the thread's shaped runs, so the next layout shapes all its text
/// again: for benchmarking shaping.
pub fn clear_shape_cache() {
    SHAPE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.entries.clear();
        cache.order.clear();
    });
}

/// `font`'s line metrics at `font_size`, rounded to whole pixels, as
/// browsers do, so lines start on pixel boundaries.
fn line_metrics(font: &Font, font_size: f32) -> Option<LineMetrics> {
    let line = font.metrics().scaled_f32(font_size).h_line()?;
    Some(LineMetrics { ascent: line.ascender.round(), descent: (-line.descender).round(), line_gap: line.line_gap.round() })
}

/// `font`'s glyph for `c`, 0 if it has none.
fn glyph_index(font: &Font, c: char) -> u16 {
    font.charmap().map_unicode(c).map_or(0, |glyph| glyph.to_u32() as u16)
}

impl Fonts {
    fn shape_uncached(&self, text: &str, font_size: f32, weight: u16, features: FontFeatures) -> ShapedText {
        let mut glyphs = Vec::with_capacity(text.len());
        let mut metrics = LineMetrics::default();
        let ttf_fonts = &self.0.faces;
        let Some(font_index) = ttf_font_index(ttf_fonts, weight) else {
            return ShapedText::default();
        };
        let font = &ttf_fonts.faces[font_index];
        if let Some(m) = line_metrics(font, font_size) {
            metrics = m;
        }
        // The fallback faces glyphs came from, whose metrics the line takes too.
        let mut used_fallbacks: Vec<usize> = Vec::new();
        // Each character goes to the first face that has it; a run of
        // characters in one face is shaped as a whole: the face and
        // where the run started.
        let mut run: Option<(usize, usize)> = None;
        for (i, c) in text.char_indices() {
            // Newlines are line breaks, not glyphs.
            if c == '\u{fe0f}' || c.is_control() {
                if let Some((face, start)) = run.take() {
                    ttf_fonts.shape_run(self.id(), face, text, start..i, font_size, features, &mut glyphs);
                }
                continue;
            }
            let mut face = font_index;
            let mut id = glyph_index(font, c);
            if id == 0 && (c as u32) < 0x1F000 {
                if let Some((fallback, fallback_id)) = ttf_fonts.fallback_glyph(c, weight) {
                    (face, id) = (fallback, fallback_id);
                    if !used_fallbacks.contains(&face) {
                        used_fallbacks.push(face);
                    }
                }
            }
            if is_emoji(c, id) {
                if let Some((face, start)) = run.take() {
                    ttf_fonts.shape_run(self.id(), face, text, start..i, font_size, features, &mut glyphs);
                }
                // Characters no font has are drawn as color emoji
                // when an emoji font is available (see the renderer),
                // a bit wider than the font's missing-glyph box.
                let face = &ttf_fonts.faces[face];
                let advance = face.glyph_metrics().h_advance(GlyphId::new(id.into())) * font_size / f32::from(face.units_per_em());
                glyphs.push(ShapedGlyph { cluster: i as u32, font: EMOJI_FONT, id, advance: advance + font_size * 0.85 });
                continue;
            }
            match run {
                Some((run_face, _)) if run_face == face => {}
                _ => {
                    if let Some((face, start)) = run.take() {
                        ttf_fonts.shape_run(self.id(), face, text, start..i, font_size, features, &mut glyphs);
                    }
                    run = Some((face, i));
                }
            }
        }
        if let Some((face, start)) = run {
            ttf_fonts.shape_run(self.id(), face, text, start..text.len(), font_size, features, &mut glyphs);
        }
        for &face in &used_fallbacks {
            if let Some(m) = line_metrics(&ttf_fonts.faces[face], font_size) {
                metrics.ascent = metrics.ascent.max(m.ascent);
                metrics.descent = metrics.descent.max(m.descent);
            }
        }

        ShapedText::new(text, glyphs, metrics)
    }
}

fn ttf_font_index(fonts: &TtfFaces, weight: u16) -> Option<usize> {
    match fonts.primary {
        0 => None,
        1 => Some(0),
        _ => Some(if weight > 0 { 1 } else { 0 }),
    }
}

impl TtfFaces {
    /// Adds the face in `bytes`, returning its index.
    fn push(&mut self, bytes: &[u8]) -> Result<usize, FontError> {
        let face = Font::new(bytes.to_vec(), 0).ok_or(FontError("not an OpenType font"))?;
        self.faces.push(face);
        Ok(self.faces.len() - 1)
    }

    /// Shapes `text[range]`, all of whose characters `face` has, onto
    /// `glyphs`: kerning, ligatures and the font's other default features,
    /// and `features`. `set` is the font set's id, for the plan cache.
    #[allow(clippy::too_many_arguments)]
    fn shape_run(&self, set: u32, face: usize, text: &str, range: core::ops::Range<usize>, font_size: f32, features: FontFeatures, glyphs: &mut Vec<ShapedGlyph>) {
        let font = &self.faces[face];
        let mut buffer = harfrust::Buffer::new();
        buffer.push_str(&text[range.clone()]);
        // The text around the run, which contextual features may look at.
        buffer.set_pre_context(&text[..range.start]);
        buffer.set_post_context(&text[range.end..]);
        // No bidi yet: everything runs left to right, which keeps clusters
        // in text order as line breaking needs.
        buffer.set_direction(harfrust::Direction::LeftToRight);
        buffer.guess_segment_properties();
        let shaper = harfrust::ShaperFont::new(font);
        let user_features: Vec<_> = features.tags().map(|tag| harfrust::Feature::new(harfrust::Tag::new(&tag), 1, ..)).collect();
        let result = SHAPE_PLANS.with(|plans| {
            let mut plans = plans.borrow_mut();
            let key = (set, face, buffer.script(), features);
            let plan = match plans.iter().position(|(k, _)| *k == key) {
                Some(index) => &plans[index].1,
                None => {
                    if plans.len() >= MAX_SHAPE_PLANS {
                        plans.clear();
                    }
                    let plan = harfrust::ShapePlan::new(font, buffer.direction(), buffer.script(), None, &user_features);
                    plans.push((key, plan));
                    &plans.last().unwrap().1
                }
            };
            harfrust::shape(&shaper, &mut buffer, harfrust::ShapeOptions::new().plan(Some(plan)).features(&user_features))
        });
        if let Err(error) = result {
            log::warn!("shaping {:?} failed: {error:?}", &text[range]);
            return;
        }
        let scale = font_size / f32::from(font.units_per_em());
        for (info, position) in buffer.glyph_infos().iter().zip(buffer.glyph_positions()) {
            glyphs.push(ShapedGlyph {
                cluster: (range.start as u32) + info.cluster,
                font: face as u16,
                id: info.glyph_id as u16,
                advance: position.x_advance as f32 * scale,
            });
        }
    }

    /// The first fallback face of `weight` with a glyph for `c`, and the glyph.
    fn fallback_glyph(&self, c: char, weight: u16) -> Option<(usize, u16)> {
        self.fallbacks.iter().find_map(|&(regular, bold)| {
            let face = if weight > 0 { bold } else { regular };
            let id = glyph_index(&self.faces[face], c);
            (id != 0).then_some((face, id))
        })
    }
}

/// Whether `c` is drawn from a color emoji font: emoji-only planes, and
/// anything the text font lacks.
fn is_emoji(c: char, glyph_index: u16) -> bool {
    (c as u32) >= 0x1F000 || glyph_index == 0
}

impl TextMeasurer for Fonts {
    fn shape(&self, text: &str, font_size: f32, weight: u16, features: FontFeatures) -> Arc<ShapedText> {
        let size_bits = font_size.to_bits();
        if let Some(shaped) = SHAPE_CACHE.with(|cache| cache.borrow().get(text, self.id(), size_bits, weight, features)) {
            return shaped;
        }
        let shaped = Arc::new(self.shape_uncached(text, font_size, weight, features));
        SHAPE_CACHE.with(|cache| cache.borrow_mut().insert(text, self.id(), size_bits, weight, features, shaped.clone()));
        shaped
    }

    /// The width of `text`'s widest line and the height of its lines, as
    /// [`shape`](Self::shape) shapes it.
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }
        let shaped = self.shape(text, font_size, weight, FontFeatures::NONE);
        let lines = shaped.line_count(f32::INFINITY, xerune::text::WordBreak::Normal).max(1);
        (shaped.max_content_width(), lines as f32 * shaped.metrics.normal_line_height())
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

/// `c` from the color emoji font `font`, from its strike closest to twice
/// `target_size` (they are scaled down when drawn), if it is a PNG.
fn try_find_color_emoji_glyph(font: &Font, c: char, target_size: f32) -> Option<CachedGlyph> {
    let glyph = font.charmap().map_unicode(c)?;
    let bitmap = font.bitmap_strikes().select_bitmap(glyph, Some(target_size * 2.0))?;
    let read_fonts::model::glyph::bitmap::Pixels::Png(data) = bitmap.pixels() else {
        return None;
    };
    let mut decoder = png::Decoder::new(data);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;

    let img_w = info.width;
    let img_h = info.height;
    if img_w == 0 || img_h == 0 {
        return None;
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
        _ => return None,
    }

    Some(CachedGlyph {
        width: img_w,
        height: img_h,
        xmin: 0,
        ymin: 0,
        bitmap: Vec::new(),
        rgba_bitmap: Some(rgba_pixels),
        is_color: true,
    })
}

/// Where and how [`blit_glyph`] draws.
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

fn emoji_cache_key(c: char, sub_px: u32) -> (usize, u16, u32) {
    let emoji_id = ((c as u32) ^ ((c as u32) >> 16)) as u16;
    (0xffff_usize, emoji_id, sub_px)
}

/// Looks up a glyph in the cache, rasterizing it first if needed: a color
/// emoji for `emoji` when an emoji font has it, else glyph `id` of
/// `fonts[font_index]`. None for glyphs without pixels (spaces), which are
/// cached too, so that they aren't looked for again every frame.
#[allow(clippy::too_many_arguments)]
fn cached_glyph<'c>(
    cache: &'c mut HashMap<(usize, u16, u32), Option<CachedGlyph>>,
    fonts: &[Font],
    color_emoji: Option<&Font>,
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
        let glyph = emoji
            .zip(color_emoji)
            .and_then(|(c, font)| try_find_color_emoji_glyph(font, c, px))
            .or_else(|| rasterize_glyph(fonts.get(font_index)?, id, px));
        cache.insert(key, glyph);
    }
    cache.get(&key)?.as_ref()
}

/// Glyph `id` of `font` at `px` pixels per em, unhinted; None for glyphs
/// without pixels (spaces).
fn rasterize_glyph(font: &Font, id: u16, px: f32) -> Option<CachedGlyph> {
    let outline = font.outline_glyphs().get(GlyphId::new(id.into()))?;
    // Outlines go up from the baseline, pixels down.
    let mut path = Path::with_transform(path::Transform::scale_translate(1.0, -1.0, 0.0, 0.0));
    if let Err(error) = outline.draw(&DrawOptions::new().with_ppem(Some(px)), DrawMemory::Scratch, &mut path) {
        log::warn!("drawing glyph {id} failed: {error:?}");
        return None;
    }
    let mask = path.fill(FillRule::NonZero)?;
    Some(CachedGlyph {
        width: mask.width,
        height: mask.height,
        xmin: mask.left,
        ymin: -(mask.top + mask.height as i32),
        bitmap: mask.coverage,
        rgba_bitmap: None,
        is_color: false,
    })
}

impl PathSink for Path {
    fn move_to(&mut self, x: f32, y: f32) {
        Path::move_to(self, x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        Path::line_to(self, x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        Path::quad_to(self, cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        Path::close(self);
    }
}

/// Blends one cached glyph with its top-left corner at (gx, gy); color
/// glyphs are scaled to `px` square.
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
    /// Per kind of draw command (see [`COMMAND_KINDS`]): how many were
    /// drawn and how long they took, while profiling.
    profile: Option<CommandTimes>,
    image_cache: HashMap<String, (u32, u32, Vec<u32>)>, // src -> (width, height, packed pixels)
    glyph_cache: HashMap<(usize, u16, u32), Option<CachedGlyph>>,
    clip_stack: Vec<Rect>,
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
            profile: None,
        }
    }

    /// Forgets the rasterized glyphs, so the next frame rasterizes all its
    /// text again: for benchmarking rasterizing.
    pub fn clear_glyph_cache(&mut self) {
        self.glyph_cache.clear();
    }
}

/// The kinds of draw commands profiled, in [`CommandTimes`] order.
const COMMAND_KINDS: [&str; 6] = ["rect", "text", "image", "shadow", "check", "canvas"];

/// Count and time per kind of draw command.
type CommandTimes = [(u32, core::time::Duration); COMMAND_KINDS.len()];

/// Where `command` is counted in [`CommandTimes`]; clips aren't.
fn command_kind(command: &DrawCommand) -> Option<usize> {
    Some(match command {
        DrawCommand::DrawRect { .. } => 0,
        DrawCommand::DrawText { .. } => 1,
        DrawCommand::DrawImage { .. } => 2,
        DrawCommand::DrawBoxShadow { .. } => 3,
        DrawCommand::DrawCheckMark { .. } => 4,
        DrawCommand::DrawCanvas { .. } => 5,
        _ => return None,
    })
}

impl Renderer for SoftwareRenderer {
    fn set_profiling(&mut self, on: bool) {
        if on != self.profile.is_some() {
            self.profile = on.then(Default::default);
            if on {
                // A build without NEON is slow on ARM and easy to miss.
                eprintln!("xerune_render: SIMD {}", blitter::SIMD);
            }
        }
    }

    fn take_profile(&mut self) -> Vec<xerune::CommandProfile> {
        let Some(times) = &mut self.profile else { return Vec::new() };
        let taken = core::mem::take(times);
        COMMAND_KINDS
            .iter()
            .zip(taken)
            .filter(|(_, (count, _))| *count > 0)
            .map(|(kind, (count, time))| xerune::CommandProfile::new(kind, count, time))
            .collect()
    }

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
            fonts: &self.fonts.0.faces,
            color_emoji: self.fonts.color_emoji(),
            clip_stack: &mut self.clip_stack,
            profile: self.profile.as_mut(),
            swap_rb,
            rotate: rotation != Rotation::Deg0,
            rotation: rotation.degrees(),
            x_offset: origin.0,
            y_offset: origin.1,
            image_cache: &mut self.image_cache,
            glyph_cache: &mut self.glyph_cache,
            measurer: &self.fonts,
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
    fonts: &'a TtfFaces,
    color_emoji: Option<&'a Font>,
    clip_stack: &'a mut Vec<Rect>,
    profile: Option<&'a mut CommandTimes>,
    swap_rb: bool,
    #[allow(dead_code)]
    rotate: bool,
    rotation: u32,
    x_offset: i32,
    y_offset: i32,
    image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>,
    glyph_cache: &'a mut HashMap<(usize, u16, u32), Option<CachedGlyph>>,
    /// Shapes the text of commands that come without a glyph run.
    measurer: &'a Fonts,
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

            let started = self.profile.is_some().then(std::time::Instant::now);

            match command {
                DrawCommand::Clip { rect } => {
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

                    let faces = self.fonts;
                    let Some(font_index) = ttf_font_index(faces, *weight) else {
                        continue;
                    };
                    let ttf_fonts: &[Font] = &faces.faces;
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

                    // A command without a run shapes its text here, as
                    // one line.
                    let unshaped;
                    let run = match run {
                        Some(run) => run,
                        None => {
                            let shaped = self.measurer.shape(text, px, *weight, FontFeatures::NONE);
                            let (end, baseline) = (shaped.glyphs.len() as u32, shaped.metrics.ascent);
                            unshaped = xerune::text::GlyphRun { shaped, start: 0, end, text_start: 0, baseline };
                            &unshaped
                        }
                    };
                    // Shaped glyphs: positions are the advances.
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

                }
                DrawCommand::DrawImage {
                    src,
                    rect,
                    border_radius,
                    opacity,
                } => {
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
                            *opacity,
                            clip,
                            self.rotation,
                        );
                    } else {
                        let grey = xerune::Color::new(200, 200, 200, (255.0 * opacity.clamp(0.0, 1.0) + 0.5) as u8);
                        let grey = pack_color(grey, self.swap_rb);
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
                DrawCommand::DrawCheckMark { rect, color } => {
                    let local_rect = self.translate_rect(rect);
                    let clip = self.get_clip_rect();
                    draw_check_mark(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        local_rect,
                        *color,
                        self.swap_rb,
                        clip,
                        self.rotation,
                    );
                }
                DrawCommand::DrawCanvas { id, rect, border_radius, opacity } => {
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
                            *opacity,
                            self.swap_rb,
                            clip,
                            self.rotation,
                        );
                    }
                }
                // A command added to xerune after this renderer: skipped.
                _ => {}
            }

            // (Text skipped for a missing font isn't counted.)
            if let (Some(started), Some(times), Some(kind)) = (started, self.profile.as_deref_mut(), command_kind(command)) {
                times[kind].0 += 1;
                times[kind].1 += started.elapsed();
            }
        }

        self.clip_stack.pop();
    }
}

/// Draws RGBA pixels (a canvas) scaled into `rect`, `opacity` (0 to 1) as
/// opaque as they are.
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
    opacity: f32,
    swap_rb: bool,
    clip: Option<Rect>,
    rotation: u32,
) {
    // Unfaded pixels skip the multiply: a separate loop for faded ones.
    let opacity = opacity.clamp(0.0, 1.0);
    let blit = if opacity < 1.0 { blit_image_rgba_faded::<true> } else { blit_image_rgba_faded::<false> };
    blit(buffer, logical_w, logical_h, physical_w, rect, border_radius, img_w, img_h, img_rgba, opacity, swap_rb, clip, rotation);
}

fn blit_image_rgba_faded<const FADE: bool>(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect: &Rect,
    border_radius: f32,
    img_w: u32,
    img_h: u32,
    img_rgba: &[u8],
    opacity: f32,
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

    if start_x >= end_x || start_y >= end_y || rw <= 0 || rh <= 0 || img_w == 0 || img_h == 0 || img_rgba.is_empty() || opacity == 0.0 {
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

            if FADE {
                coverage *= opacity;
            }
            if coverage > 0.0 {
                let blended_pixel = if coverage < 1.0 {
                    let a_cov = ((pixel >> 24) & 0xff) as f32 * coverage;
                    (pixel & 0x00ffffff) | (crate::blitter::round_alpha(a_cov) << 24)
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

/// Draws a decoded image scaled into `rect`, `opacity` (0 to 1) as opaque
/// as it is.
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
    opacity: f32,
    clip: Option<Rect>,
    rotation: u32,
) {
    // Unfaded pixels skip the multiply: a separate loop for faded ones.
    let opacity = opacity.clamp(0.0, 1.0);
    let blit = if opacity < 1.0 { blit_image_faded::<true> } else { blit_image_faded::<false> };
    blit(buffer, logical_w, logical_h, physical_w, rect, border_radius, img_w, img_h, img_pixels, opacity, clip, rotation);
}

fn blit_image_faded<const FADE: bool>(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect: &Rect,
    border_radius: f32,
    img_w: u32,
    img_h: u32,
    img_pixels: &[u32],
    opacity: f32,
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

    if start_x >= end_x || start_y >= end_y || rw <= 0 || rh <= 0 || img_w == 0 || img_h == 0 || img_pixels.is_empty() || opacity == 0.0 {
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

            if FADE {
                coverage *= opacity;
            }
            if coverage > 0.0 {
                let blended_pixel = if coverage < 1.0 {
                    let a = ((pixel >> 24) & 0xff) as f32 * coverage;
                    (pixel & 0x00ffffff) | (crate::blitter::round_alpha(a) << 24)
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
fn round_i32(v: f32) -> i32 {
    floor_i32(v + 0.5)
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
        SoftwareRenderer::new(roboto()).render(&mut target, commands, &RenderResources::new(&canvases, &images), area);
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
            DrawCommand::DrawCheckMark { rect: Rect::new(44.5, 4.0, 15.0, 15.0), color: xerune::Color::WHITE },
        ]
    }

    /// Pixels of a checked 13x13 checkbox in a Chromium screenshot: white on
    /// the stroke, untouched accent beside it.
    #[test]
    fn check_mark_matches_chromium() {
        let accent = xerune::Color::new(0, 0x75, 0xff, 255);
        let commands = [rect(0.0, 0.0, 13.0, 13.0, accent), DrawCommand::DrawCheckMark { rect: Rect::new(0.0, 0.0, 13.0, 13.0), color: xerune::Color::WHITE }];
        let buffer = draw(13, 13, 0, &commands, Rect::new(0.0, 0.0, 13.0, 13.0));
        let at = |x: usize, y: usize| buffer[y * 13 + x] & 0xffffff;
        for (x, y) in [(3, 7), (4, 7), (9, 3), (5, 8)] {
            assert!(at(x, y) & 0xff0000 >= 0xe00000, "({x}, {y}) is {:06x}, not white", at(x, y));
        }
        for (x, y) in [(2, 2), (9, 9), (1, 6), (11, 2)] {
            assert_eq!(at(x, y), 0x0075ff, "({x}, {y})");
        }
    }

    /// `box-shadow: inset 0 0 0 3px` is a 3px ring inside the box, as in
    /// Chromium.
    #[test]
    fn inset_spread_shadow_is_a_ring_inside() {
        let ring = xerune::Color::new(255, 0, 0, 255);
        let commands = [
            rect(0.0, 0.0, 20.0, 20.0, xerune::Color::new(0, 0, 255, 255)),
            DrawCommand::DrawBoxShadow { rect: Rect::new(0.0, 0.0, 20.0, 20.0), border_radius: 0.0, offset_x: 0.0, offset_y: 0.0, blur_radius: 0.0, spread_radius: 3.0, color: ring, inset: true },
        ];
        let buffer = draw(20, 20, 0, &commands, Rect::new(0.0, 0.0, 20.0, 20.0));
        let at = |x: usize, y: usize| buffer[y * 20 + x] & 0xffffff;
        for (x, y) in [(0, 0), (2, 10), (17, 10), (10, 19), (10, 0)] {
            assert_eq!(at(x, y), 0xff0000, "({x}, {y}) is in the ring");
        }
        for (x, y) in [(3, 3), (10, 10), (16, 16)] {
            assert_eq!(at(x, y), 0x0000ff, "({x}, {y}) is inside it");
        }
    }

    /// A round element's outer shadow is outside the circle only, also for a
    /// radius beyond half its size, and fades out within the blur radius.
    #[test]
    fn round_shadow_is_outside_the_circle() {
        let white = xerune::Color::new(255, 255, 255, 255);
        let commands = [DrawCommand::DrawBoxShadow {
            rect: Rect::new(10.0, 10.0, 40.0, 40.0),
            border_radius: 9999.0,
            offset_x: 0.0,
            offset_y: 0.0,
            blur_radius: 8.0,
            spread_radius: 0.0,
            color: white,
            inset: false,
        }];
        let buffer = draw(60, 60, 0, &commands, Rect::new(0.0, 0.0, 60.0, 60.0));
        let at = |x: usize, y: usize| buffer[y * 60 + x] & 0xffffff;
        for (x, y) in [(30, 30), (17, 17), (42, 42), (11, 30)] {
            assert_eq!(at(x, y), 0, "({x}, {y}) is inside the circle");
        }
        for (x, y) in [(8, 30), (30, 51), (13, 13)] {
            assert_ne!(at(x, y), 0, "({x}, {y}) is in the shadow");
        }
        assert_eq!(at(0, 0), 0, "the corner is beyond the blur");
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
        let mut renderer = SoftwareRenderer::new(roboto());
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
        SoftwareRenderer::new(roboto()).render(&mut target, &commands, &RenderResources::new(&canvases, &images), area);
        for y in 0..height as usize {
            assert_eq!(&padded[y * stride..y * stride + width as usize], &full[y * width as usize..(y + 1) * width as usize], "row {y}");
            assert!(padded[y * stride + width as usize..(y + 1) * stride].iter().all(|p| *p == pad), "padding of row {y} was written");
        }
    }

    #[test]
    fn image_cache_follows_image_store_evictions() {
        let mut renderer = SoftwareRenderer::new(roboto());
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
        let fonts = [Font::new(include_bytes!("../../resources/fonts/Roboto-Regular.ttf").as_slice(), 0).unwrap()];
        let id = glyph_index(&fonts[0], 'a');
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
        let font = Font::new(include_bytes!("../../resources/fonts/NotoColorEmoji.ttf").as_slice(), 0).unwrap();
        let emojis = ['🚀', '🎨', '⭐', '💻', '👥', '🔥', '⚙', '🛠', '⚡', '➕', '🛡'];
        for e in emojis {
            let res = try_find_color_emoji_glyph(&font, e, 34.0);
            assert!(res.is_some(), "Emoji {} failed to decode!", e);
        }
    }

    #[test]
    fn test_blit_image_empty_buffer() {
        let mut buffer = vec![0u32; 100];
        let rect = Rect { x: 0.0, y: 0.0, width: 10.0, height: 10.0 };
        // Must not panic on zero dimensions or empty pixel slice
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 0, &[], 1.0, None, 0);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 10, 0, &[], 1.0, None, 0);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 10, &[], 1.0, None, 0);
    }

    fn roboto() -> Fonts {
        Fonts::ttf(
            include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
            Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
        )
        .unwrap()
    }

    /// Lines are hhea's, as in Chromium, unless OS/2 sets `USE_TYPO_METRICS`
    /// (neither font does): Roboto 1900 above and 500 below of 2048 units,
    /// the Hangul font 1160 and 288 of 1000.
    #[test]
    fn line_metrics_are_the_fonts() {
        let metrics = |bytes: &[u8]| line_metrics(&Font::new(bytes.to_vec(), 0).unwrap(), 16.0).unwrap();
        let roboto = metrics(include_bytes!("../../resources/fonts/Roboto-Regular.ttf"));
        assert_eq!((roboto.ascent, roboto.descent, roboto.line_gap), (15.0, 4.0, 0.0));
        let hangul = metrics(include_bytes!("../../resources/fonts/XeruneHangul-Regular.otf"));
        assert_eq!((hangul.ascent, hangul.descent, hangul.line_gap), (19.0, 5.0, 0.0));
    }

    #[test]
    fn shape_uses_fractional_advances() {
        let measurer = roboto();
        let text = "The quick brown fox jumps over the lazy dog";
        let shaped = measurer.shape(text, 14.0, 0, FontFeatures::NONE);
        assert_eq!(shaped.glyphs.len(), text.chars().count());
        // Chromium lays this out 276.36px wide with Roboto 14px; rounding
        // every advance up would make it 293px.
        assert!((shaped.width() - 276.4).abs() < 1.0, "width {}", shaped.width());
        let m = shaped.metrics;
        // Chromium: `line-height: normal` is 16px for Roboto 14px.
        assert_eq!((m.ascent, m.descent, m.line_gap), (13.0, 3.0, 0.0));
        assert!(shaped.glyphs.windows(2).all(|w| w[0].cluster < w[1].cluster));
        assert_ne!(measurer.shape(text, 14.0, 1, FontFeatures::NONE).glyphs[0].font, shaped.glyphs[0].font);
    }

    #[test]
    fn shape_is_cached() {
        let measurer = roboto();
        let a = measurer.shape("cached run", 15.0, 0, FontFeatures::NONE);
        let b = measurer.shape("cached run", 15.0, 0, FontFeatures::NONE);
        assert!(Arc::ptr_eq(&a, &b));
        assert!(!Arc::ptr_eq(&a, &measurer.shape("cached run", 16.0, 0, FontFeatures::NONE)));
    }

    #[test]
    fn features_switch_glyphs() {
        let measurer = roboto();
        // Roboto's digits are tabular: 1175 of 2048 units, the bold "1" too.
        let tabular = measurer.shape("1", 64.0, 1, FontFeatures::NONE);
        assert!((tabular.width() - 36.72).abs() < 0.01, "width {}", tabular.width());
        // Its proportional "1" (pnum) is narrower: 985 units.
        let proportional = measurer.shape("1", 64.0, 1, FontFeatures::PROPORTIONAL_NUMS);
        assert!((proportional.width() - 30.78).abs() < 0.01, "width {}", proportional.width());
        assert_ne!(proportional.glyphs[0].id, tabular.glyphs[0].id);
        // Plans differ by features: asking for the defaults again gets them.
        assert_eq!(measurer.shape("11", 64.0, 1, FontFeatures::NONE).width(), 2.0 * tabular.width());
    }

    #[test]
    fn fallback_fonts_draw_what_the_text_font_lacks() {
        let fonts = roboto()
            .with_fallback(
                include_bytes!("../../resources/fonts/XeruneHangul-Regular.otf"),
                Some(include_bytes!("../../resources/fonts/XeruneHangul-Bold.otf")),
            )
            .unwrap();
        let latin = fonts.shape("Today", 16.0, 0, FontFeatures::NONE);
        let mixed = fonts.shape("오늘 Today", 16.0, 0, FontFeatures::NONE);
        // Hangul from the fallback's regular (face 2) or bold (3) face, Latin from Roboto.
        let faces = |shaped: &ShapedText| shaped.glyphs.iter().map(|g| g.font).collect::<Vec<_>>();
        assert_eq!(faces(&mixed), [2, 2, 0, 0, 0, 0, 0, 0]);
        assert_eq!(faces(&fonts.shape("오늘", 16.0, 700, FontFeatures::NONE)), [3, 3]);
        assert!(mixed.glyphs.iter().all(|g| g.id != 0 && g.advance > 0.0));
        // Its Hangul is 0.92 em wide (920 units), and its line is taller.
        assert!((mixed.glyphs[0].advance - 14.72).abs() < 0.001);
        assert!(mixed.metrics.ascent > latin.metrics.ascent && mixed.metrics.descent > latin.metrics.descent);
        assert_eq!(fonts.shape("Today", 16.0, 0, FontFeatures::NONE).metrics.ascent, roboto().shape("Today", 16.0, 0, FontFeatures::NONE).metrics.ascent);
    }

    #[test]
    fn text_without_a_run_is_shaped_by_the_renderer() {
        let fonts = roboto();
        let shaped = fonts.shape("AVATAR office", 20.0, 0, FontFeatures::NONE);
        let text = |run| DrawCommand::DrawText {
            text: "AVATAR office".to_string(),
            rect: Rect { x: 4.0, y: 4.0, width: 180.0, height: 30.0 },
            color: xerune::Color::new(255, 255, 255, 255),
            font_size: 20.0,
            weight: 0,
            run,
        };
        let run = xerune::text::GlyphRun { shaped: shaped.clone(), start: 0, end: shaped.glyphs.len() as u32, text_start: 0, baseline: shaped.metrics.ascent };
        let draw = |command: DrawCommand| {
            let mut buffer = vec![0u32; 200 * 40];
            let (canvases, images) = no_resources();
            let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), 200, 40);
            let area = target.area();
            SoftwareRenderer::new(fonts.clone()).render(&mut target, &[command], &RenderResources::new(&canvases, &images), area);
            buffer
        };
        let unshaped = draw(text(None));
        assert!(unshaped.iter().any(|&p| p != 0));
        assert!(unshaped == draw(text(Some(run))), "the renderer shapes as layout does");
    }

    #[test]
    fn measure_text_is_the_shaped_size() {
        let fonts = roboto();
        let shaped = fonts.shape("AVATAR", 20.0, 0, FontFeatures::NONE);
        assert_eq!(fonts.measure_text("AVATAR", 20.0, 0), (shaped.width(), shaped.metrics.normal_line_height()));
        // The widest of its lines, each a line tall.
        let (width, height) = fonts.measure_text("AVATAR\nA", 20.0, 0);
        assert_eq!((width, height), (shaped.width(), 2.0 * shaped.metrics.normal_line_height()));
    }

    #[test]
    fn font_sets_do_not_share_cached_shapes() {
        let bold = include_bytes!("../../resources/fonts/Roboto-Bold.ttf");
        let (regular, bold) = (roboto(), Fonts::ttf(bold, None).unwrap());
        let a = regular.shape("same text", 14.0, 0, FontFeatures::NONE);
        let b = bold.shape("same text", 14.0, 0, FontFeatures::NONE);
        assert!(!Arc::ptr_eq(&a, &b));
        assert_ne!(a.width(), b.width());
        assert_ne!(regular.measure_text("same text", 14.0, 0), bold.measure_text("same text", 14.0, 0));
    }
}
