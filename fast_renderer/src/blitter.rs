#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;

#[cfg(target_arch = "x86")]
use core::arch::x86::*;

#[cfg(target_arch = "aarch64")]
use core::arch::aarch64::*;

#[cfg(all(target_arch = "arm", target_feature = "neon"))]
use core::arch::arm::*;

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
        let inv_a = 255 - src_a;
        
        let src_ag = ((src & 0xFF00FF00) >> 8) * src_a;
        let src_rb = (src & 0x00FF00FF) * src_a;
        
        let ag = ((d & 0xFF00FF00) >> 8) * inv_a + src_ag;
        let rb = (d & 0x00FF00FF) * inv_a + src_rb;
        
        let temp_ag = ag + ((ag >> 8) & 0x00FF00FF) + 0x00010001;
        let temp_rb = rb + ((rb >> 8) & 0x00FF00FF) + 0x00010001;
        
        *dst = (temp_ag & 0xFF00FF00) | ((temp_rb >> 8) & 0x00FF00FF);
    }
}

pub fn blend_solid_span(dst: &mut [u32], color: u32) {
    let a = (color >> 24) & 0xff;
    if a == 255 {
        dst.fill(color);
    } else if a > 0 {
        let inv_a = 255 - a;
        let src_b0 = color & 0xFF;
        let src_b1 = (color >> 8) & 0xFF;
        let src_b2 = (color >> 16) & 0xFF;
        let src_b3 = (color >> 24) & 0xFF;

        let scale_b0 = (src_b0 * a) as u16;
        let scale_b1 = (src_b1 * a) as u16;
        let scale_b2 = (src_b2 * a) as u16;
        let scale_b3 = (src_b3 * a) as u16;
        
        let mut chunks_std = dst;
        
        #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
        {
            let (simd_slice, remainder) = chunks_std.split_at_mut(chunks_std.len() - chunks_std.len() % 4);
            chunks_std = remainder;
            
            unsafe {
                let inv_a_vec = _mm_set1_epi16(inv_a as i16);
                let src_scale = _mm_set_epi16(
                    scale_b3 as i16, scale_b2 as i16, scale_b1 as i16, scale_b0 as i16,
                    scale_b3 as i16, scale_b2 as i16, scale_b1 as i16, scale_b0 as i16,
                );
                
                let mut ptr = simd_slice.as_mut_ptr();
                let end = ptr.add(simd_slice.len());
                while ptr < end {
                    let dst_vec = _mm_loadu_si128(ptr as *const __m128i);
                    let low = _mm_unpacklo_epi8(dst_vec, _mm_setzero_si128());
                    let high = _mm_unpackhi_epi8(dst_vec, _mm_setzero_si128());
                    
                    let low_res = _mm_add_epi16(_mm_mullo_epi16(low, inv_a_vec), src_scale);
                    let high_res = _mm_add_epi16(_mm_mullo_epi16(high, inv_a_vec), src_scale);
                    
                    let temp_low = _mm_add_epi16(low_res, _mm_set1_epi16(1));
                    let temp_low = _mm_add_epi16(temp_low, _mm_srli_epi16(low_res, 8));
                    let res_low = _mm_srli_epi16(temp_low, 8);
                    
                    let temp_high = _mm_add_epi16(high_res, _mm_set1_epi16(1));
                    let temp_high = _mm_add_epi16(temp_high, _mm_srli_epi16(high_res, 8));
                    let res_high = _mm_srli_epi16(temp_high, 8);
                    
                    let packed = _mm_packus_epi16(res_low, res_high);
                    _mm_storeu_si128(ptr as *mut __m128i, packed);
                    ptr = ptr.add(4);
                }
            }
        }
        
        #[cfg(any(target_arch = "aarch64", all(target_arch = "arm", target_feature = "neon")))]
        {
            let (simd_slice, remainder) = chunks_std.split_at_mut(chunks_std.len() - chunks_std.len() % 4);
            chunks_std = remainder;
            
            unsafe {
                let inv_a_vec = vdupq_n_u16(inv_a as u16);
                let src_scale_arr = [
                    scale_b0, scale_b1, scale_b2, scale_b3,
                    scale_b0, scale_b1, scale_b2, scale_b3,
                ];
                let src_scale_vec = vld1q_u16(src_scale_arr.as_ptr());
                let one_vec = vdupq_n_u16(1);
                
                let mut ptr = simd_slice.as_mut_ptr();
                let end = ptr.add(simd_slice.len());
                while ptr < end {
                    let dst_vec = vld1q_u8(ptr as *const u8);
                    let low = vmovl_u8(vget_low_u8(dst_vec));
                    let high = vmovl_u8(vget_high_u8(dst_vec));
                    
                    let low_res = vaddq_u16(vmulq_u16(low, inv_a_vec), src_scale_vec);
                    let high_res = vaddq_u16(vmulq_u16(high, inv_a_vec), src_scale_vec);
                    
                    let temp_low = vaddq_u16(low_res, one_vec);
                    let temp_low = vaddq_u16(temp_low, vshrq_n_u16(low_res, 8));
                    let res_low = vshrq_n_u16(temp_low, 8);
                    
                    let temp_high = vaddq_u16(high_res, one_vec);
                    let temp_high = vaddq_u16(temp_high, vshrq_n_u16(high_res, 8));
                    let res_high = vshrq_n_u16(temp_high, 8);
                    
                    let packed = vcombine_u8(vqmovn_u16(res_low), vqmovn_u16(res_high));
                    vst1q_u8(ptr as *mut u8, packed);
                    ptr = ptr.add(4);
                }
            }
        }
        
        // Scalar fallback loop for remainder
        let src_ag = ((color & 0xFF00FF00) >> 8) * a;
        let src_rb = (color & 0x00FF00FF) * a;
        for pixel in chunks_std.iter_mut() {
            let d = *pixel;
            let ag = ((d & 0xFF00FF00) >> 8) * inv_a + src_ag;
            let rb = (d & 0x00FF00FF) * inv_a + src_rb;
            
            let temp_ag = ag + ((ag >> 8) & 0x00FF00FF) + 0x00010001;
            let temp_rb = rb + ((rb >> 8) & 0x00FF00FF) + 0x00010001;
            
            *pixel = (temp_ag & 0xFF00FF00) | ((temp_rb >> 8) & 0x00FF00FF);
        }
    }
}

