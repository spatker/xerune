use crate::blitter::{pack_color, blend_solid_rect, blend_pixel, calc_pixel_index};
use crate::gradient::{draw_gradient_rect, sample_gradient};
#[cfg(not(feature = "std"))]
use crate::F32Ext;
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
    rotation: u32,
) {
    if rect_w <= 0 || rect_h <= 0 {
        return;
    }

    if radius <= 0.0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, grad, swap_rb, clip_rect, rotation);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, rect_y, rect_w, rect_h, packed, clip_rect, rotation);
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
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, rect_x, center_y, rect_w, center_h, grad, swap_rb, clip_rect, rotation);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, rect_x, center_y, rect_w, center_h, packed, clip_rect, rotation);
        }
    }

    // Top strip
    let top_x = rect_x + r_i32;
    let top_w = rect_w - 2 * r_i32;
    if top_w > 0 && r_i32 > 0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, top_x, rect_y, top_w, r_i32, grad, swap_rb, clip_rect, rotation);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, rect_y, top_w, r_i32, packed, clip_rect, rotation);
        }
    }

    // Bottom strip
    let bottom_y = rect_y + rect_h - r_i32;
    if top_w > 0 && r_i32 > 0 {
        if let Some(grad) = gradient {
            draw_gradient_rect(buffer, logical_w, logical_h, physical_w, top_x, bottom_y, top_w, r_i32, grad, swap_rb, clip_rect, rotation);
        } else if let Some(col) = color {
            let packed = pack_color(col, swap_rb);
            blend_solid_rect(buffer, logical_w, logical_h, physical_w, top_x, bottom_y, top_w, r_i32, packed, clip_rect, rotation);
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

