use crate::blitter::{pack_color, blend_solid_rect, blend_pixel};
use crate::gradient::{draw_gradient_rect, sample_gradient};

pub fn draw_rounded_rect(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    radius: f32,
    color: Option<xerune::Color>,
    gradient: Option<&xerune::LinearGradient>,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotate: bool,
) {
    if rect_w <= 0 || rect_h <= 0 {
        return;
    }

    if radius <= 0.0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, grad, swap_rb, clip_rect, rotate);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, packed, clip_rect, rotate);
        }
        return;
    }

    let r_f32 = radius.min(rect_w as f32 / 2.0).min(rect_h as f32 / 2.0).max(0.0);
    let r_i32 = r_f32.ceil() as i32;

    // Center strip
    let center_y = rect_y + r_i32;
    let center_h = rect_h - 2 * r_i32;
    if center_h > 0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, rect_x, center_y, rect_w, center_h, grad, swap_rb, clip_rect, rotate);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, center_y, rect_w, center_h, packed, clip_rect, rotate);
        }
    }

    // Top strip
    let top_x = rect_x + r_i32;
    let top_w = rect_w - 2 * r_i32;
    if top_w > 0 && r_i32 > 0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, top_x, rect_y, top_w, r_i32, grad, swap_rb, clip_rect, rotate);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, rect_y, top_w, r_i32, packed, clip_rect, rotate);
        }
    }

    // Bottom strip
    let bottom_y = rect_y + rect_h - r_i32;
    if top_w > 0 && r_i32 > 0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, top_x, bottom_y, top_w, r_i32, grad, swap_rb, clip_rect, rotate);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, bottom_y, top_w, r_i32, packed, clip_rect, rotate);
        }
    }

    // Corners
    let corners = [
        // Top-Left
        (
            rect_x,
            rect_y,
            rect_x + r_i32,
            rect_y + r_i32,
            rect_x as f32 + r_f32,
            rect_y as f32 + r_f32,
        ),
        // Top-Right
        (
            rect_x + rect_w - r_i32,
            rect_y,
            rect_x + rect_w,
            rect_y + r_i32,
            rect_x as f32 + rect_w as f32 - r_f32,
            rect_y as f32 + r_f32,
        ),
        // Bottom-Left
        (
            rect_x,
            rect_y + rect_h - r_i32,
            rect_x + r_i32,
            rect_y + rect_h,
            rect_x as f32 + r_f32,
            rect_y as f32 + rect_h as f32 - r_f32,
        ),
        // Bottom-Right
        (
            rect_x + rect_w - r_i32,
            rect_y + rect_h - r_i32,
            rect_x + rect_w,
            rect_y + rect_h,
            rect_x as f32 + rect_w as f32 - r_f32,
            rect_y as f32 + rect_h as f32 - r_f32,
        ),
    ];

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

    let r_min = r_f32 - 0.5;
    let r_max = r_f32 + 0.5;
    let r_min_sq = if r_min > 0.0 { r_min * r_min } else { 0.0 };
    let r_max_sq = r_max * r_max;

    let use_table = r_i32 <= 64;
    let mut corner_cov = [0.0f32; 64 * 64];
    if use_table {
        for dy_idx in 0..r_i32 {
            for dx_idx in 0..r_i32 {
                let dx = dx_idx as f32 + 0.5 - r_f32;
                let dy = dy_idx as f32 + 0.5 - r_f32;
                let d2 = dx * dx + dy * dy;
                let coverage = if d2 >= r_max_sq {
                    0.0
                } else if d2 <= r_min_sq {
                    1.0
                } else {
                    (r_f32 + 0.5 - d2.sqrt()).clamp(0.0, 1.0)
                };
                corner_cov[(dy_idx * 64 + dx_idx) as usize] = coverage;
            }
        }
    }

    for (corner_idx, &(x1, y1, x2, y2, cx, cy)) in corners.iter().enumerate() {
        let start_x = x1.max(clip_x1);
        let start_y = y1.max(clip_y1);
        let end_x = x2.min(clip_x2);
        let end_y = y2.min(clip_y2);

        for py in start_y..end_y {
            for px in start_x..end_x {
                let coverage = if use_table {
                    let dx_idx = match corner_idx {
                        0 | 2 => px - rect_x,
                        1 | 3 => rect_x + rect_w - 1 - px,
                        _ => 0,
                    };
                    let dy_idx = match corner_idx {
                        0 | 1 => py - rect_y,
                        2 | 3 => rect_y + rect_h - 1 - py,
                        _ => 0,
                    };
                    if dx_idx >= 0 && dx_idx < 64 && dy_idx >= 0 && dy_idx < 64 {
                        corner_cov[(dy_idx * 64 + dx_idx) as usize]
                    } else {
                        0.0
                    }
                } else {
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    let d2 = dx * dx + dy * dy;
                    if d2 >= r_max_sq {
                        0.0
                    } else if d2 <= r_min_sq {
                        1.0
                    } else {
                        (r_f32 + 0.5 - d2.sqrt()).clamp(0.0, 1.0)
                    }
                };
                
                if coverage > 0.0 {
                    let src_color = if let Some(grad) = gradient {
                        let t = if (grad.angle % 360.0 - 90.0).abs() < 45.0 || (grad.angle % 360.0 - 270.0).abs() < 45.0 {
                            (px - rect_x) as f32 / rect_w as f32
                        } else {
                            (py - rect_y) as f32 / rect_h as f32
                        };
                        let col = sample_gradient(&grad.stops, t);
                        pack_color(col, swap_rb)
                    } else if let Some(col) = color {
                        pack_color(col, swap_rb)
                    } else {
                        0
                    };

                    let alpha = ((src_color >> 24) & 0xff) as f32 * coverage;
                    let packed_col = (src_color & 0x00ffffff) | ((alpha.round() as u32) << 24);

                    let idx = if rotate {
                        (px as usize * physical_w as usize) + (physical_w as usize - 1 - py as usize)
                    } else {
                        (py as usize * physical_w as usize) + px as usize
                    };

                    if idx < buffer.len() {
                        blend_pixel(&mut buffer[idx], packed_col);
                    }
                }
            }
        }
    }
}

