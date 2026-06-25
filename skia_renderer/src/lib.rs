use xerune::{Canvas, DrawCommand, TextMeasurer, Renderer};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint, Mask, PathBuilder, FillRule, PixmapRef};
use std::collections::HashMap;

#[cfg(feature = "profile")]
macro_rules! profile {
    ($($tt:tt)*) => { coarse_prof::profile!($($tt)*); };
}

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

pub struct TinySkiaMeasurer<'a> {
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for TinySkiaMeasurer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        // Simple font selection: 0 = Regular, >0 = Bold (if available)
        let font_index = if weight > 0 && self.fonts.len() > 1 { 1 } else { 0 };

        let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        layout.reset(&fontdue::layout::LayoutSettings {
            ..fontdue::layout::LayoutSettings::default()
        });
        layout.append(&self.fonts[..], &fontdue::layout::TextStyle::new(text, font_size, font_index));

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
        
        // Use font metrics for stable height
        let height = if let Some(metrics) = self.fonts[font_index].horizontal_line_metrics(font_size) {
            metrics.new_line_size
        } else {
            if max_y > min_y { max_y - min_y } else { 20.0 }
        };

        (width, height)
    }
}

pub struct TinySkiaRenderer<'a> {
    pub pixmap: tiny_skia::PixmapMut<'a>,
    pub fonts: &'a [Font],
    pub clip_stack: Vec<tiny_skia::Rect>,
    pub current_mask: Option<Mask>,
    pub clip_mask_dirty: bool,
    pub image_cache: &'a mut HashMap<String, Pixmap>,
    pub gradient_cache: &'a mut HashMap<String, Pixmap>,
    pub glyph_cache: &'a mut HashMap<(usize, u16, u32, [u8; 4]), Pixmap>,
    pub layout: fontdue::layout::Layout,
    pub swap_rb: bool,
    pub transform: Transform,
}

impl<'a> TinySkiaRenderer<'a> {
    pub fn new(
        pixmap: tiny_skia::PixmapMut<'a>,
        fonts: &'a [Font],
        image_cache: &'a mut HashMap<String, Pixmap>,
        gradient_cache: &'a mut HashMap<String, Pixmap>,
        glyph_cache: &'a mut HashMap<(usize, u16, u32, [u8; 4]), Pixmap>,
    ) -> Self {
        profile!("renderer_new");
        Self {
            pixmap,
            fonts,
            clip_stack: Vec::new(),
            current_mask: None,
            clip_mask_dirty: true,
            image_cache,
            gradient_cache,
            glyph_cache,
            layout: fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown),
            swap_rb: false,
            transform: Transform::identity(),
        }
    }

    fn to_skia_color(&self, color: xerune::Color) -> tiny_skia::Color {
        if self.swap_rb {
            tiny_skia::Color::from_rgba8(color.b, color.g, color.r, color.a)
        } else {
            tiny_skia::Color::from_rgba8(color.r, color.g, color.b, color.a)
        }
    }

    fn update_clip_mask(&mut self) {
        self.clip_mask_dirty = true;
    }

    fn generate_mask(&mut self) {
        profile!("clip_mask_generate");
        self.clip_mask_dirty = false;
        if self.clip_stack.is_empty() {
            self.current_mask = None;
            return;
        }

        // Calculate intersection
        let mut intersect = self.clip_stack[0];
        for r in self.clip_stack.iter().skip(1) {
            if let Some(i) = intersect.intersect(r) {
                intersect = i;
            } else {
                // Empty intersection -> empty mask (draw nothing).
                if let Some(mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) {
                     self.current_mask = Some(mask);
                }
                return;
            }
        }

        // Optimize: skip mask creation if the clip covers the entire physical pixmap
        let path = PathBuilder::from_rect(intersect);
        if let Some(phys_bounds) = path.bounds().transform(self.transform) {
            let pm_w = self.pixmap.width() as f32;
            let pm_h = self.pixmap.height() as f32;
            let covers = phys_bounds.x() <= 0.1 && phys_bounds.y() <= 0.1 && 
                         phys_bounds.right() >= pm_w - 0.1 && phys_bounds.bottom() >= pm_h - 0.1;
            if covers {
                 self.current_mask = None;
                 return;
            }
        }

        // Create mask
        if let Some(mut mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) {
             mask.fill_path(&path, FillRule::Winding, true, self.transform); // true = anti-alias
             self.current_mask = Some(mask);
        }
    }

    fn is_fully_inside_clip(&self, logical_bounds: tiny_skia::Rect) -> bool {
        profile!("clip_mask_is_fully_inside");
        if let Some(intersect) = self.get_clip_rect() {
            if intersect.width() <= 0.0 || intersect.height() <= 0.0 {
                return false;
            }
            let eps = 0.5; // Epsilon tolerance for edge-touching float elements
            return intersect.x() - eps <= logical_bounds.x() && 
                   intersect.y() - eps <= logical_bounds.y() && 
                   intersect.right() + eps >= logical_bounds.right() && 
                   intersect.bottom() + eps >= logical_bounds.bottom();
        }
        true
    }

    fn get_clip_rect(&self) -> Option<tiny_skia::Rect> {
        if self.clip_stack.is_empty() {
            return None;
        }
        let mut intersect = self.clip_stack[0];
        for clip_rect in self.clip_stack.iter().skip(1) {
            if let Some(i) = intersect.intersect(clip_rect) {
                intersect = i;
            } else {
                return tiny_skia::Rect::from_xywh(0.0, 0.0, 0.0, 1.0); // Degenerate to discard
            }
        }
        Some(intersect)
    }
}

