#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(not(feature = "std"))]
extern crate alloc;

pub mod blitter;
pub mod gradient;
pub mod rounded_rect;

#[cfg(feature = "std")]
use fontdue::Font;
use xerune::{Canvas, DrawCommand, Rect, Renderer, TextMeasurer};
use xerune::alloc_prelude::*;

use blitter::{pack_color, blend_solid_rect, blend_pixel, blend_glyph_span, div_255, calc_pixel_index};
use rounded_rect::{draw_rounded_rect, draw_rounded_border, draw_box_shadow};

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

impl<'a> TextMeasurer for FastMeasurer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        profile!("text_measure");
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        #[cfg(feature = "std")]
        thread_local! {
            static MEASURE_CACHE: std::cell::RefCell<HashMap<String, Vec<(u32, u16, f32, f32)>>> = std::cell::RefCell::new(HashMap::with_capacity(256));
        }

        let font_size_bits = font_size.to_bits();
        #[cfg(feature = "std")]
        let cached = MEASURE_CACHE.with(|cache| {
            if let Some(entries) = cache.borrow().get(text) {
                for &(sz, wt, w, h) in entries {
                    if sz == font_size_bits && wt == weight {
                        return Some((w, h));
                    }
                }
            }
            None
        });
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
                    if (glyph.parent as u32) == 0xFE0F || glyph.parent == '\u{fe0f}' {
                        continue;
                    }
                    let is_emoji = (glyph.parent as u32) >= 0x2000 || glyph.key.glyph_index == 0;
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

                let width = if max_x > min_x { max_x - min_x } else { 0.0 };
                
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
            cache.borrow_mut()
                .entry(text.to_string())
                .or_insert_with(Vec::new)
                .push((font_size_bits, weight, result.0, result.1));
        });

        result
    }
}

pub struct CachedGlyph {
    pub width: u32,
    pub height: u32,
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
            bitmap: Vec::new(),
            rgba_bitmap: Some(rgba_pixels),
            is_color: true,
        });
    }
    None
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
    pub image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>, // (width, height, pixels)
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
        canvases: &HashMap<String, Canvas>,
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
                self.render(commands, canvases, dirty_rect);
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
}

