use rmtui::{DrawCommand, TextMeasurer, Renderer};
use fontdue::Font;
use tiny_skia::{Pixmap, Transform, PixmapPaint};

pub struct TinySkiaMeasurer<'a> {
    pub fonts: &'a [Font],
}

impl<'a> TextMeasurer for TinySkiaMeasurer<'a> {
    fn measure_text(&self, text: &str, font_size: f32) -> (f32, f32) {
        if text.trim().is_empty() {
            return (0.0, 0.0);
        }

        let mut layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
        layout.reset(&fontdue::layout::LayoutSettings {
            ..fontdue::layout::LayoutSettings::default()
        });
        layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, font_size, 0));

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
    fn measure_text(&self, text: &str, font_size: f32) -> (f32, f32) {
        let measurer = TinySkiaMeasurer { fonts: self.fonts };
        measurer.measure_text(text, font_size)
    }
}

impl<'a> Renderer for TinySkiaRenderer<'a> {
    fn render(&mut self, commands: &[DrawCommand]) {
        for command in commands {
            match command {
                DrawCommand::DrawText { text, x, y, color, font_size } => {
                    let mut text_layout = fontdue::layout::Layout::new(fontdue::layout::CoordinateSystem::PositiveYDown);
                    text_layout.reset(&fontdue::layout::LayoutSettings {
                        ..fontdue::layout::LayoutSettings::default()
                    });
                    text_layout.append(self.fonts, &fontdue::layout::TextStyle::new(text, *font_size, 0));

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
                DrawCommand::DrawRect { rect, color } => {
                    let mut paint = tiny_skia::Paint::default();
                    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
                    
                     let r = tiny_skia::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                     if let Some(r) = r {
                         self.pixmap.fill_rect(r, &paint, Transform::identity(), None);
                     }
                }
                DrawCommand::DrawImage { src, rect } => {
                    // Try to load image if local
                    let loaded = if let Ok(data) = std::fs::read(src) {
                         if let Ok(png_pixmap) = Pixmap::decode_png(&data) {
                             let sx = rect.width / png_pixmap.width() as f32;
                             let sy = rect.height / png_pixmap.height() as f32;
                             let transform = Transform::from_scale(sx, sy).post_translate(rect.x, rect.y);
                             
                             self.pixmap.draw_pixmap(
                                 0, 0,
                                 png_pixmap.as_ref(),
                                 &PixmapPaint::default(),
                                 transform,
                                 None
                             );
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
                _ => {}
            }
        }
    }
}
