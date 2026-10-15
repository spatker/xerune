use crate::blitter::{pack_color, blend_solid_rect, blend_pixel, calc_pixel_index};
use crate::gradient::{draw_gradient_rect, draw_shaded_rounded_rect};
#[cfg(not(feature = "std"))]
use crate::F32Ext;
/// The span-based linear gradient path only handles horizontal and vertical
/// gradients.
fn is_axis_aligned(angle: f32) -> bool {
    let a = (angle % 90.0 + 90.0) % 90.0;
    a < 0.01 || a > 89.99
}

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
    gradient: Option<&xerune::Gradient>,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotation: u32,
) {
    if rect_w <= 0 || rect_h <= 0 {
        return;
    }

    let linear = match gradient {
        Some(xerune::Gradient::Linear(linear)) if radius <= 0.0 && is_axis_aligned(linear.angle) => Some(linear),
        Some(other) => {
            let rect = xerune::Rect { x: rect_x as f32, y: rect_y as f32, width: rect_w as f32, height: rect_h as f32 };
            draw_shaded_rounded_rect(buffer, logical_w, logical_h, physical_w, rect, radius, other, swap_rb, clip_rect, rotation);
            return;
        }
        None => None,
    };

    if radius <= 0.0 {
        if let Some(grad) = linear {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, grad, swap_rb, clip_rect, rotation);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, packed, clip_rect, rotation);
        }
        return;
    }

    // Gradients with rounded corners took the per-pixel path above.
    let Some(solid) = color else {
        return;
    };
    let packed_solid = pack_color(solid, swap_rb);

    let r_f32 = radius.min(rect_w as f32 / 2.0).min(rect_h as f32 / 2.0).max(0.0);
    let r_i32 = r_f32.ceil() as i32;

    // Center strip
    let center_y = rect_y + r_i32;
    let center_h = rect_h - 2 * r_i32;
    if center_h > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, center_y, rect_w, center_h, packed_solid, clip_rect, rotation);
    }

    // Top strip
    let top_x = rect_x + r_i32;
    let top_w = rect_w - 2 * r_i32;
    if top_w > 0 && r_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, rect_y, top_w, r_i32, packed_solid, clip_rect, rotation);
    }

    // Bottom strip
    let bottom_y = rect_y + rect_h - r_i32;
    if top_w > 0 && r_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, bottom_y, top_w, r_i32, packed_solid, clip_rect, rotation);
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

    let use_small_table = r_i32 <= 16;
    let use_table = r_i32 <= 64;
    let mut corner_cov_small = [0.0f32; 16 * 16];
    let mut corner_cov_large = Vec::new();

    if use_small_table {
        for dy_idx in 0..r_i32 {
            let row_off = dy_idx as usize * 16;
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
                corner_cov_small[row_off + dx_idx as usize] = coverage;
            }
        }
    } else if use_table {
        let size = (r_i32 * r_i32) as usize;
        corner_cov_large.resize(size, 0.0f32);
        for dy_idx in 0..r_i32 {
            let row_off = dy_idx as usize * r_i32 as usize;
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
                corner_cov_large[row_off + dx_idx as usize] = coverage;
            }
        }
    }

    for (corner_idx, &(x1, y1, x2, y2, cx, cy)) in corners.iter().enumerate() {
        let start_x = x1.max(clip_x1);
        let start_y = y1.max(clip_y1);
        let end_x = x2.min(clip_x2);
        let end_y = y2.min(clip_y2);

        let is_left = corner_idx == 0 || corner_idx == 2;
        let is_top = corner_idx == 0 || corner_idx == 1;

        for py in start_y..end_y {
            let dy_idx = if is_top { py - rect_y } else { rect_y + rect_h - 1 - py };
            if dy_idx < 0 || dy_idx >= r_i32 { continue; }

            if use_small_table {
                let row_off = dy_idx as usize * 16;
                for px in start_x..end_x {
                    let dx_idx = if is_left { px - rect_x } else { rect_x + rect_w - 1 - px };
                    if dx_idx >= 0 && dx_idx < r_i32 {
                        let coverage = corner_cov_small[row_off + dx_idx as usize];
                        if coverage > 0.0 {
                            let src_color = packed_solid;

                            let alpha = ((src_color >> 24) & 0xff) as f32 * coverage;
                            let packed_col = (src_color & 0x00ffffff) | ((alpha.round() as u32) << 24);

                            let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                            if idx < buffer.len() {
                                blend_pixel(&mut buffer[idx], packed_col);
                            }
                        }
                    }
                }
            } else if use_table {
                let row_off = dy_idx as usize * r_i32 as usize;
                for px in start_x..end_x {
                    let dx_idx = if is_left { px - rect_x } else { rect_x + rect_w - 1 - px };
                    if dx_idx >= 0 && dx_idx < r_i32 {
                        let coverage = corner_cov_large[row_off + dx_idx as usize];
                        if coverage > 0.0 {
                            let src_color = packed_solid;

                            let alpha = ((src_color >> 24) & 0xff) as f32 * coverage;
                            let packed_col = (src_color & 0x00ffffff) | ((alpha.round() as u32) << 24);

                            let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                            if idx < buffer.len() {
                                blend_pixel(&mut buffer[idx], packed_col);
                            }
                        }
                    }
                }
            } else {
                for px in start_x..end_x {
                    let dx = px as f32 + 0.5 - cx;
                    let dy = py as f32 + 0.5 - cy;
                    let d2 = dx * dx + dy * dy;
                    let coverage = if d2 >= r_max_sq {
                        0.0
                    } else if d2 <= r_min_sq {
                        1.0
                    } else {
                        (r_f32 + 0.5 - d2.sqrt()).clamp(0.0, 1.0)
                    };

                    if coverage > 0.0 {
                        let src_color = packed_solid;

                        let alpha = ((src_color >> 24) & 0xff) as f32 * coverage;
                        let packed_col = (src_color & 0x00ffffff) | ((alpha.round() as u32) << 24);

                        let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                        if idx < buffer.len() {
                            blend_pixel(&mut buffer[idx], packed_col);
                        }
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
    rotation: u32,
) {
    if rect_w <= 0 || rect_h <= 0 || border_width <= 0.0 {
        return;
    }

    let packed_border = pack_color(border_color, swap_rb);

    if radius <= 0.0 {
        let bw = border_width.round() as i32;
        // Top
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, bw, packed_border, clip_rect, rotation);
        // Bottom
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + rect_h - bw, rect_w, bw, packed_border, clip_rect, rotation);
        // Left
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + bw, bw, rect_h - 2 * bw, packed_border, clip_rect, rotation);
        // Right
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + rect_w - bw, rect_y + bw, bw, rect_h - 2 * bw, packed_border, clip_rect, rotation);
        return;
    }

    let r_f32 = radius.min(rect_w as f32 / 2.0).min(rect_h as f32 / 2.0).max(0.0);
    let r_i32 = r_f32.ceil() as i32;
    let bw_f32 = border_width;
    let bw_i32 = bw_f32.round() as i32;

    // Straight segments
    let top_w = rect_w - 2 * r_i32;
    if top_w > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + r_i32, rect_y, top_w, bw_i32, packed_border, clip_rect, rotation);
    }
    if top_w > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + r_i32, rect_y + rect_h - bw_i32, top_w, bw_i32, packed_border, clip_rect, rotation);
    }
    let side_h = rect_h - 2 * r_i32;
    if side_h > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y + r_i32, bw_i32, side_h, packed_border, clip_rect, rotation);
    }
    if side_h > 0 && bw_i32 > 0 {
        blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x + rect_w - bw_i32, rect_y + r_i32, bw_i32, side_h, packed_border, clip_rect, rotation);
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

    let use_small_table = r_i32 <= 16;
    let use_table = r_i32 <= 64;
    let mut corner_cov_small = [0.0f32; 16 * 16];
    let mut corner_cov_large = Vec::new();

    if use_small_table {
        for dy_idx in 0..r_i32 {
            let row_off = dy_idx as usize * 16;
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
                corner_cov_small[row_off + dx_idx as usize] = coverage;
            }
        }
    } else if use_table {
        let size = (r_i32 * r_i32) as usize;
        corner_cov_large.resize(size, 0.0f32);
        for dy_idx in 0..r_i32 {
            let row_off = dy_idx as usize * r_i32 as usize;
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
                corner_cov_large[row_off + dx_idx as usize] = coverage;
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
                let coverage = if use_small_table {
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
                    if dx_idx >= 0 && dx_idx < r_i32 && dy_idx >= 0 && dy_idx < r_i32 {
                        corner_cov_small[(dy_idx * 16 + dx_idx) as usize]
                    } else {
                        0.0
                    }
                } else if use_table {
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
                    if dx_idx >= 0 && dx_idx < r_i32 && dy_idx >= 0 && dy_idx < r_i32 {
                        corner_cov_large[(dy_idx * r_i32 + dx_idx) as usize]
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

                    let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                    if idx < buffer.len() {
                        blend_pixel(&mut buffer[idx], packed_col);
                    }
                }
            }
        }
    }
}

