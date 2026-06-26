pub mod blitter;
pub mod gradient;
pub mod rounded_rect;

use std::collections::HashMap;
use fontdue::Font;
use xerune::{Canvas, DrawCommand, Rect, Renderer, TextMeasurer};

use blitter::{pack_color, blend_solid_rect, blend_pixel, blend_glyph_span, div_255};
use rounded_rect::{draw_rounded_rect, draw_rounded_border};

#[cfg(feature = "profile")]
macro_rules! profile {
    ($($tt:tt)*) => { coarse_prof::profile!($($tt)*); };
}

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

pub struct FastMeasurer<'a> {
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for FastMeasurer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        profile!("text_measure");
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        thread_local! {
            static MEASURE_CACHE: std::cell::RefCell<HashMap<String, Vec<(u32, u16, f32, f32)>>> = std::cell::RefCell::new(HashMap::with_capacity(256));
        }

        let font_size_bits = font_size.to_bits();
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

        if let Some(dims) = cached {
            return dims;
        }

        let font_index = if weight > 0 && self.fonts.len() > 1 { 1 } else { 0 };

        let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        layout.reset(&fontdue::layout::LayoutSettings::default());
        layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, font_size, font_index));

        let mut min_x = f32::MAX;
        let mut min_y = f32::MAX;
        let mut max_x = f32::MIN;
        let mut max_y = f32::MIN;

        for glyph in layout.glyphs() {
            let gx = glyph.x;
            let gy = glyph.y;
            let gw = glyph.width as f32;
            let gh = glyph.height as f32;

            if gx < min_x { min_x = gx; }
            if gy < min_y { min_y = gy; }
            if gx + gw > max_x { max_x = gx + gw; }
            if gy + gh > max_y { max_y = gy + gh; }
        }

        let width = if max_x > min_x { max_x - min_x } else { 0.0 };
        
        let height = if let Some(metrics) = self.fonts[font_index].horizontal_line_metrics(font_size) {
            metrics.new_line_size
        } else {
            if max_y > min_y { max_y - min_y } else { 20.0 }
        };

        let result = (width, height);
        MEASURE_CACHE.with(|cache| {
            cache.borrow_mut()
                .entry(text.to_string())
                .or_insert_with(Vec::new)
                .push((font_size_bits, weight, width, height));
        });

        result
    }
}

pub struct CachedGlyph {
    pub width: u32,
    pub height: u32,
    pub bitmap: Vec<u8>,
}

pub struct FastRenderer<'a> {
    pub buffer: &'a mut [u32],
    pub width: u32,
    pub height: u32,
    pub physical_width: u32,
    pub physical_height: u32,
    pub fonts: &'a [Font],
    pub clip_stack: Vec<Rect>,
    pub swap_rb: bool,
    pub rotate: bool,
    pub image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>, // (width, height, pixels)
    pub glyph_cache: &'a mut HashMap<(usize, u16, u32), CachedGlyph>,
    pub layout: fontdue::layout::Layout,
}

impl<'a> FastRenderer<'a> {
    pub fn new(
        buffer: &'a mut [u32],
        width: u32,
        height: u32,
        fonts: &'a [Font],
        image_cache: &'a mut HashMap<String, (u32, u32, Vec<u32>)>,
        glyph_cache: &'a mut HashMap<(usize, u16, u32), CachedGlyph>,
    ) -> Self {
        Self {
            buffer,
            width,
            height,
            physical_width: width,
            physical_height: height,
            fonts,
            clip_stack: Vec::new(),
            swap_rb: false,
            rotate: false,
            image_cache,
            glyph_cache,
            layout: fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown),
        }
    }

    fn get_clip_rect(&self) -> Option<Rect> {
        self.clip_stack.last().copied()
    }
}

impl<'a> TextMeasurer for FastRenderer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        let measurer = FastMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size, weight)
    }
}