impl<'a> Renderer for FastRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>, _dirty_rect: Option<Rect>) {
        profile!("render_full");
        
        let tile_rect = Rect {
            x: self.x_offset as f32,
            y: self.y_offset as f32,
            width: self.width as f32,
            height: self.height as f32,
        };

        let active_clip = tile_rect;

        let local_base_clip = Rect {
            x: active_clip.x - self.x_offset as f32,
            y: active_clip.y - self.y_offset as f32,
            width: active_clip.width,
            height: active_clip.height,
        };

        self.clip_stack.push(local_base_clip);

        for command in commands {
            let cmd_bounds = command.bounds();

            if let Some(cb) = cmd_bounds {
                if !cb.intersects(&tile_rect) {
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
                    border_width,
                    border_color,
                    border_style: _,
                    border_bottom_only,
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

                    if *border_width > 0.0 {
                        if let Some(bc) = border_color {
                            if *border_bottom_only {
                                let bw = border_width.round() as i32;
                                let packed_border = pack_color(*bc, self.swap_rb);
                                blend_solid_rect(
                                    self.buffer,
                                    self.width,
                                    self.height,
                                    self.physical_width,
                                    local_rect.x as i32,
                                    local_rect.y as i32 + local_rect.height as i32 - bw,
                                    local_rect.width as i32,
                                    bw,
                                    packed_border,
                                    clip,
                                    self.rotation,
                                );
                            } else {
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
                                    *border_width,
                                    *bc,
                                    self.swap_rb,
                                    clip,
                                    self.rotation,
                                );
                            }
                        }
                    }
                }
                DrawCommand::DrawText {
                    text,
                    rect,
                    color,
                    font_size,
                    weight,
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
                            if ttf_fonts.is_empty() {
                                continue;
                            }
                            let font_index = if *weight > 0 && ttf_fonts.len() > 1 { 1 } else { 0 };

                            {
                                profile!("text_layout");
                                self.layout.reset(&fontdue::layout::LayoutSettings::default());
                                self.layout.append(ttf_fonts, &fontdue::layout::TextStyle::new(text, *font_size, font_index));
                            }

                            profile!("text_rasterize_blend");
                            let color_a = (packed_color >> 24) & 0xff;
                            let r = (packed_color >> 16) & 0xff;
                            let g = (packed_color >> 8) & 0xff;
                            let b = packed_color & 0xff;

                            let mut extra_x: f32 = 0.0;
                            for glyph in self.layout.glyphs() {
                                if (glyph.parent as u32) == 0xFE0F || glyph.parent == '\u{fe0f}' {
                                    continue;
                                }
                                let sub_px = (glyph.key.px * 16.0) as u32;
                                let is_emoji = (glyph.parent as u32) >= 0x2000 || glyph.key.glyph_index == 0;
                                let cache_key = if is_emoji {
                                    let emoji_id = ((glyph.parent as u32) ^ ((glyph.parent as u32) >> 16)) as u16;
                                    (0xffff_usize, emoji_id, sub_px)
                                } else {
                                    (glyph.font_index, glyph.key.glyph_index, sub_px)
                                };

                                if !self.glyph_cache.contains_key(&cache_key) {
                                    let mut color_glyph = None;
                                    if is_emoji {
                                        if let Some(fbs) = self.font_bytes {
                                            color_glyph = try_find_color_emoji_glyph(fbs, glyph.parent, glyph.key.px);
                                        }
                                    }

                                    if let Some(cg) = color_glyph {
                                        self.glyph_cache.insert(cache_key, cg);
                                     } else if let Some(font) = ttf_fonts.get(glyph.font_index) {
                                        let (metrics, bitmap) = font.rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
                                        if metrics.width > 0 && metrics.height > 0 {
                                            self.glyph_cache.insert(
                                                cache_key,
                                                CachedGlyph {
                                                    width: metrics.width as u32,
                                                    height: metrics.height as u32,
                                                    bitmap,
                                                    rgba_bitmap: None,
                                                    is_color: false,
                                                },
                                            );
                                        }
                                    }
                                }

                                if let Some(cached) = self.glyph_cache.get(&cache_key) {
                                    let gx = (local_rect.x + glyph.x + extra_x) as i32;
                                    let gy = if cached.is_color {
                                        (local_rect.y + glyph.key.px * 0.10) as i32
                                    } else {
                                        (local_rect.y + glyph.y) as i32
                                    };
                                    let target_sz = glyph.key.px.round().max(1.0) as i32;
                                    if cached.is_color {
                                        extra_x += glyph.key.px * 0.85;
                                    }
                                    let gw = if cached.is_color { target_sz } else { cached.width as i32 };
                                    let gh = if cached.is_color { target_sz } else { cached.height as i32 };

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
                                                            let idx = calc_pixel_index(x, y, self.rotation, self.physical_width, self.width, self.height);
                                                            if idx < self.buffer.len() {
                                                                let final_color = if self.swap_rb {
                                                                    let sr = (color_pixel >> 16) & 0xff;
                                                                    let sg = (color_pixel >> 8) & 0xff;
                                                                    let sb = color_pixel & 0xff;
                                                                    (sa << 24) | (sb << 16) | (sg << 8) | sr
                                                                } else {
                                                                    color_pixel
                                                                };
                                                                blend_pixel(&mut self.buffer[idx], final_color);
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        } else if self.rotation != 0 {
                                            for y in start_y..end_y {
                                                let src_y = (y - gy) as usize;
                                                let src_row_offset = src_y * cached.width as usize;
                                                for x in start_x..end_x {
                                                    let src_x = (x - gx) as usize;
                                                    let cov = cached.bitmap[src_row_offset + src_x];
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
                                                let glyph_span = &cached.bitmap[src_y * cached.width as usize + src_x_start..src_y * cached.width as usize + src_x_start + draw_w];
                                                let dst_span = &mut self.buffer[dst_row_start..dst_row_start + draw_w];
                                                blend_glyph_span(dst_span, glyph_span, packed_color);
                                            }
                                        }
                                    }
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
                                        let gx = (pen_x + glyph.x_offset as f32) as i32;
                                        let gy = (pen_y + glyph.y_offset as f32) as i32;
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

                    #[cfg(feature = "std")]
                    {
                        if !self.image_cache.contains_key(src) {
                            let path_to_read = src.strip_prefix('/').unwrap_or(src);
                            if let Ok(data) = std::fs::read(path_to_read) {
                                if let Ok(png_pixmap) = tiny_skia::Pixmap::decode_png(&data) {
                                    let w = png_pixmap.width();
                                    let h = png_pixmap.height();
                                    if w > 0 && h > 0 {
                                        let mut pixels = Vec::with_capacity((w * h) as usize);
                                        for chunk in png_pixmap.data().chunks_exact(4) {
                                            let r = chunk[0];
                                            let g = chunk[1];
                                            let b = chunk[2];
                                            let a = chunk[3];
                                            let col = xerune::Color::new(r, g, b, a);
                                            pixels.push(pack_color(col, self.swap_rb));
                                        }
                                        self.image_cache.insert(src.clone(), (w, h, pixels));
                                    }
                                } else {
                                    log::warn!("Failed to decode PNG image: {}", src);
                                }
                            } else {
                                log::warn!("Failed to read image file: {}", src);
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

                    #[cfg(not(feature = "std"))]
                    {
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
                    if let Some(canvas) = canvases.get(id) {
                        let mut pixels = Vec::with_capacity((canvas.width * canvas.height) as usize);
                        for chunk in canvas.data.chunks_exact(4) {
                            let r = chunk[0];
                            let g = chunk[1];
                            let b = chunk[2];
                            let a = chunk[3];
                            let col = xerune::Color::new(r, g, b, a);
                            pixels.push(pack_color(col, self.swap_rb));
                        }
                        
                        let clip = self.get_clip_rect();
                        blit_image(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            &local_rect,
                            *border_radius,
                            canvas.width,
                            canvas.height,
                            &pixels,
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
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 0, &[], None, false);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 10, 0, &[], None, false);
        blit_image(&mut buffer, 10, 10, 10, &rect, 0.0, 0, 10, &[], None, false);
    }
}
