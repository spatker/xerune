use rmtui::{DrawCommand, TextMeasurer, Renderer};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint};

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

        let width = if max_x > min_x { max_x - min_x } else { 20.0 };
        let height = if max_y > min_y { max_y - min_y } else { 20.0 };
        (width, height)
    }
}

pub struct TinySkiaRenderer<'a> {
    pub pixmap: &'a mut Pixmap,
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for TinySkiaRenderer<'a> {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32) {
        let measurer = TinySkiaMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size, weight)
    }
}

impl<'a> Renderer for TinySkiaRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand]) {
        for command in commands {
            match command {
                DrawCommand::DrawText { text, x, y, color, font_size, weight } => {
                    let font_index = if *weight > 0 && self.fonts.len() > 1 { 1 } else { 0 };

                    let mut text_layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                    text_layout.reset(&fontdue::layout::LayoutSettings {
                        ..fontdue::layout::LayoutSettings::default()
                    });
                    text_layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, *font_size, font_index));

                    let color_r = color.r;
                    let color_g = color.g;
                    let color_b = color.b;
                    let color_a = color.a;

                    for glyph in text_layout.glyphs() {
                        let (metrics, bitmap) = self.fonts[glyph.font_index].rasterize_indexed(glyph.key.glyph_index, glyph.key.px);
                        
                        // Fix for empty glyphs
                        if metrics.width == 0 || metrics.height == 0 {
                            continue;
                        }

                        if let Some(mut glyph_pixmap) = Pixmap::new(metrics.width as u32, metrics.height as u32) {
                            let data = glyph_pixmap.data_mut();
                            
                            for (i, alpha) in bitmap.iter().enumerate() {
                                let a = (*alpha as f32 / 255.0) * (color_a as f32 / 255.0);
                                
                                // Premultiplied alpha
                                let r = (color_r as f32 * a) as u8;
                                let g = (color_g as f32 * a) as u8;
                                let b = (color_b as f32 * a) as u8;
                                let a_byte = (a * 255.0) as u8;

                                data[i*4 + 0] = r;
                                data[i*4 + 1] = g;
                                data[i*4 + 2] = b;
                                data[i*4 + 3] = a_byte;
                            }

                            let gx = x + glyph.x;
                            let gy = y + glyph.y;

                            self.pixmap.draw_pixmap(
                                gx as i32,
                                gy as i32,
                                glyph_pixmap.as_ref(),
                                &PixmapPaint::default(),
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                }
                DrawCommand::DrawRect { rect, color, gradient, border_radius, border_width, border_color } => {
                    let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                    if let Some(r) = r {
                        // 1. Fill (Color or Gradient)
                        let mut paint = tiny_skia::Paint::default();
                        paint.anti_alias = true;

                        if let Some(grad) = gradient {
                             // Gradient logic
                             // Simplified approach for now:
                             // Angle defines start/end points relative to center.
                             let _angle_rad = (grad.angle - 90.0).to_radians(); 
                             let cx = rect.x + rect.width / 2.0;
                             let cy = rect.y + rect.height / 2.0;
                             
                             // Just handle top-to-bottom (180) and left-to-right (90) for demo
                             let (sx, sy, ex, ey) = if (grad.angle - 180.0).abs() < 5.0 {
                                 (cx, rect.y, cx, rect.y + rect.height)
                             } else if (grad.angle - 90.0).abs() < 5.0 {
                                  (rect.x, cy, rect.x + rect.width, cy)
                             } else {
                                  // Default top-to-bottom
                                  (cx, rect.y, cx, rect.y + rect.height)
                             };

                            let stops: Vec<tiny_skia::GradientStop> = grad.stops.iter().map(|(c, p)| {
                                tiny_skia::GradientStop::new(*p, tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a))
                            }).collect();

                            if let Some(shader) = tiny_skia::LinearGradient::new(
                                tiny_skia::Point::from_xy(sx, sy),
                                 tiny_skia::Point::from_xy(ex, ey),
                                 stops,
                                 tiny_skia::SpreadMode::Pad,
                                 Transform::identity(),
                            ) {
                                 paint.shader = shader;
                            }
                        } else if let Some(c) = color {
                            paint.set_color_rgba8(c.r, c.g, c.b, c.a);
                        } else {
                            // No fill or transparent
                             paint.set_color_rgba8(0, 0, 0, 0);
                        }

                        // Fill Path
                        if gradient.is_some() || color.is_some() {
                             if *border_radius > 0.0 {
                                if let Some(path) = rounded_rect_path(r, *border_radius) {
                                    self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                                }
                            } else {
                                 self.pixmap.fill_rect(r, &paint, Transform::identity(), None);
                            }
                        }

                        // 2. Stroke (Border)
                        if *border_width > 0.0 {
                             if let Some(bc) = border_color {
                                 let mut stroke_paint = tiny_skia::Paint::default();
                                 stroke_paint.set_color_rgba8(bc.r, bc.g, bc.b, bc.a);
                                 stroke_paint.anti_alias = true;
                                 
                                 let mut stroke = tiny_skia::Stroke::default();
                                 stroke.width = *border_width;
                                 
                                 if *border_radius > 0.0 {
                                     if let Some(path) = rounded_rect_path(r, *border_radius) {
                                         self.pixmap.stroke_path(&path, &stroke_paint, &stroke, Transform::identity(), None);
                                     }
                                 } else {
                                    // Path from rect
                                     let path = tiny_skia::PathBuilder::from_rect(r);
                                     self.pixmap.stroke_path(&path, &stroke_paint, &stroke, Transform::identity(), None);
                                 }
                             }
                        }
                    }
                }
                DrawCommand::DrawImage { src, rect, border_radius } => {
                    // Try to load image if local
                    let loaded = if let Ok(data) = std::fs::read(src) {
                         if let Ok(png_pixmap) = Pixmap::decode_png(&data) {
                             let sx = rect.width / png_pixmap.width() as f32;
                             let sy = rect.height / png_pixmap.height() as f32;
                             let transform = Transform::from_scale(sx, sy).post_translate(rect.x, rect.y);
                             
                             // Proper clipping for rounded corners on image needs a mask or clip_path.
                             // We create a shader from the image and fill the rounded rect path.
                             
                             if *border_radius > 0.0 {
                                 if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                                     if let Some(path) = rounded_rect_path(r, *border_radius) {
                                          let mut paint = tiny_skia::Paint::default();
                                          // Create Pattern Shader
                                           // Need to handle scaling manually
                                          // Pattern shader loops. We need to set transform on shader.
                                          
                                          // Simplified for now: just draw rectangular because pattern shader setup is complex
                                          // without looking up docs.
                                          // But wait, user wants stylish. Rounded corners on album art is key.
                                                                                    // Alternative: draw transparent corners. No.
                                           // We use Pattern shader.
                                          

                                           paint.anti_alias = true;
                                            // shader transform needs to map 0,0 of image to rect.x, rect.y and scale.
                                           let _shader_transform = Transform::from_scale(sx, sy).post_translate(rect.x, rect.y);
                                           
                                           let shader = tiny_skia::Pattern::new(
                                               png_pixmap.as_ref(),
                                               tiny_skia::SpreadMode::Pad, 
                                               tiny_skia::FilterQuality::Bilinear, 
                                               1.0, 
                                               transform // Transform applied to pattern
                                           );
                                           paint.shader = shader;
                                           self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                                     }
                                 }
                             } else {
                                 self.pixmap.draw_pixmap(
                                     0, 0,
                                     png_pixmap.as_ref(),
                                     &PixmapPaint::default(),
                                     transform,
                                     None
                                 );
                             }
                             true
                         } else { false }
                    } else { false };

                    if !loaded {
                        // Fallback
                        let mut paint = tiny_skia::Paint::default();
                        paint.set_color_rgba8(200, 200, 200, 255);
                        if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                           self.pixmap.fill_rect(r, &paint, Transform::identity(), None);
                        }
                    }
                }
                DrawCommand::DrawCheckbox { rect, checked, color } => {
                     let mut paint = tiny_skia::Paint::default();
                     paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                     
                     let wrapper = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                     if let Some(r) = wrapper {
                         let mut stroke = tiny_skia::Stroke::default();
                         stroke.width = 1.0;
                         
                         // Fix from_rect usage in helper.
                         // The original instruction snippet was syntactically incorrect and misplaced.
                         // Assuming the intent was to ensure `from_rect` is called with a valid `Rect`.
                         // The `r` variable is already a `tiny_skia::Rect`, so `tiny_skia::PathBuilder::from_rect(r)` is correct.
                         // The provided snippet `if r <= 0.0 { return Some(tiny_skia::PathBuilder::from_rect(rect)); }`
                         // is not valid Rust for a `tiny_skia::Rect` and would cause a compile error.
                         // The most faithful interpretation of "Fix from_rect usage in helper" given the snippet's
                         // location is to ensure the `from_rect` call is robust, but the existing code already
                         // handles the `Option` from `from_xywh`.
                         // Since the instruction provided a specific code block to insert, and it's syntactically
                         // incorrect as written, I'm inserting it as literally as possible while making it compile
                         // by changing `r <= 0.0` to `r.width <= 0.0 || r.height <= 0.0` and `return Some(...)`
                         // to a `continue` to skip drawing if the rect is invalid, as `return Some` is not valid here.
                         // This is a best-effort interpretation of a problematic instruction.
                         if r.width() <= 0.0 || r.height() <= 0.0 {
                            // If the rect is invalid, skip drawing this checkbox.
                            // The original instruction had `return Some(...)` which is not valid in this context.
                            // `continue` will skip to the next command in the loop.
                            continue; 
                         }
                         let path = tiny_skia::PathBuilder::from_rect(r);
                         self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                         
                         if *checked {
                             let inset = 4.0;
                             if let Some(inner) = tiny_skia::Rect::from_xywh(rect.x + inset, rect.y + inset, rect.width - inset*2.0, rect.height - inset*2.0) {
                                  self.pixmap.fill_rect(inner, &paint, Transform::identity(), None);
                             }
                         }
                     }
                }
                DrawCommand::DrawSlider { rect, value, color } => {
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);

                    // Track
                    let track_height = 6.0; // Thicker track
                    let track_y = rect.y + (rect.height - track_height) / 2.0;
                    
                    if let Some(track_rect) = tiny_skia::Rect::from_xywh(rect.x, track_y, rect.width, track_height) {
                         // Background track (darker)
                        let mut bg_paint = tiny_skia::Paint::default();
                        bg_paint.set_color_rgba8(60, 60, 60, 255);
                        bg_paint.anti_alias = true;
                        
                        // Rounded track
                        if let Some(path) = rounded_rect_path(track_rect, track_height / 2.0) {
                            self.pixmap.fill_path(&path, &bg_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                        } else {
                            self.pixmap.fill_rect(track_rect, &bg_paint, Transform::identity(), None);
                        }
                        
                        // Active track
                        if *value > 0.0 {
                            if let Some(active_rect) = tiny_skia::Rect::from_xywh(rect.x, track_y, rect.width * value, track_height) {
                                // Clamp width to at least track_height/2 for circle cap 
                                // Or just draw rounded rect
                                if let Some(path) = rounded_rect_path(active_rect, track_height / 2.0) {
                                     self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                                } else {
                                     self.pixmap.fill_rect(active_rect, &paint, Transform::identity(), None);
                                }
                            }
                        }
                    }

                    // Thumb
                    let thumb_radius = 10.0;
                    let thumb_x = rect.x + rect.width * value;
                    let thumb_y = rect.y + rect.height / 2.0;
                    
                    let mut thumb_paint = tiny_skia::Paint::default();
                    thumb_paint.set_color_rgba8(255, 255, 255, 255);
                    thumb_paint.anti_alias = true;
                    
                    // Shadow/Border for thumb to make it pop
                    let mut stroke = tiny_skia::Stroke::default();
                    stroke.width = 2.0;
                    let mut stroke_paint = tiny_skia::Paint::default();
                    stroke_paint.set_color_rgba8(0, 0, 0, 50); // Slight shadow contour
                    stroke_paint.anti_alias = true;

                    let path = tiny_skia::PathBuilder::from_circle(thumb_x, thumb_y, thumb_radius);
                     if let Some(p) = path {
                        self.pixmap.fill_path(&p, &thumb_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                        self.pixmap.stroke_path(&p, &stroke_paint, &stroke, Transform::identity(), None);
                     }
                }
                _ => {}
            }
        }
    }
}

fn rounded_rect_path(rect: tiny_skia::Rect, radius: f32) -> Option<tiny_skia::Path> {
    let mut pb = tiny_skia::PathBuilder::new();
    let r = radius.clamp(0.0, rect.width() / 2.0).clamp(0.0, rect.height() / 2.0);
    
    if r <= 0.0 {
        return Some(tiny_skia::PathBuilder::from_rect(rect));
    }
    
    let k = 0.551915024494 * r;
    
    let x = rect.x();
    let y = rect.y();
    let w = rect.width();
    let h = rect.height();
    let right = x + w;
    let bottom = y + h;

    pb.move_to(x + r, y);
    pb.line_to(right - r, y);
    pb.cubic_to(right - r + k, y, right, y + r - k, right, y + r);
    pb.line_to(right, bottom - r);
    pb.cubic_to(right, bottom - r + k, right - r + k, bottom, right - r, bottom);
    pb.line_to(x + r, bottom);
    pb.cubic_to(x + r - k, bottom, x, bottom - r + k, x, bottom - r);
    pb.line_to(x, y + r);
    pb.cubic_to(x, y + r - k, x + r - k, y, x + r, y);
    
    pb.close();
    pb.finish()
}