#[inline(always)]
pub fn calc_pixel_index(x: i32, y: i32, rotation: u32, physical_w: u32, logical_w: u32, logical_h: u32) -> usize {
    match rotation {
        90 => (x as usize * physical_w as usize) + (physical_w as usize - 1 - y as usize),
        180 => ((logical_h as usize - 1 - y as usize) * physical_w as usize) + (physical_w as usize - 1 - x as usize),
        270 => ((logical_w as usize - 1 - x as usize) * physical_w as usize) + (y as usize),
        _ => (y as usize * physical_w as usize) + (x as usize),
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
    rotation: u32,
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

    let a = (color >> 24) & 0xff;
    let pw = physical_w as usize;

    match rotation {
        90 => {
            let span_len = (end_y - start_y) as usize;
            for x in start_x..end_x {
                let start_idx = (x as usize * pw) + (pw - end_y as usize);
                if start_idx + span_len <= buffer.len() {
                    let dst_span = &mut buffer[start_idx..start_idx + span_len];
                    if a == 255 {
                        dst_span.fill(color);
                    } else {
                        blend_solid_span(dst_span, color);
                    }
                }
            }
        }
        270 => {
            let span_len = (end_y - start_y) as usize;
            let lw = logical_w as usize;
            for x in start_x..end_x {
                let py = lw - 1 - x as usize;
                let start_idx = py * pw + start_y as usize;
                if start_idx + span_len <= buffer.len() {
                    let dst_span = &mut buffer[start_idx..start_idx + span_len];
                    if a == 255 {
                        dst_span.fill(color);
                    } else {
                        blend_solid_span(dst_span, color);
                    }
                }
            }
        }
        180 => {
            let span_w = (end_x - start_x) as usize;
            let lh = logical_h as usize;
            for y in start_y..end_y {
                let py = lh - 1 - y as usize;
                let start_idx = py * pw + (pw - end_x as usize);
                if start_idx + span_w <= buffer.len() {
                    let dst_span = &mut buffer[start_idx..start_idx + span_w];
                    if a == 255 {
                        dst_span.fill(color);
                    } else {
                        blend_solid_span(dst_span, color);
                    }
                }
            }
        }
        _ => {
            let span_w = (end_x - start_x) as usize;
            for y in start_y..end_y {
                let start_idx = (y as usize * pw) + start_x as usize;
                if start_idx + span_w <= buffer.len() {
                    let dst_span = &mut buffer[start_idx..start_idx + span_w];
                    if a == 255 {
                        dst_span.fill(color);
                    } else {
                        blend_solid_span(dst_span, color);
                    }
                }
            }
        }
    }
}

pub fn blend_glyph_span(dst: &mut [u32], coverage: &[u8], color: u32) {
    let color_a = (color >> 24) & 0xff;
    if color_a == 0 {
        return;
    }

    let src_b0 = (color & 0xFF) as i16;
    let src_b1 = ((color >> 8) & 0xFF) as i16;
    let src_b2 = ((color >> 16) & 0xFF) as i16;
    let src_b3 = ((color >> 24) & 0xFF) as i16;

    let mut chunks_dst = dst;
    let mut chunks_cov = coverage;

    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        let len = chunks_dst.len().min(chunks_cov.len());
        let simd_len = len - len % 4;
        let (simd_dst, rem_dst) = chunks_dst.split_at_mut(simd_len);
        let (simd_cov, rem_cov) = chunks_cov.split_at(simd_len);
        chunks_dst = rem_dst;
        chunks_cov = rem_cov;

        unsafe {
            let color_a_vec = _mm_set1_epi16(color_a as i16);
            let src_channels = _mm_set_epi16(src_b3, src_b2, src_b1, src_b0, src_b3, src_b2, src_b1, src_b0);
            
            let mut ptr_dst = simd_dst.as_mut_ptr();
            let mut ptr_cov = simd_cov.as_ptr();
            let end_dst = ptr_dst.add(simd_len);

            while ptr_dst < end_dst {
                let cov_u32 = core::ptr::read_unaligned(ptr_cov as *const u32);
                if cov_u32 == 0 {
                    ptr_dst = ptr_dst.add(4);
                    ptr_cov = ptr_cov.add(4);
                    continue;
                }

                let cov_vec = _mm_cvtsi32_si128(cov_u32 as i32);
                let cov_16 = _mm_unpacklo_epi8(cov_vec, _mm_setzero_si128());
                let alpha_mul = _mm_mullo_epi16(cov_16, color_a_vec);
                
                let temp_alpha = _mm_add_epi16(alpha_mul, _mm_set1_epi16(1));
                let temp_alpha = _mm_add_epi16(temp_alpha, _mm_srli_epi16(alpha_mul, 8));
                let a_vec = _mm_srli_epi16(temp_alpha, 8);
                
                let inv_a_vec = _mm_sub_epi16(_mm_set1_epi16(255), a_vec);
                
                let a_low = _mm_unpacklo_epi16(a_vec, a_vec);
                let low_a_vec = _mm_unpacklo_epi32(a_low, a_low);
                let high_a_vec = _mm_unpackhi_epi32(a_low, a_low);
                
                let inv_a_low = _mm_unpacklo_epi16(inv_a_vec, inv_a_vec);
                let low_inv_a_vec = _mm_unpacklo_epi32(inv_a_low, inv_a_low);
                let high_inv_a_vec = _mm_unpackhi_epi32(inv_a_low, inv_a_low);

                let dst_vec = _mm_loadu_si128(ptr_dst as *const __m128i);
                let low_dst = _mm_unpacklo_epi8(dst_vec, _mm_setzero_si128());
                let high_dst = _mm_unpackhi_epi8(dst_vec, _mm_setzero_si128());

                let low_res = _mm_add_epi16(_mm_mullo_epi16(src_channels, low_a_vec), _mm_mullo_epi16(low_dst, low_inv_a_vec));
                let high_res = _mm_add_epi16(_mm_mullo_epi16(src_channels, high_a_vec), _mm_mullo_epi16(high_dst, high_inv_a_vec));

                let temp_low = _mm_add_epi16(low_res, _mm_set1_epi16(1));
                let temp_low = _mm_add_epi16(temp_low, _mm_srli_epi16(low_res, 8));
                let res_low = _mm_srli_epi16(temp_low, 8);
                
                let temp_high = _mm_add_epi16(high_res, _mm_set1_epi16(1));
                let temp_high = _mm_add_epi16(temp_high, _mm_srli_epi16(high_res, 8));
                let res_high = _mm_srli_epi16(temp_high, 8);

                let packed = _mm_packus_epi16(res_low, res_high);
                _mm_storeu_si128(ptr_dst as *mut __m128i, packed);

                ptr_dst = ptr_dst.add(4);
                ptr_cov = ptr_cov.add(4);
            }
        }
    }

    #[cfg(any(target_arch = "aarch64", all(target_arch = "arm", target_feature = "neon")))]
    {
        let len = chunks_dst.len().min(chunks_cov.len());
        let simd_len = len - len % 4;
        let (simd_dst, rem_dst) = chunks_dst.split_at_mut(simd_len);
        let (simd_cov, rem_cov) = chunks_cov.split_at(simd_len);
        chunks_dst = rem_dst;
        chunks_cov = rem_cov;

        unsafe {
            let one_vec = vdupq_n_u16(1);
            let src_channels = vld1q_u16([src_b0, src_b1, src_b2, src_b3, src_b0, src_b1, src_b2, src_b3].as_ptr() as *const u16);
            
            let mut ptr_dst = simd_dst.as_mut_ptr();
            let mut ptr_cov = simd_cov.as_ptr();
            let end_dst = ptr_dst.add(simd_len);

            while ptr_dst < end_dst {
                let cov_u32 = core::ptr::read_unaligned(ptr_cov as *const u32);
                if cov_u32 == 0 {
                    ptr_dst = ptr_dst.add(4);
                    ptr_cov = ptr_cov.add(4);
                    continue;
                }

                let cov_vec = vcreate_u8(cov_u32 as u64);
                let cov_16 = vget_low_u16(vmovl_u8(cov_vec));
                let alpha_mul = vmul_n_u16(cov_16, color_a as u16);
                
                let temp_alpha = vadd_u16(alpha_mul, vdup_n_u16(1));
                let temp_alpha = vadd_u16(temp_alpha, vshr_n_u16(alpha_mul, 8));
                let a_vec = vshr_n_u16(temp_alpha, 8);
                
                let inv_a_vec = vsub_u16(vdup_n_u16(255), a_vec);
                
                let low_a_vec = vcombine_u16(vdup_lane_u16(a_vec, 0), vdup_lane_u16(a_vec, 1));
                let high_a_vec = vcombine_u16(vdup_lane_u16(a_vec, 2), vdup_lane_u16(a_vec, 3));
                
                let low_inv_a_vec = vcombine_u16(vdup_lane_u16(inv_a_vec, 0), vdup_lane_u16(inv_a_vec, 1));
                let high_inv_a_vec = vcombine_u16(vdup_lane_u16(inv_a_vec, 2), vdup_lane_u16(inv_a_vec, 3));

                let dst_vec = vld1q_u8(ptr_dst as *const u8);
                let low_dst = vmovl_u8(vget_low_u8(dst_vec));
                let high_dst = vmovl_u8(vget_high_u8(dst_vec));

                let low_res = vaddq_u16(vmulq_u16(src_channels, low_a_vec), vmulq_u16(low_dst, low_inv_a_vec));
                let high_res = vaddq_u16(vmulq_u16(src_channels, high_a_vec), vmulq_u16(high_dst, high_inv_a_vec));

                let temp_low = vaddq_u16(low_res, one_vec);
                let temp_low = vaddq_u16(temp_low, vshrq_n_u16(low_res, 8));
                let res_low = vshrq_n_u16(temp_low, 8);
                
                let temp_high = vaddq_u16(high_res, one_vec);
                let temp_high = vaddq_u16(temp_high, vshrq_n_u16(high_res, 8));
                let res_high = vshrq_n_u16(temp_high, 8);

                let packed = vcombine_u8(vqmovn_u16(res_low), vqmovn_u16(res_high));
                vst1q_u8(ptr_dst as *mut u8, packed);

                ptr_dst = ptr_dst.add(4);
                ptr_cov = ptr_cov.add(4);
            }
        }
    }

    // Scalar fallback loop for remainder
    for (pixel, &cov) in chunks_dst.iter_mut().zip(chunks_cov.iter()) {
        if cov > 0 {
            let a = div_255(color_a * cov as u32);
            if a == 255 {
                *pixel = color;
            } else if a > 0 {
                let d = *pixel;
                let inv_a = 255 - a;
                
                let src_ag = ((color & 0xFF00FF00) >> 8) * a;
                let src_rb = (color & 0x00FF00FF) * a;
                
                let ag = ((d & 0xFF00FF00) >> 8) * inv_a + src_ag;
                let rb = (d & 0x00FF00FF) * inv_a + src_rb;
                
                let temp_ag = ag + ((ag >> 8) & 0x00FF00FF) + 0x00010001;
                let temp_rb = rb + ((rb >> 8) & 0x00FF00FF) + 0x00010001;
                
                *pixel = (temp_ag & 0xFF00FF00) | ((temp_rb >> 8) & 0x00FF00FF);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scalar_blend_pixel(dst: u32, color: u32) -> u32 {
        let a = (color >> 24) & 0xff;
        if a == 255 {
            return color;
        }
        if a == 0 {
            return dst;
        }
        let inv_a = 255 - a;
        let src_ag = ((color & 0xFF00FF00) >> 8) * a;
        let src_rb = (color & 0x00FF00FF) * a;
        let ag = ((dst & 0xFF00FF00) >> 8) * inv_a + src_ag;
        let rb = (dst & 0x00FF00FF) * inv_a + src_rb;

        let temp_ag = ag + ((ag >> 8) & 0x00FF00FF) + 0x00010001;
        let temp_rb = rb + ((rb >> 8) & 0x00FF00FF) + 0x00010001;

        (temp_ag & 0xFF00FF00) | ((temp_rb >> 8) & 0x00FF00FF)
    }

    #[test]
    fn blend_solid_span_simd_matches_scalar_reference() {
        let colors = [
            0xC8FFFFFF,
            0x80123456,
            0x01AABBCC,
            0xFEFEDCBA,
            0x7F808080,
            0x00000000,
            0xFFFFFFFF,
        ];
        let bg_patterns = [
            0x00000000,
            0xFFFFFFFF,
            0xFF000000,
            0xFF112233,
            0x88445566,
        ];

        for &color in &colors {
            for &bg in &bg_patterns {
                for len in [1, 2, 3, 4, 5, 7, 8, 16] {
                    let mut buffer = vec![bg; len];
                    blend_solid_span(&mut buffer, color);
                    let expected: Vec<u32> = (0..len).map(|_| scalar_blend_pixel(bg, color)).collect();
                    assert_eq!(
                        buffer,
                        expected,
                        "Mismatch for color {:#010X} over bg {:#010X} with length {}",
                        color,
                        bg,
                        len
                    );
                }
            }
        }
    }
}