pub fn draw_rounded_border(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    radius: f32,
    border_width: f32,
    border_color: xerune::Color,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotate: bool,
) {
    if rect_w <= 0 || rect_h <= 0 || border_width <= 0.0 {
        return;
    }

    let packed_border = pack_color(border_color, swap_rb);

    if radius <= 0.0 {
        let bw = border_width.round() as i32;
        // Top
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, bw, packed_border, clip_rect, rotate);
        // Bottom
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + rect_h - bw, rect_w, bw, packed_border, clip_rect, rotate);
        // Left
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + bw, bw, rect_h - 2 * bw, packed_border, clip_rect, rotate);
        // Right
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + rect_w - bw, rect_y + bw, bw, rect_h - 2 * bw, packed_border, clip_rect, rotate);
        return;
    }

    let r_f32 = radius.min(rect_w as f32 / 2.0).min(rect_h as f32 / 2.0).max(0.0);
    let r_i32 = r_f32.ceil() as i32;
    let bw_f32 = border_width;
    let bw_i32 = bw_f32.round() as i32;

    // Straight segments
    let top_w = rect_w - 2 * r_i32;
    if top_w > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + r_i32, rect_y, top_w, bw_i32, packed_border, clip_rect, rotate);
    }
    if top_w > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + r_i32, rect_y + rect_h - bw_i32, top_w, bw_i32, packed_border, clip_rect, rotate);
    }
    let side_h = rect_h - 2 * r_i32;
    if side_h > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + r_i32, bw_i32, side_h, packed_border, clip_rect, rotate);
    }
    if side_h > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + rect_w - bw_i32, rect_y + r_i32, bw_i32, side_h, packed_border, clip_rect, rotate);
    }

    // Corner arcs
    let corners = [
        // Top-Left
        (
            rect_x,
            rect_y,
            rect_x + r_i32,
            rect_y + r_i32,
            rect_x as f32 + r_f32,
            rect_y as f32 + r_f32,
        ),
        // Top-Right
        (
            rect_x + rect_w - r_i32,
            rect_y,
            rect_x + rect_w,
            rect_y + r_i32,
            rect_x as f32 + rect_w as f32 - r_f32,
            rect_y as f32 + r_f32,
        ),
        // Bottom-Left
        (
            rect_x,
            rect_y + rect_h - r_i32,
            rect_x + r_i32,
            rect_y + rect_h,
            rect_x as f32 + r_f32,
            rect_y as f32 + rect_h as f32 - r_f32,
        ),
        // Bottom-Right
        (
            rect_x + rect_w - r_i32,
            rect_y + rect_h - r_i32,
            rect_x + rect_w,
            rect_y + rect_h,
            rect_x as f32 + rect_w as f32 - r_f32,
            rect_y as f32 + rect_h as f32 - r_f32,
        ),
    ];

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

    let r_min = r_f32 - 0.5;
    let r_max = r_f32 + 0.5;
    let r_min_sq = if r_min > 0.0 { r_min * r_min } else { 0.0 };
    let r_max_sq = r_max * r_max;

    let r_in = (r_f32 - bw_f32).max(0.0);
    let r_in_min = r_in - 0.5;
    let r_in_max = r_in + 0.5;
    let r_in_min_sq = if r_in_min > 0.0 { r_in_min * r_in_min } else { 0.0 };
    let r_in_max_sq = r_in_max * r_in_max;

    let use_table = r_i32 <= 64;
    let mut corner_cov = [0.0f32; 64 * 64];
    if use_table {
        for dy_idx in 0..r_i32 {
            for dx_idx in 0..r_i32 {
                let dx = dx_idx as f32 + 0.5 - r_f32;
                let dy = dy_idx as f32 + 0.5 - r_f32;
                let d2 = dx * dx + dy * dy;
                let coverage = if d2 >= r_max_sq || d2 <= r_in_min_sq {
                    0.0
                } else if d2 <= r_min_sq && d2 >= r_in_max_sq {
                    1.0
                } else {
                    let d = d2.sqrt();
                    let cov_out = (r_f32 + 0.5 - d).clamp(0.0, 1.0);
                    let cov_in = (d - r_in + 0.5).clamp(0.0, 1.0);
                    cov_out * cov_in
                };
                corner_cov[(dy_idx * 64 + dx_idx) as usize] = coverage;
            }
        }
    }

    for (corner_idx, &(x1, y1, x2, y2, cx, cy)) in corners.iter().enumerate() {
        let start_x = x1.max(clip_x1);
        let start_y = y1.max(clip_y1);
        let end_x = x2.min(clip_x2);
        let end_y = y2.min(clip_y2);

        for py in start_y..end_y {
            for px in start_x..end_x {
                let coverage = if use_table {
                    let dx_idx = match corner_idx {
                        0 | 2 => px - rect_x,
                        1 | 3 => rect_x + rect_w - 1 - px,
                        _ => 0,
                    };
                    let dy_idx = match corner_idx {
                        0 | 1 => py - rect_y,
                        2 | 3 => rect_y + rect_h - 1 - py,
                        _ => 0,
                    };
                    if dx_idx >= 0 && dx_idx < 64 && dy_idx >= 0 && dy_idx < 64 {
                        corner_cov[(dy_idx * 64 + dx_idx) as usize]
                    } else {
                        0.0
                    }
                } else {
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    let d2 = dx * dx + dy * dy;
                    if d2 >= r_max_sq || d2 <= r_in_min_sq {
                        0.0
                    } else if d2 <= r_min_sq && d2 >= r_in_max_sq {
                        1.0
                    } else {
                        let d = d2.sqrt();
                        let cov_out = (r_f32 + 0.5 - d).clamp(0.0, 1.0);
                        let cov_in = (d - r_in + 0.5).clamp(0.0, 1.0);
                        cov_out * cov_in
                    }
                };

                if coverage > 0.0 {
                    let alpha = ((packed_border >> 24) & 0xff) as f32 * coverage;
                    let packed_col = (packed_border & 0x00ffffff) | ((alpha.round() as u32) << 24);

                    let idx = if rotate {
                        (px as usize * physical_w as usize) + (physical_w as usize - 1 - py as usize)
                    } else {
                        (py as usize * physical_w as usize) + px as usize
                    };

                    if idx < buffer.len() {
                        blend_pixel(&mut buffer[idx], packed_col);
                    }
                }
            }
        }
    }
}