pub fn draw_box_shadow(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    radius: f32,
    offset_x: f32,
    offset_y: f32,
    blur_radius: f32,
    spread_radius: f32,
    shadow_color: xerune::Color,
    inset: bool,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotation: u32,
) {
    if rect_w <= 0 || rect_h <= 0 || shadow_color.a == 0 {
        return;
    }

    let orig_rx = rect_x as f32;
    let orig_ry = rect_y as f32;
    let orig_rw = rect_w as f32;
    let orig_rh = rect_h as f32;

    let sx = rect_x as f32 + offset_x - spread_radius;
    let sy = rect_y as f32 + offset_y - spread_radius;
    let sw = rect_w as f32 + spread_radius * 2.0;
    let sh = rect_h as f32 + spread_radius * 2.0;
    let s_radius = (radius + spread_radius).max(0.0);

    let blur = blur_radius.max(0.0);
    let expand = blur * 1.5;

    let (min_x, min_y, max_x, max_y) = if inset {
        (
            orig_rx.floor() as i32,
            orig_ry.floor() as i32,
            (orig_rx + orig_rw).ceil() as i32,
            (orig_ry + orig_rh).ceil() as i32,
        )
    } else {
        (
            (sx - expand).floor() as i32,
            (sy - expand).floor() as i32,
            (sx + sw + expand).ceil() as i32,
            (sy + sh + expand).ceil() as i32,
        )
    };

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

    let start_x = min_x.max(clip_x1);
    let start_y = min_y.max(clip_y1);
    let end_x = max_x.min(clip_x2);
    let end_y = max_y.min(clip_y2);

    if start_x >= end_x || start_y >= end_y {
        return;
    }

    let packed = pack_color(shadow_color, swap_rb);
    let base_a = ((packed >> 24) & 0xff) as f32;

    let orig_left = orig_rx + radius;
    let orig_right = orig_rx + orig_rw - radius;
    let orig_top = orig_ry + radius;
    let orig_bottom = orig_ry + orig_rh - radius;

    for py in start_y..end_y {
        let y_f = py as f32 + 0.5;
        for px in start_x..end_x {
            let x_f = px as f32 + 0.5;

            // Check if inside the original element border box
            let odx = if x_f < orig_left { orig_left - x_f } else if x_f > orig_right { x_f - orig_right } else { 0.0 };
            let ody = if y_f < orig_top { orig_top - y_f } else if y_f > orig_bottom { y_f - orig_bottom } else { 0.0 };
            let is_inside_element = if odx > 0.0 && ody > 0.0 {
                (odx * odx + ody * ody) <= radius * radius
            } else {
                x_f >= orig_rx && x_f <= orig_rx + orig_rw && y_f >= orig_ry && y_f <= orig_ry + orig_rh
            };

            if !inset && is_inside_element {
                continue;
            }
            if inset && !is_inside_element {
                continue;
            }

            let box_r = s_radius.min(sw / 2.0).min(sh / 2.0).max(0.0);
            let b_left = sx + box_r;
            let b_right = sx + sw - box_r;
            let b_top = sy + box_r;
            let b_bottom = sy + sh - box_r;

            let dx = if x_f < b_left { b_left - x_f } else if x_f > b_right { x_f - b_right } else { 0.0 };
            let dy = if y_f < b_top { b_top - y_f } else if y_f > b_bottom { y_f - b_bottom } else { 0.0 };

            let dist_from_edge = if dx > 0.0 && dy > 0.0 {
                (dx * dx + dy * dy).sqrt() - box_r
            } else if dx > 0.0 {
                dx - box_r
            } else if dy > 0.0 {
                dy - box_r
            } else {
                let d_left = x_f - sx;
                let d_right = (sx + sw) - x_f;
                let d_top = y_f - sy;
                let d_bottom = (sy + sh) - y_f;
                -d_left.min(d_right).min(d_top).min(d_bottom)
            };

            let alpha_factor = if !inset {
                if blur <= 0.5 {
                    if dist_from_edge <= 0.0 { 1.0 } else { 0.0 }
                } else {
                    let u = (dist_from_edge / blur).clamp(-1.0, 1.0);
                    let t = 0.5 - 0.5 * u;
                    t * t * (3.0 - 2.0 * t)
                }
            } else {
                let d_inside = -dist_from_edge;
                if blur <= 0.5 {
                    if d_inside <= spread_radius { 1.0 } else { 0.0 }
                } else {
                    let u = ((d_inside - spread_radius) / blur).clamp(-1.0, 1.0);
                    let t = 0.5 - 0.5 * u;
                    t * t * (3.0 - 2.0 * t)
                }
            };

            if alpha_factor > 0.0 {
                let final_a = (base_a * alpha_factor).round() as u32;
                if final_a > 0 {
                    let pixel_color = (packed & 0x00ffffff) | (final_a << 24);
                    let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);

                    if idx < buffer.len() {
                        blend_pixel(&mut buffer[idx], pixel_color);
                    }
                }
            }
        }
    }
}

