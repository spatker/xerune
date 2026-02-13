use xerune::{DrawCommand, TextMeasurer, Renderer};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint, Mask, PathBuilder, FillRule};

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
    pub clip_stack: Vec<tiny_skia::Rect>,
    pub current_mask: Option<Mask>,
}

impl<'a> TinySkiaRenderer<'a> {
    pub fn new(pixmap: &'a mut Pixmap, fonts: &'a [Font]) -> Self {
        Self {
            pixmap,
            fonts,
            clip_stack: Vec::new(),
            current_mask: None,
        }
    }

    fn update_clip_mask(&mut self) {
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
                // We create a new mask which is initialized to fully transparent (0), effectively hiding everything.
                if let Some(mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) {
                     self.current_mask = Some(mask);
                }
                return;
            }
        }

        // Create mask
        if let Some(mut mask) = Mask::new(self.pixmap.width(), self.pixmap.height()) {
             let path = PathBuilder::from_rect(intersect);
             mask.fill_path(&path, FillRule::Winding, true, Transform::identity()); // true = anti-alias
             self.current_mask = Some(mask);
        }
    }
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
                DrawCommand::Clip { rect } => {
                    if let Some(r) = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height) {
                        self.clip_stack.push(r);
                        self.update_clip_mask();
                    }
                }
                DrawCommand::PopClip => {
                    self.clip_stack.pop();
                    self.update_clip_mask();
                }
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
                                    self.current_mask.as_ref(),
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
                                    self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), self.current_mask.as_ref());
                                }
                            } else {
                                 self.pixmap.fill_rect(r, &paint, Transform::identity(), self.current_mask.as_ref());
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
                                         self.pixmap.stroke_path(&path, &stroke_paint, &stroke, Transform::identity(), self.current_mask.as_ref());
                                     }
                                 } else {
                                    // Path from rect
                                     let path = tiny_skia::PathBuilder::from_rect(r);
                                     self.pixmap.stroke_path(&path, &stroke_paint, &stroke, Transform::identity(), self.current_mask.as_ref());
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
                                          paint.anti_alias = true;
                                          
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
                                           self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), self.current_mask.as_ref());
                                     }
                                 }
                             } else {
                                 self.pixmap.draw_pixmap(
                                     0, 0,
                                     png_pixmap.as_ref(),
                                     &PixmapPaint::default(),
                                     transform,
                                     self.current_mask.as_ref()
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
                           self.pixmap.fill_rect(r, &paint, Transform::identity(), self.current_mask.as_ref());
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
                         
                         // Ensure the rect is valid before attempting to draw.
                         if r.width() <= 0.0 || r.height() <= 0.0 {
                            continue; 
                         }
                         let path = tiny_skia::PathBuilder::from_rect(r);
                         self.pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
                         
                         if *checked {
                             let inset = 4.0;
                             if let Some(inner) = tiny_skia::Rect::from_xywh(rect.x + inset, rect.y + inset, rect.width - inset*2.0, rect.height - inset*2.0) {
                                  self.pixmap.fill_rect(inner, &paint, Transform::identity(), self.current_mask.as_ref());
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
                            self.pixmap.fill_path(&path, &bg_paint, tiny_skia::FillRule::Winding, Transform::identity(), self.current_mask.as_ref());
                        } else {
                            self.pixmap.fill_rect(track_rect, &bg_paint, Transform::identity(), self.current_mask.as_ref());
                        }
                        
                        // Active track
                        if *value > 0.0 {
                            if let Some(active_rect) = tiny_skia::Rect::from_xywh(rect.x, track_y, rect.width * value, track_height) {
                                // Clamp width to at least track_height/2 for circle cap 
                                // Or just draw rounded rect
                                if let Some(path) = rounded_rect_path(active_rect, track_height / 2.0) {
                                     self.pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), self.current_mask.as_ref());
                                } else {
                                     self.pixmap.fill_rect(active_rect, &paint, Transform::identity(), self.current_mask.as_ref());
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
                        self.pixmap.fill_path(&p, &thumb_paint, tiny_skia::FillRule::Winding, Transform::identity(), self.current_mask.as_ref());
                        self.pixmap.stroke_path(&p, &stroke_paint, &stroke, Transform::identity(), self.current_mask.as_ref());
                      }
                }

            }
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
