#[inline(always)]
pub fn div_255(x: u32) -> u32 {
    (x + 1 + (x >> 8)) >> 8
}

#[inline(always)]
pub fn pack_color(color: xerune::Color, swap_rb: bool) -> u32 {
    let r = color.r as u32;
    let g = color.g as u32;
    let b = color.b as u32;
    let a = color.a as u32;
    if swap_rb {
        (a << 24) | (b << 16) | (g << 8) | r
    } else {
        (a << 24) | (r << 16) | (g << 8) | b
    }
}

#[inline(always)]
pub fn blend_pixel(dst: &mut u32, src: u32) {
    let src_a = (src >> 24) & 0xff;
    if src_a == 255 {
        *dst = src;
    } else if src_a > 0 {
        let d = *dst;
        let dst_a = (d >> 24) & 0xff;
        
        let src_r = (src >> 16) & 0xff;
        let src_g = (src >> 8) & 0xff;
        let src_b = src & 0xff;
        
        let dst_r = (d >> 16) & 0xff;
        let dst_g = (d >> 8) & 0xff;
        let dst_b = d & 0xff;
        
        let inv_a = 255 - src_a;
        
        let r = div_255(src_r * src_a + dst_r * inv_a);
        let g = div_255(src_g * src_a + dst_g * inv_a);
        let b = div_255(src_b * src_a + dst_b * inv_a);
        let a = src_a + div_255(dst_a * inv_a);
        
        *dst = (a << 24) | (r << 16) | (g << 8) | b;
    }
}

pub fn blend_solid_span(dst: &mut [u32], color: u32) {
    let a = (color >> 24) & 0xff;
    if a == 255 {
        dst.fill(color);
    } else if a > 0 {
        let r = (color >> 16) & 0xff;
        let g = (color >> 8) & 0xff;
        let b = color & 0xff;
        let inv_a = 255 - a;
        
        for pixel in dst.iter_mut() {
            let d = *pixel;
            let dst_a = (d >> 24) & 0xff;
            let dst_r = (d >> 16) & 0xff;
            let dst_g = (d >> 8) & 0xff;
            let dst_b = d & 0xff;
            
            let res_r = div_255(r * a + dst_r * inv_a);
            let res_g = div_255(g * a + dst_g * inv_a);
            let res_b = div_255(b * a + dst_b * inv_a);
            let res_a = a + div_255(dst_a * inv_a);
            
            *pixel = (res_a << 24) | (res_r << 16) | (res_g << 8) | res_b;
        }
    }
}

pub fn blend_solid_rect(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    color: u32,
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

    if start_x >= end_x || start_y >= end_y {
        return;
    }

    if rotate {
        for y in start_y..end_y {
            for x in start_x..end_x {
                let idx = (x as usize * physical_w as usize) + (physical_w as usize - 1 - y as usize);
                if idx < buffer.len() {
                    blend_pixel(&mut buffer[idx], color);
                }
            }
        }
    } else {
        let span_w = (end_x - start_x) as usize;
        for y in start_y..end_y {
            let start_idx = (y * physical_w as i32 + start_x) as usize;
            let dst_span = &mut buffer[start_idx..start_idx + span_w];
            blend_solid_span(dst_span, color);
        }
    }
}

pub fn blend_glyph_span(dst: &mut [u32], coverage: &[u8], color: u32) {
    let color_a = (color >> 24) & 0xff;
    if color_a == 0 {
        return;
    }
    let r = (color >> 16) & 0xff;
    let g = (color >> 8) & 0xff;
    let b = color & 0xff;

    for (pixel, &cov) in dst.iter_mut().zip(coverage.iter()) {
        if cov > 0 {
            let a = div_255(color_a * cov as u32);
            if a == 255 {
                *pixel = color;
            } else if a > 0 {
                let d = *pixel;
                let dst_a = (d >> 24) & 0xff;
                let dst_r = (d >> 16) & 0xff;
                let dst_g = (d >> 8) & 0xff;
                let dst_b = d & 0xff;
                let inv_a = 255 - a;
                
                let res_r = div_255(r * a + dst_r * inv_a);
                let res_g = div_255(g * a + dst_g * inv_a);
                let res_b = div_255(b * a + dst_b * inv_a);
                let res_a = a + div_255(dst_a * inv_a);
                
                *pixel = (res_a << 24) | (res_r << 16) | (res_g << 8) | res_b;
            }
        }
    }
}
