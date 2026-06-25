use crate::blitter::{pack_color, blend_solid_span, blend_pixel};

pub fn sample_gradient(stops: &[(xerune::Color, f32)], t: f32) -> xerune::Color {
    if stops.is_empty() {
        return xerune::Color::BLACK;
    }
    if stops.len() == 1 {
        return stops[0].0;
    }
    let t = t.clamp(0.0, 1.0);
    
    let mut left = &stops[0];
    let mut right = &stops[stops.len() - 1];
    
    let mut found_left = false;
    let mut found_right = false;

    for stop in stops {
        if stop.1 <= t {
            if !found_left || stop.1 > left.1 {
                left = stop;
                found_left = true;
            }
        }
        if stop.1 >= t {
            if !found_right || stop.1 < right.1 {
                right = stop;
                found_right = true;
            }
        }
    }
    
    if !found_left {
        left = &stops[0];
    }
    if !found_right {
        right = &stops[stops.len() - 1];
    }

    if (right.1 - left.1).abs() < 1e-5 {
        return left.0;
    }
    
    let factor = (t - left.1) / (right.1 - left.1);
    let factor = factor.clamp(0.0, 1.0);
    
    let r = ((1.0 - factor) * left.0.r as f32 + factor * right.0.r as f32).round() as u8;
    let g = ((1.0 - factor) * left.0.g as f32 + factor * right.0.g as f32).round() as u8;
    let b = ((1.0 - factor) * left.0.b as f32 + factor * right.0.b as f32).round() as u8;
    let a = ((1.0 - factor) * left.0.a as f32 + factor * right.0.a as f32).round() as u8;
    
    xerune::Color::new(r, g, b, a)
}

pub fn draw_gradient_rect(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    gradient: &xerune::LinearGradient,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotate: bool,
) {
    let (clip_x1, clip_y1, clip_x2, clip_y2) = if let Some(cr) = clip_rect {
        (
            cr.x.max(0.0) as i32,
            cr.y.max(0.0) as i32,
            (cr.x + cr.width).min(logical_w as f32) as i32,
            (cr.y + cr.height).min(logical_h as f32) as i32,
        )
    } else {
        (0, 0, logical_w as i32, logical_h as i32)
    };

    let start_x = rect_x.max(clip_x1);
    let start_y = rect_y.max(clip_y1);
    let end_x = (rect_x + rect_w).min(clip_x2);
    let end_y = (rect_y + rect_h).min(clip_y2);

    if start_x >= end_x || start_y >= end_y || rect_w <= 0 || rect_h <= 0 {
        return;
    }

    let angle_normalized = (gradient.angle % 360.0 + 360.0) % 360.0;
    let is_horizontal = (angle_normalized - 90.0).abs() < 45.0 || (angle_normalized - 270.0).abs() < 45.0;

    if rotate {
        // Rotated pixel-by-pixel gradient fill
        let is_reverse = if is_horizontal {
            (angle_normalized - 270.0).abs() < 45.0
        } else {
            (angle_normalized - 0.0).abs() < 45.0 || (angle_normalized - 360.0).abs() < 45.0
        };

        for y in start_y..end_y {
            for x in start_x..end_x {
                let t = if is_horizontal {
                    if is_reverse {
                        1.0 - ((x - rect_x) as f32 / rect_w as f32)
                    } else {
                        (x - rect_x) as f32 / rect_w as f32
                    }
                } else {
                    if is_reverse {
                        1.0 - ((y - rect_y) as f32 / rect_h as f32)
                    } else {
                        (y - rect_y) as f32 / rect_h as f32
                    }
                };

                let col = sample_gradient(&gradient.stops, t);
                let color = pack_color(col, swap_rb);
                let idx = (x as usize * physical_w as usize) + (physical_w as usize - 1 - y as usize);
                if idx < buffer.len() {
                    blend_pixel(&mut buffer[idx], color);
                }
            }
        }
    } else {
        // Standard horizontal or vertical contiguous gradients
        if is_horizontal {
            let mut span_colors = Vec::with_capacity(rect_w as usize);
            let is_reverse = (angle_normalized - 270.0).abs() < 45.0;
            for dx in 0..rect_w {
                let t = if is_reverse {
                    1.0 - (dx as f32 / rect_w as f32)
                } else {
                    dx as f32 / rect_w as f32
                };
                let col = sample_gradient(&gradient.stops, t);
                span_colors.push(pack_color(col, swap_rb));
            }

            let draw_w = (end_x - start_x) as usize;
            let x_offset = (start_x - rect_x) as usize;
            for y in start_y..end_y {
                let start_idx = (y * physical_w as i32 + start_x) as usize;
                for x_idx in 0..draw_w {
                    let src_color = span_colors[x_offset + x_idx];
                    blend_pixel(&mut buffer[start_idx + x_idx], src_color);
                }
            }
        } else {
            let is_reverse = (angle_normalized - 0.0).abs() < 45.0 || (angle_normalized - 360.0).abs() < 45.0;
            let draw_w = (end_x - start_x) as usize;
            for y in start_y..end_y {
                let t = if is_reverse {
                    1.0 - ((y - rect_y) as f32 / rect_h as f32)
                } else {
                    (y - rect_y) as f32 / rect_h as f32
                };
                let col = sample_gradient(&gradient.stops, t);
                let color = pack_color(col, swap_rb);
                let start_idx = (y * physical_w as i32 + start_x) as usize;
                let dst_span = &mut buffer[start_idx..start_idx + draw_w];
                blend_solid_span(dst_span, color);
            }
        }
    }
}