impl<'a> Renderer for FastRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>, dirty_rect: Option<Rect>) {
        profile!("render_full");
        if let Some(dr) = dirty_rect {
            self.clip_stack.push(dr);
        }

        for command in commands {
            let cmd_bounds = command.bounds();

            if let Some(dr) = dirty_rect {
                if let Some(cb) = cmd_bounds {
                    if !cb.intersects(&dr) {
                        continue;
                    }
                }
            }

            match command {
                DrawCommand::Clip { rect } => {
                    profile!("render_clip");
                    let intersected = if let Some(top) = self.clip_stack.last() {
                        let x1 = top.x.max(rect.x);
                        let y1 = top.y.max(rect.y);
                        let x2 = (top.x + top.width).min(rect.x + rect.width);
                        let y2 = (top.y + top.height).min(rect.y + rect.height);
                        Rect {
                            x: x1,
                            y: y1,
                            width: (x2 - x1).max(0.0),
                            height: (y2 - y1).max(0.0),
                        }
                    } else {
                        *rect
                    };
                    self.clip_stack.push(intersected);
                }
                DrawCommand::PopClip => {
                    profile!("render_pop_clip");
                    self.clip_stack.pop();
                }
                DrawCommand::DrawRect {
                    rect,
                    color,
                    gradient,
                    border_radius,
                    border_width,
                    border_color,
                } => {
                    profile!("render_rect");
                    let clip = self.get_clip_rect();

                    if color.is_some() || gradient.is_some() {
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            rect.x as i32,
                            rect.y as i32,
                            rect.width as i32,
                            rect.height as i32,
                            *border_radius,
                            *color,
                            gradient.as_ref(),
                            self.swap_rb,
                            clip,
                            self.rotate,
                        );
                    }

                    if *border_width > 0.0 {
                        if let Some(bc) = border_color {
                            draw_rounded_border(
                                self.buffer,
                                self.width,
                                self.height,
                                self.physical_width,
                                rect.x as i32,
                                rect.y as i32,
                                rect.width as i32,
                                rect.height as i32,
                                *border_radius,
                                *border_width,
                                *bc,
                                self.swap_rb,
                                clip,
                                self.rotate,
                            );
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
                    let font_index = if *weight > 0 && self.fonts.len() > 1 { 1 } else { 0 };

                    {
                        profile!("text_layout");
                        self.layout.reset(&fontdue::layout::LayoutSettings::default());
                        self.layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, *font_size, font_index));
                    }

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

                    profile!("text_rasterize_blend");
                    let color_a = (packed_color >> 24) & 0xff;
                    let r = (packed_color >> 16) & 0xff;
                    let g = (packed_color >> 8) & 0xff;
                    let b = packed_color & 0xff;

                    for glyph in self.layout.glyphs() {
                        let sub_px = (glyph.key.px * 16.0) as u32;
                        let cache_key = (glyph.font_index, glyph.key.glyph_index, sub_px);

                        if !self.glyph_cache.contains_key(&cache_key) {
                            let (metrics, bitmap) = self.fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
                            if metrics.width > 0 && metrics.height > 0 {
                                self.glyph_cache.insert(
                                    cache_key,
                                    CachedGlyph {
                                        width: metrics.width as u32,
                                        height: metrics.height as u32,
                                        bitmap,
                                    },
                                );
                            }
                        }

                        if let Some(cached) = self.glyph_cache.get(&cache_key) {
                            let gx = (rect.x + glyph.x) as i32;
                            let gy = (rect.y + glyph.y) as i32;
                            let gw = cached.width as i32;
                            let gh = cached.height as i32;

                            let start_x = gx.max(clip_x1);
                            let start_y = gy.max(clip_y1);
                            let end_x = (gx + gw).min(clip_x2);
                            let end_y = (gy + gh).min(clip_y2);

                            if start_x < end_x && start_y < end_y {
                                if self.rotate {
                                    for y in start_y..end_y {
                                        let src_y = (y - gy) as usize;
                                        let src_row_offset = src_y * cached.width as usize;
                                        for x in start_x..end_x {
                                            let src_x = (x - gx) as usize;
                                            let cov = cached.bitmap[src_row_offset + src_x];
                                            if cov > 0 {
                                                let a = div_255(color_a * cov as u32);
                                                if a > 0 {
                                                    let idx = (x as usize * self.physical_width as usize) + (self.physical_width as usize - 1 - y as usize);
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
                DrawCommand::DrawImage {
                    src,
                    rect,
                    border_radius,
                } => {
                    profile!("render_image");
                    if !self.image_cache.contains_key(src) {
                        if let Ok(data) = std::fs::read(src) {
                            if let Ok(png_pixmap) = tiny_skia::Pixmap::decode_png(&data) {
                                let w = png_pixmap.width();
                                let h = png_pixmap.height();
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
                            } else {
                                log::warn!("Failed to decode PNG image: {}", src);
                            }
                        } else {
                            log::warn!("Failed to read image file: {}", src);
                        }
                    }

                    if let Some(&(img_w, img_h, ref img_pixels)) = self.image_cache.get(src) {
                        let clip = self.get_clip_rect();
                        blit_image(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            rect,
                            *border_radius,
                            img_w,
                            img_h,
                            img_pixels,
                            clip,
                            self.rotate,
                        );
                    } else {
                        let clip = self.get_clip_rect();
                        let grey = pack_color(xerune::Color::new(200, 200, 200, 255), self.swap_rb);
                        blend_solid_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            rect.x as i32,
                            rect.y as i32,
                            rect.width as i32,
                            rect.height as i32,
                            grey,
                            clip,
                            self.rotate,
                        );
                    }
                }
                DrawCommand::DrawCheckbox { rect, checked, color } => {
                    profile!("render_checkbox");
                    let clip = self.get_clip_rect();
                    draw_rounded_border(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                        0.0,
                        1.0,
                        *color,
                        self.swap_rb,
                        clip,
                        self.rotate,
                    );
                    if *checked {
                        let inset = 4;
                        let inner_x = rect.x as i32 + inset;
                        let inner_y = rect.y as i32 + inset;
                        let inner_w = rect.width as i32 - inset * 2;
                        let inner_h = rect.height as i32 - inset * 2;
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
                            self.rotate,
                        );
                    }
                }
                DrawCommand::DrawSlider { rect, value, color } => {
                    profile!("render_slider");
                    let clip = self.get_clip_rect();

                    let track_h = 6.0;
                    let track_y = rect.y + (rect.height - track_h) / 2.0;
                    let bg_color = xerune::Color::new(60, 60, 60, 255);
                    draw_rounded_rect(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        rect.x as i32,
                        track_y as i32,
                        rect.width as i32,
                        track_h as i32,
                        track_h / 2.0,
                        Some(bg_color),
                        None,
                        self.swap_rb,
                        clip,
                        self.rotate,
                    );

                    if *value > 0.0 {
                        let active_w = rect.width * value;
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            rect.x as i32,
                            track_y as i32,
                            active_w as i32,
                            track_h as i32,
                            track_h / 2.0,
                            Some(*color),
                            None,
                            self.swap_rb,
                            clip,
                            self.rotate,
                        );
                    }

                    let thumb_r = 10.0;
                    let thumb_x = rect.x + rect.width * value;
                    let thumb_y = rect.y + rect.height / 2.0;
                    
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
                        self.rotate,
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
                        self.rotate,
                    );
                }
                DrawCommand::DrawProgress { rect, value, max, color } => {
                    profile!("render_progress");
                    let clip = self.get_clip_rect();

                    let bg_color = xerune::Color::new(200, 200, 200, 255);
                    draw_rounded_rect(
                        self.buffer,
                        self.width,
                        self.height,
                        self.physical_width,
                        rect.x as i32,
                        rect.y as i32,
                        rect.width as i32,
                        rect.height as i32,
                        rect.height / 2.0,
                        Some(bg_color),
                        None,
                        self.swap_rb,
                        clip,
                        self.rotate,
                    );

                    let progress = (value / max).clamp(0.0, 1.0);
                    if progress > 0.0 {
                        let active_w = rect.width * progress;
                        draw_rounded_rect(
                            self.buffer,
                            self.width,
                            self.height,
                            self.physical_width,
                            rect.x as i32,
                            rect.y as i32,
                            active_w as i32,
                            rect.height as i32,
                            rect.height / 2.0,
                            Some(*color),
                            None,
                            self.swap_rb,
                            clip,
                            self.rotate,
                        );
                    }
                }
                DrawCommand::DrawCanvas { id, rect } => {
                    profile!("render_canvas");
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
                            rect,
                            0.0,
                            canvas.width,
                            canvas.height,
                            &pixels,
                            clip,
                            self.rotate,
                        );
                    }
                }
            }
        }

        if dirty_rect.is_some() {
            self.clip_stack.pop();
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
    rotate: bool,
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

    if start_x >= end_x || start_y >= end_y || rw <= 0 || rh <= 0 {
        return;
    }

    let scale_x = img_w as f32 / rw as f32;
    let scale_y = img_h as f32 / rh as f32;

    let r_f32 = border_radius.min(rw as f32 / 2.0).min(rh as f32 / 2.0).max(0.0);

    for py in start_y..end_y {
        let dy_offset = py - ry;
        let src_y = ((dy_offset as f32 * scale_y) as u32).min(img_h - 1);
        let src_row_start = (src_y * img_w) as usize;

        for px in start_x..end_x {
            let dx_offset = px - rx;
            let src_x = ((dx_offset as f32 * scale_x) as u32).min(img_w - 1);
            let pixel = img_pixels[src_row_start + src_x as usize];

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

                let idx = if rotate {
                    (px as usize * physical_w as usize) + (physical_w as usize - 1 - py as usize)
                } else {
                    (py as usize * physical_w as usize) + px as usize
                };

                if idx < buffer.len() {
                    blend_pixel(&mut buffer[idx], blended_pixel);
                }
            }
        }
    }
}