/// Draws a border whose sides differ in width or color. Each side is a solid
/// band; in a corner, the line from the outer to the inner corner divides
/// the two sides, as in CSS, and the outer edge follows `radius`. Corner
/// coverage comes from distances; only pixels the diagonal crosses are
/// sampled.
pub fn draw_border_sides(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect_x: i32,
    rect_y: i32,
    rect_w: i32,
    rect_h: i32,
    radius: f32,
    border: &xerune::style::Borders,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotation: u32,
) {
    if rect_w <= 0 || rect_h <= 0 {
        return;
    }
    let width = |s: &xerune::style::BorderSide| (s.width.round().max(0.0) as i32).min(rect_w.max(rect_h));
    let color = |s: &xerune::style::BorderSide| pack_color(s.color.unwrap_or(xerune::Color::from_rgba8(0, 0, 0, 0)), swap_rb);
    let (bt, br, bb, bl) = (width(&border.top), width(&border.right), width(&border.bottom), width(&border.left));
    let (ct, cr, cb, cl) = (color(&border.top), color(&border.right), color(&border.bottom), color(&border.left));
    let r = radius.min(rect_w as f32 / 2.0).min(rect_h as f32 / 2.0).max(0.0);
    let ri = r.ceil() as i32;

    // Corner boxes (width, height): top-left, top-right, bottom-left, bottom-right.
    let corner = [(bl.max(ri), bt.max(ri)), (br.max(ri), bt.max(ri)), (bl.max(ri), bb.max(ri)), (br.max(ri), bb.max(ri))];
    let band = |buffer: &mut [u32], x: i32, y: i32, w: i32, h: i32, c: u32| {
        if w > 0 && h > 0 {
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, x, y, w, h, c, clip_rect, rotation);
        }
    };
    band(buffer, rect_x + corner[0].0, rect_y, rect_w - corner[0].0 - corner[1].0, bt, ct);
    band(buffer, rect_x + corner[2].0, rect_y + rect_h - bb, rect_w - corner[2].0 - corner[3].0, bb, cb);
    band(buffer, rect_x, rect_y + corner[0].1, bl, rect_h - corner[0].1 - corner[2].1, cl);
    band(buffer, rect_x + rect_w - br, rect_y + corner[1].1, br, rect_h - corner[1].1 - corner[3].1, cr);

    let (clip_x1, clip_y1, clip_x2, clip_y2) = match clip_rect {
        Some(cr) => (
            cr.x.max(0.0) as i32,
            cr.y.max(0.0) as i32,
            (cr.x + cr.width).min(logical_w as f32) as i32,
            (cr.y + cr.height).min(logical_h as f32) as i32,
        ),
        None => (0, 0, logical_w as i32, logical_h as i32),
    };
    // Per corner: box, then the horizontal side (top/bottom) and the vertical
    // side (left/right) as (width, color), and whether it is on the right/bottom.
    let corners = [
        (corner[0], (bt, ct), (bl, cl), false, false),
        (corner[1], (bt, ct), (br, cr), true, false),
        (corner[2], (bb, cb), (bl, cl), false, true),
        (corner[3], (bb, cb), (br, cr), true, true),
    ];
    for ((cw, ch), (bh, colh), (bv, colv), right, bottom) in corners {
        if bh == 0 && bv == 0 {
            continue; // no border meets here
        }
        let (bh_f, bv_f) = (bh as f32, bv as f32);
        let (rx, ry) = (r - bv_f, r - bh_f);
        let rounded_inner = rx > 0.0 && ry > 0.0;
        let (inv_rx, inv_ry) = if rounded_inner { (1.0 / rx, 1.0 / ry) } else { (0.0, 0.0) };
        for j in 0..ch.min(rect_h) {
            let py = if bottom { rect_y + rect_h - 1 - j } else { rect_y + j };
            if py < clip_y1 || py >= clip_y2 {
                continue;
            }
            for i in 0..cw.min(rect_w) {
                let px = if right { rect_x + rect_w - 1 - i } else { rect_x + i };
                if px < clip_x1 || px >= clip_x2 {
                    continue;
                }
                // (u, v): distance of the pixel center from the outer corner,
                // growing inwards.
                let (u, v) = (i as f32 + 0.5, j as f32 + 0.5);
                let in_arc = u < r && v < r;
                // Coverage of the area inside the outer edge...
                let outer = if in_arc {
                    let (dx, dy) = (r - u, r - v);
                    (r + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                if outer == 0.0 {
                    continue;
                }
                // ...and outside the inner edge (the padding box).
                let inner = if rounded_inner && in_arc && u > bv_f && v > bh_f {
                    let (ex, ey) = ((r - u) * inv_rx, (r - v) * inv_ry);
                    let e = (ex * ex + ey * ey).sqrt();
                    ((e - 1.0) * 0.5 * (rx + ry) + 0.5).clamp(0.0, 1.0)
                } else {
                    1.0 - (u + 0.5 - bv_f).clamp(0.0, 1.0) * (v + 0.5 - bh_f).clamp(0.0, 1.0)
                };
                let coverage = outer * inner;
                if coverage == 0.0 {
                    continue;
                }
                // The diagonal from the outer to the inner corner divides the
                // sides; sample only pixels it crosses.
                let horizontal_share = if bv == 0 {
                    1.0
                } else if bh == 0 {
                    0.0
                } else {
                    let below = |u: f32, v: f32| v * bv_f > u * bh_f;
                    let (u0, v0) = (i as f32, j as f32);
                    let corners_below = [(u0, v0), (u0 + 1.0, v0), (u0, v0 + 1.0), (u0 + 1.0, v0 + 1.0)]
                        .iter().filter(|&&(cu, cv)| below(cu, cv)).count();
                    match corners_below {
                        0 => 1.0,
                        4 => 0.0,
                        _ => {
                            let mut above = 0;
                            for sy in 0..4 {
                                for sx in 0..4 {
                                    if !below(u0 + (sx as f32 + 0.5) / 4.0, v0 + (sy as f32 + 0.5) / 4.0) {
                                        above += 1;
                                    }
                                }
                            }
                            above as f32 / 16.0
                        }
                    }
                };
                let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);
                if idx >= buffer.len() {
                    continue;
                }
                for (share, packed) in [(horizontal_share, colh), (1.0 - horizontal_share, colv)] {
                    let alpha = ((packed >> 24) & 0xff) as f32 * coverage * share;
                    if alpha >= 0.5 {
                        blend_pixel(&mut buffer[idx], (packed & 0x00ff_ffff) | ((alpha.round() as u32) << 24));
                    }
                }
            }
        }
    }
}
