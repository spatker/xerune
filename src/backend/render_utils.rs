/// Translate touch/mouse screen space coordinates `(touch_x, touch_y)` to logical layout coordinates based on display rotation.
pub fn map_touch_to_logical(touch_x: f32, touch_y: f32, disp_w: f32, disp_h: f32, rotation: u32) -> (f32, f32) {
    match rotation {
        90 => (touch_y, disp_w - 1.0 - touch_x),
        180 => (disp_w - 1.0 - touch_x, disp_h - 1.0 - touch_y),
        270 => (disp_h - 1.0 - touch_y, touch_x),
        _ => (touch_x, touch_y),
    }
}

/// Blit local logical ARGB8888 buffer to target physical display slice with rotation support (0°, 90°, 180°, 270°).
pub fn blit_rotated(
    local_buffer: &[u32],
    draw_slice: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    disp_w: u32,
    disp_h: u32,
    rotation: u32,
) {
    let lw = logical_w as usize;
    let lh = logical_h as usize;
    let dw = disp_w as usize;
    let dh = disp_h as usize;

    const BLOCK: usize = 32;
    let mut block_buf = [0u32; BLOCK * BLOCK];

    match rotation {
        90 => {
            for ty in (0..dh).step_by(BLOCK) {
                let ty_end = (ty + BLOCK).min(dh);
                let bh = ty_end - ty;
                for tx in (0..dw).step_by(BLOCK) {
                    let tx_end = (tx + BLOCK).min(dw);
                    let bw = tx_end - tx;

                    for py in 0..bh {
                        let y_p = ty + py;
                        let x_l = y_p;
                        let block_row = py * BLOCK;
                        for px in 0..bw {
                            let x_p = tx + px;
                            let y_l = (lh - 1) - x_p;
                            block_buf[block_row + px] = local_buffer[y_l * lw + x_l];
                        }
                    }

                    for py in 0..bh {
                        let dst_offset = (ty + py) * dw + tx;
                        let src_offset = py * BLOCK;
                        draw_slice[dst_offset..dst_offset + bw].copy_from_slice(&block_buf[src_offset..src_offset + bw]);
                    }
                }
            }
        }
        180 => {
            for py in 0..dh {
                let dst_offset = py * dw;
                let src_y = (lh - 1 - py) * lw;
                for px in 0..dw {
                    let src_x = lw - 1 - px;
                    draw_slice[dst_offset + px] = local_buffer[src_y + src_x];
                }
            }
        }
        270 => {
            for ty in (0..dh).step_by(BLOCK) {
                let ty_end = (ty + BLOCK).min(dh);
                let bh = ty_end - ty;
                for tx in (0..dw).step_by(BLOCK) {
                    let tx_end = (tx + BLOCK).min(dw);
                    let bw = tx_end - tx;

                    for py in 0..bh {
                        let y_p = ty + py;
                        let x_l = (lw - 1) - y_p;
                        let block_row = py * BLOCK;
                        for px in 0..bw {
                            let x_p = tx + px;
                            let y_l = x_p;
                            block_buf[block_row + px] = local_buffer[y_l * lw + x_l];
                        }
                    }

                    for py in 0..bh {
                        let dst_offset = (ty + py) * dw + tx;
                        let src_offset = py * BLOCK;
                        draw_slice[dst_offset..dst_offset + bw].copy_from_slice(&block_buf[src_offset..src_offset + bw]);
                    }
                }
            }
        }
        _ => {
            draw_slice.copy_from_slice(local_buffer);
        }
    }
}