impl<'a> TextMeasurer for TinySkiaRenderer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        profile!("text_measure");
        let measurer = TinySkiaMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size, weight)
    }
}

impl<'a> Renderer for TinySkiaRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>, dirty_rect: Option<xerune::Rect>) {
        profile!("render_full");
        if let Some(dr) = dirty_rect {
            if let Some(tr) = tiny_skia::Rect::from_xywh(dr.x, dr.y, dr.width, dr.height) {
                self.clip_stack.push(tr);
                self.update_clip_mask();
            }
        }

        for command in commands {
            let cmd_bounds = command.bounds();

            // Optimization: Skip drawing commands that are strictly outside the dirty_rect
            if let Some(dr) = dirty_rect {
                if let Some(cb) = cmd_bounds {
                    // Only draw commands that actually intersect the dirty region
                    if !cb.intersects(&dr) {
                        continue;
                    }
                }
            }
            
            // Extracted strictly un-padded optical bounds without safety bleeds
            let item_rect = match command {
                DrawCommand::Clip { rect } => Some(*rect),
                DrawCommand::PopClip => None,
                DrawCommand::DrawRect { rect, .. } => Some(*rect),
                DrawCommand::DrawText { rect, .. } => Some(*rect),
                DrawCommand::DrawImage { rect, .. } => Some(*rect),
                DrawCommand::DrawCheckbox { rect, .. } => Some(*rect),
                DrawCommand::DrawSlider { rect, .. } => Some(*rect),
                DrawCommand::DrawProgress { rect, .. } => Some(*rect),
                DrawCommand::DrawCanvas { rect, .. } => Some(*rect),
            };

            let needs_mask = if match command { DrawCommand::Clip {..} | DrawCommand::PopClip => true, _ => false } {
                false // Ignore for mask-adjusting commands
            } else if let Some(r) = item_rect {
                 let strict_rect = tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height);
                 if let Some(r) = strict_rect {
                     !self.is_fully_inside_clip(r)
                 } else { true }
            } else { true };
            
            if needs_mask && self.clip_mask_dirty {
                self.generate_mask();
            }
            let mask_to_use = if needs_mask { self.current_mask.as_ref() } else { None };

            match command {
                DrawCommand::Clip { rect } => {
                    profile!("render_clip");
                    if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                        self.clip_stack.push(r);
                        self.clip_mask_dirty = true;
                    }
                }
                DrawCommand::PopClip => {
                    profile!("render_pop_clip");
                    self.clip_stack.pop();
                    self.clip_mask_dirty = true;
                }
                DrawCommand::DrawText { text, rect, color, font_size, weight } => {
                    profile!("render_text");
                    let font_index = if *weight > 0 && self.fonts.len() > 1 { 1 } else { 0 };

                    {
                        profile!("text_layout");
                        self.layout.reset(&fontdue::layout::LayoutSettings {
                            ..fontdue::layout::LayoutSettings::default()
                        });
                        self.layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, *font_size, font_index));
                    }

                    let color_skia = self.to_skia_color(*color);

                    profile!("text_rasterize");
                    for glyph in self.layout.glyphs() {
                        let sub_px = (glyph.key.px * 16.0) as u32; // cache at subpixel alignment or just int
                        let r_u8 = (color_skia.red() * 255.0).round() as u8;
                        let g_u8 = (color_skia.green() * 255.0).round() as u8;
                        let b_u8 = (color_skia.blue() * 255.0).round() as u8;
                        let a_u8 = (color_skia.alpha() * 255.0).round() as u8;
                        let cache_key = (glyph.font_index, glyph.key.glyph_index, sub_px, [r_u8, g_u8, b_u8, a_u8]);

                        if !self.glyph_cache.contains_key(&cache_key) {
                            let (metrics, bitmap) = self.fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
                            if metrics.width > 0 && metrics.height > 0 {
                                if let Some(mut glyph_pixmap) = Pixmap::new(metrics.width as u32, metrics.height as u32) {
                                    let data = glyph_pixmap.data_mut();
                                    for (i, alpha) in bitmap.iter().enumerate() {
                                        let a = *alpha as f32 / 255.0;
                                        let r = (color_skia.red() * a * 255.0) as u8;
                                        let g = (color_skia.green() * a * 255.0) as u8;
                                        let b = (color_skia.blue() * a * 255.0) as u8;
                                        let a_byte = (color_skia.alpha() * a * 255.0) as u8;

                                        data[i*4 + 0] = r;
                                        data[i*4 + 1] = g;
                                        data[i*4 + 2] = b;
                                        data[i*4 + 3] = a_byte;
                                    }
                                    self.glyph_cache.insert(cache_key, glyph_pixmap);
                                }
                            }
                        }

                        if let Some(glyph_pixmap) = self.glyph_cache.get(&cache_key) {
                            let gx = rect.x + glyph.x;
                            let gy = rect.y + glyph.y;

                            self.pixmap.draw_pixmap(
                                gx as i32,
                                gy as i32,
                                glyph_pixmap.as_ref(),
                                &PixmapPaint::default(),
                                self.transform,
                                mask_to_use,
                            );
                        }
                    }
                }
                DrawCommand::DrawRect { rect, color, gradient, border_radius, border_width, border_color } => {
                    profile!("render_rect");
                    let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                    if let Some(r) = r {
                        // 1. Fill (Color or Gradient)
                        let mut paint = tiny_skia::Paint::default();
                        paint.anti_alias = false;

                        if let Some(grad) = gradient {
                             // Gradient logic
                             let width_int = rect.width.max(1.0) as u32;
                             let height_int = rect.height.max(1.0) as u32;
                             let cache_key = format!("grad_{}_{}_{}", grad.angle, width_int, height_int);
                             
                             if !self.gradient_cache.contains_key(&cache_key) {
                                 if let Some(mut grad_pixmap) = Pixmap::new(width_int, height_int) {
                                     let stops: Vec<tiny_skia::GradientStop> = grad.stops.iter().map(|(c, p)| {
                                         tiny_skia::GradientStop::new(*p, self.to_skia_color(*c))
                                     }).collect();
                                     
                                     // Create gradient relative to 0,0 for caching
                                     let gcx = width_int as f32 / 2.0;
                                     let gcy = height_int as f32 / 2.0;
                                     let (gsx, gsy, gex, gey) = if (grad.angle - 180.0).abs() < 5.0 {
                                         (gcx, 0.0, gcx, height_int as f32)
                                     } else if (grad.angle - 90.0).abs() < 5.0 {
                                         (0.0, gcy, width_int as f32, gcy)
                                     } else {
                                         (gcx, 0.0, gcx, height_int as f32)
                                     };

                                     if let Some(shader) = tiny_skia::LinearGradient::new(
                                         tiny_skia::Point::from_xy(gsx, gsy),
                                         tiny_skia::Point::from_xy(gex, gey),
                                         stops,
                                         tiny_skia::SpreadMode::Pad,
                                         Transform::identity(),
                                     ) {
                                         let mut grad_paint = tiny_skia::Paint::default();
                                         grad_paint.shader = shader;
                                         grad_paint.blend_mode = tiny_skia::BlendMode::Source;
                                         let grad_rect = tiny_skia::Rect::from_xywh(0.0, 0.0, width_int as f32, height_int as f32).unwrap();
                                         grad_pixmap.fill_rect(grad_rect, &grad_paint, Transform::identity(), None);
                                     }
                                     self.gradient_cache.insert(cache_key.clone(), grad_pixmap);
                                 }
                             }
                             
                             if let Some(cached_grad) = self.gradient_cache.get(&cache_key) {
                                 let shader = tiny_skia::Pattern::new(
                                     cached_grad.as_ref(),
                                     tiny_skia::SpreadMode::Pad,
                                     tiny_skia::FilterQuality::Bilinear,
                                     1.0,
                                     self.transform.pre_translate(rect.x, rect.y),
                                 );
                                 paint.shader = shader;
                             }
                        } else if let Some(c) = color {
                            paint.set_color(self.to_skia_color(*c));
                        } else {
                            // No fill or transparent
                             paint.set_color_rgba8(0, 0, 0, 0);
                        }

                        // Fill Path
                        if gradient.is_some() || color.is_some() {
                             if *border_radius > 0.0 {
                                if let Some(path) = rounded_rect_path(r, *border_radius) {
                                    self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                                }
                            } else {
                                 let mut clamped_r = r;
                                 let mut m = mask_to_use;
                                 let mut should_draw = true;
                                 
                                 if mask_to_use.is_some() {
                                     // Hardware bypass for axis-aligned shapes: clamp them directly!
                                     if let Some(clip) = self.get_clip_rect() {
                                         if let Some(intersected) = clamped_r.intersect(&clip) {
                                             clamped_r = intersected;
                                             m = None; // It is strictly clamped geometries now, no alpha-mask needed!
                                         } else {
                                             should_draw = false; // Outside bounds
                                         }
                                     }
                                 }
                                 
                                 if should_draw {
                                     self.pixmap.fill_rect(clamped_r, &paint, self.transform, m);
                                 }
                            }
                        }

                        // 2. Stroke (Border)
                        if *border_width > 0.0 {
                             if let Some(bc) = border_color {
                                 let mut stroke_paint = tiny_skia::Paint::default();
                                 stroke_paint.set_color(self.to_skia_color(*bc));
                                 stroke_paint.anti_alias = true;
                                 
                                 let mut stroke = tiny_skia::Stroke::default();
                                 stroke.width = *border_width;
                                 
                                 if *border_radius > 0.0 {
                                     if let Some(path) = rounded_rect_path(r, *border_radius) {
                                         self.pixmap.stroke_path(&path, &stroke_paint, &stroke, self.transform, mask_to_use);
                                     }
                                 } else {
                                    // Path from rect
                                     let path = tiny_skia::PathBuilder::from_rect(r);
                                     self.pixmap.stroke_path(&path, &stroke_paint, &stroke, self.transform, mask_to_use);
                                 }
                             }
                        }
                    }
                }
                DrawCommand::DrawImage { src, rect, border_radius } => {
                    profile!("render_image");
                    if !self.image_cache.contains_key(src) {
                        if let Ok(data) = std::fs::read(src) {
                            if let Ok(png_pixmap) = Pixmap::decode_png(&data) {
                                self.image_cache.insert(src.clone(), png_pixmap);
                            } else {
                                log::warn!("Failed to decode PNG: {}", src);
                            }
                        } else {
                            log::warn!("Failed to read image file: {}", src);
                        }
                    }

                     if let Some(png_pixmap) = self.image_cache.get(src) {
                         let sx = rect.width / png_pixmap.width() as f32;
                         let sy = rect.height / png_pixmap.height() as f32;
                         let transform = self.transform.pre_scale(sx, sy).pre_translate(rect.x / sx, rect.y / sy);
                             
                             // Proper clipping for rounded corners on image needs a mask or clip_path.
                             // We create a shader from the image and fill the rounded rect path.
                             
                             if *border_radius > 0.0 {
                                 if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                                     if let Some(path) = rounded_rect_path(r, *border_radius) {
                                          let mut paint = tiny_skia::Paint::default();
                                          paint.anti_alias = false;
                                          
                                          // Use a Pattern shader to draw the image within the rounded rect path.
                                          // The transform maps the image to the rect's coordinates and scale.
                                           
                                           let shader = tiny_skia::Pattern::new(
                                               png_pixmap.as_ref(),
                                               tiny_skia::SpreadMode::Pad, 
                                               tiny_skia::FilterQuality::Bilinear, 
                                               1.0, 
                                               transform // Transform applied to pattern
                                           );
                                           paint.shader = shader;
                                           self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), mask_to_use);
                                     }
                                 }
                             } else {
                                 self.pixmap.draw_pixmap(
                                     0, 0,
                                     png_pixmap.as_ref(), // Use internal identity if pre-transformed
                                     &PixmapPaint::default(),
                                     transform,
                                     mask_to_use
                                 );
                             }
                    } else {
                        // Fallback
                        let mut paint = tiny_skia::Paint::default();
                        paint.anti_alias = false;
                        paint.set_color_rgba8(200, 200, 200, 255);
                        if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                           self.pixmap.fill_rect(r, &paint, self.transform, mask_to_use);
                        }
                    }
                }
                DrawCommand::DrawCheckbox { rect, checked, color } => {
                     profile!("render_checkbox");
                     let mut paint = tiny_skia::Paint::default();
                     paint.anti_alias = false;
                     paint.set_color(self.to_skia_color(*color));
                     
                     let wrapper = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                     if let Some(r) = wrapper {
                         let mut stroke = tiny_skia::Stroke::default();
                         stroke.width = 1.0;
                         
                         // Ensure the rect is valid before attempting to draw.
                         if r.width() <= 0.0 || r.height() <= 0.0 {
                            continue; 
                         }
                         let path = tiny_skia::PathBuilder::from_rect(r);
                         self.pixmap.stroke_path(&path, &paint, &stroke, self.transform, mask_to_use);
                         
                         if *checked {
                             let inset = 4.0;
                             if let Some(inner) = tiny_skia::Rect::from_xywh(rect.x + inset, rect.y + inset, rect.width - inset*2.0, rect.height - inset*2.0) {
                                  self.pixmap.fill_rect(inner, &paint, self.transform, mask_to_use);
                             }
                         }
                     }
                }
                DrawCommand::DrawSlider { rect, value, color } => {
                    profile!("render_slider");
                    let mut paint = tiny_skia::Paint::default();
                    paint.anti_alias = false;
                    paint.set_color(self.to_skia_color(*color));

                    // Track
                    let track_height = 6.0; // Thicker track
                    let track_y = rect.y + (rect.height - track_height) / 2.0;
                    
                    if let Some(track_rect) = tiny_skia::Rect::from_xywh(rect.x, track_y, rect.width, track_height) {
                         // Background track (darker)
                        let mut bg_paint = tiny_skia::Paint::default();
                        bg_paint.set_color(self.to_skia_color(xerune::Color::new(60, 60, 60, 255)));
                        bg_paint.anti_alias = true;
                        
                        // Rounded track
                        if let Some(path) = rounded_rect_path(track_rect, track_height / 2.0) {
                            self.pixmap.fill_path(&path, &bg_paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                        } else {
                            self.pixmap.fill_rect(track_rect, &bg_paint, self.transform, mask_to_use);
                        }
                        
                        // Active track
                        if *value > 0.0 {
                            if let Some(active_rect) = tiny_skia::Rect::from_xywh(rect.x, track_y, rect.width * value, track_height) {
                                // Clamp width to at least track_height/2 for circle cap 
                                // Or just draw rounded rect
                                if let Some(path) = rounded_rect_path(active_rect, track_height / 2.0) {
                                     self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                                } else {
                                     self.pixmap.fill_rect(active_rect, &paint, self.transform, mask_to_use);
                                }
                            }
                        }
                    }

                    // Thumb
                    let thumb_radius = 10.0;
                    let thumb_x = rect.x + rect.width * value;
                    let thumb_y = rect.y + rect.height / 2.0;
                    
                    let mut thumb_paint = tiny_skia::Paint::default();
                    thumb_paint.set_color(self.to_skia_color(xerune::Color::WHITE));
                    thumb_paint.anti_alias = true;
                    
                    // Shadow/Border for thumb to make it pop
                    let mut stroke = tiny_skia::Stroke::default();
                    stroke.width = 2.0;
                    let mut stroke_paint = tiny_skia::Paint::default();
                    stroke_paint.set_color(self.to_skia_color(xerune::Color::new(0, 0, 0, 50))); // Slight shadow contour
                    stroke_paint.anti_alias = true;

                     let path = tiny_skia::PathBuilder::from_circle(thumb_x, thumb_y, thumb_radius);
                      if let Some(p) = path {
                        self.pixmap.fill_path(&p, &thumb_paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                        self.pixmap.stroke_path(&p, &stroke_paint, &stroke, self.transform, mask_to_use);
                      }
                }
                DrawCommand::DrawProgress { rect, value, max, color } => {
                    profile!("render_progress");
                    let mut paint = tiny_skia::Paint::default();
                    paint.anti_alias = false;
                    paint.set_color(self.to_skia_color(*color));
                    
                    let track_height = rect.height;
                    let track_rect = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, track_height);

                    if let Some(track_rect) = track_rect {
                        // Background track
                        let mut bg_paint = tiny_skia::Paint::default();
                        bg_paint.set_color(self.to_skia_color(xerune::Color::new(200, 200, 200, 255)));
                        bg_paint.anti_alias = true;
                        
                        if let Some(path) = rounded_rect_path(track_rect, track_height / 2.0) {
                             self.pixmap.fill_path(&path, &bg_paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                        } else {
                             self.pixmap.fill_rect(track_rect, &bg_paint, self.transform, mask_to_use);
                        }

                        // Filled bar
                        let progress = (value / max).clamp(0.0, 1.0);
                        if progress > 0.0 {
                            if let Some(active_rect) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width * progress, track_height) {
                                if let Some(path) = rounded_rect_path(active_rect, track_height / 2.0) {
                                     self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, self.transform, mask_to_use);
                                } else {
                                     self.pixmap.fill_rect(active_rect, &paint, self.transform, mask_to_use);
                                }
                            }
                        }
                    }
                }

        
                DrawCommand::DrawCanvas { id, rect } => {
                    profile!("render_canvas");
                    if let Some(canvas) = canvases.get(id) {
                        if let Some(canvas_pixmap) = PixmapRef::from_bytes(&canvas.data, canvas.width, canvas.height) {
                            let sx = rect.width / canvas.width as f32;
                            let sy = rect.height / canvas.height as f32;
                            let transform = self.transform.pre_scale(sx, sy).pre_translate(rect.x / sx, rect.y / sy);

                            self.pixmap.draw_pixmap(
                                0, 0,
                                canvas_pixmap,
                                &PixmapPaint::default(),
                                transform,
                                mask_to_use
                            );
                        }
                    }
                }
            }
        }

        if dirty_rect.is_some() {
            self.clip_stack.pop();
            self.update_clip_mask();
        }
    }
}

fn rounded_rect_path(rect: tiny_skia::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    
    // Clamp radius to ensure it doesn't exceed half the rectangle's dimensions
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    
    if r <= 0.0 {
        return Some(tiny_skia::PathBuilder::from_rect(rect));
    }
    
    // The factor for approximating a circle quadrant with a cubic Bezier curve.
    let bezier_circle_factor = (4.0 / 3.0) * (std::f32::consts::PI / 8.0).tan();
    let handle_offset = r * bezier_circle_factor;
    
    let left = rect.x();
    let top = rect.y();
    let right = rect.x() + rect.width();
    let bottom = rect.y() + rect.height();

    // Start at the top edge, just after the top-left corner
    pb.move_to(left + r, top);
    
    // Top edge
    pb.line_to(right - r, top);
    
    // Top-right corner
    pb.cubic_to(
        right - r + handle_offset, top,            // Control point 1
        right, top + r - handle_offset,            // Control point 2
        right, top + r                             // End point
    );
    
    // Right edge
    pb.line_to(right, bottom - r);
    
    // Bottom-right corner
    pb.cubic_to(
        right, bottom - r + handle_offset,
        right - r + handle_offset, bottom,
        right - r, bottom
    );
    
    // Bottom edge
    pb.line_to(left + r, bottom);
    
    // Bottom-left corner
    pb.cubic_to(
        left + r - handle_offset, bottom,
        left, bottom - r + handle_offset,
        left, bottom - r
    );
    
    // Left edge
    pb.line_to(left, top + r);
    
    // Top-left corner
    pb.cubic_to(
        left, top + r - handle_offset,
        left + r - handle_offset, top,
        left + r, top
    );
    
    pb.close();
    pb.finish()
}
