use crate::blitter::{pack_color, blend_solid_span, blend_pixel, calc_pixel_index};
#[cfg(not(feature = "std"))]
use crate::F32Ext;
use xerune::alloc_prelude::Vec;
/// Sample a stop list at `t`, interpolating in premultiplied alpha like CSS
/// does (so fading to `transparent` doesn't pass through grey). Stops must be
/// sorted by position; at a hard stop the later color wins.
pub fn sample_gradient(stops: &[(xerune::Color, f32)], t: f32) -> xerune::Color {
    let Some(first) = stops.first() else {
        return xerune::Color::BLACK;
    };
    let Some(i) = stops.iter().position(|s| s.1 > t) else {
        return stops[stops.len() - 1].0;
    };
    if i == 0 {
        return first.0;
    }
    let (left, right) = (stops[i - 1], stops[i]);
    let f = (t - left.1) / (right.1 - left.1);
    if left.0 == right.0 || f <= 0.0 {
        return left.0;
    }

    let la = left.0.a as f32;
    let ra = right.0.a as f32;
    let a = la + (ra - la) * f;
    if a <= 0.0 {
        return xerune::Color::new(0, 0, 0, 0);
    }
    let channel = |l: u8, r: u8| -> u8 {
        let premul = l as f32 * la + (r as f32 * ra - l as f32 * la) * f;
        (premul / a + 0.5) as u8
    };
    xerune::Color::new(
        channel(left.0.r, right.0.r),
        channel(left.0.g, right.0.g),
        channel(left.0.b, right.0.b),
        (a + 0.5) as u8,
    )
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

    if start_x >= end_x || start_y >= end_y || rect_w <= 0 || rect_h <= 0 {
        return;
    }

    let angle_normalized = (gradient.angle % 360.0 + 360.0) % 360.0;
    let is_horizontal = (angle_normalized - 90.0).abs() < 45.0 || (angle_normalized - 270.0).abs() < 45.0;

    let pw = physical_w as usize;

    if is_horizontal {
        let is_reverse = (angle_normalized - 270.0).abs() < 45.0;
        let mut span_colors = Vec::with_capacity(rect_w as usize);
        for dx in 0..rect_w {
            let t = if is_reverse {
                1.0 - (dx as f32 / rect_w as f32)
            } else {
                dx as f32 / rect_w as f32
            };
            let col = sample_gradient(&gradient.stops, t);
            span_colors.push(pack_color(col, swap_rb));
        }

        match rotation {
            90 => {
                let span_len = (end_y - start_y) as usize;
                for px in start_x..end_x {
                    let color = span_colors[(px - rect_x) as usize];
                    let start_idx = (px as usize * pw) + (pw - end_y as usize);
                    if start_idx + span_len <= buffer.len() {
                        let dst_span = &mut buffer[start_idx..start_idx + span_len];
                        blend_solid_span(dst_span, color);
                    }
                }
            }
            270 => {
                let span_len = (end_y - start_y) as usize;
                let lw = logical_w as usize;
                for px in start_x..end_x {
                    let color = span_colors[(px - rect_x) as usize];
                    let py_phys = lw - 1 - px as usize;
                    let start_idx = py_phys * pw + start_y as usize;
                    if start_idx + span_len <= buffer.len() {
                        let dst_span = &mut buffer[start_idx..start_idx + span_len];
                        blend_solid_span(dst_span, color);
                    }
                }
            }
            180 => {
                let draw_len = (end_x - start_x) as usize;
                let lh = logical_h as usize;
                for py in start_y..end_y {
                    let py_phys = lh - 1 - py as usize;
                    let start_idx = py_phys * pw + (pw - end_x as usize);
                    if start_idx + draw_len <= buffer.len() {
                        for (i, px) in (start_x..end_x).rev().enumerate() {
                            let src_color = span_colors[(px - rect_x) as usize];
                            blend_pixel(&mut buffer[start_idx + i], src_color);
                        }
                    }
                }
            }
            _ => {
                let draw_w = (end_x - start_x) as usize;
                let x_offset = (start_x - rect_x) as usize;
                for py in start_y..end_y {
                    let start_idx = (py as usize * pw) + start_x as usize;
                    if start_idx + draw_w <= buffer.len() {
                        for x_idx in 0..draw_w {
                            let src_color = span_colors[x_offset + x_idx];
                            blend_pixel(&mut buffer[start_idx + x_idx], src_color);
                        }
                    }
                }
            }
        }
    } else {
        let is_reverse = (angle_normalized - 0.0).abs() < 45.0 || (angle_normalized - 360.0).abs() < 45.0;
        let mut span_colors = Vec::with_capacity(rect_h as usize);
        for dy in 0..rect_h {
            let t = if is_reverse {
                1.0 - (dy as f32 / rect_h as f32)
            } else {
                dy as f32 / rect_h as f32
            };
            let col = sample_gradient(&gradient.stops, t);
            span_colors.push(pack_color(col, swap_rb));
        }

        match rotation {
            90 => {
                let span_len = (end_y - start_y) as usize;
                for px in start_x..end_x {
                    let start_idx = (px as usize * pw) + (pw - end_y as usize);
                    if start_idx + span_len <= buffer.len() {
                        for (i, py) in (start_y..end_y).rev().enumerate() {
                            let src_color = span_colors[(py - rect_y) as usize];
                            blend_pixel(&mut buffer[start_idx + i], src_color);
                        }
                    }
                }
            }
            270 => {
                let span_len = (end_y - start_y) as usize;
                let lw = logical_w as usize;
                for px in start_x..end_x {
                    let py_phys = lw - 1 - px as usize;
                    let start_idx = py_phys * pw + start_y as usize;
                    if start_idx + span_len <= buffer.len() {
                        for (i, py) in (start_y..end_y).enumerate() {
                            let src_color = span_colors[(py - rect_y) as usize];
                            blend_pixel(&mut buffer[start_idx + i], src_color);
                        }
                    }
                }
            }
            180 => {
                let draw_w = (end_x - start_x) as usize;
                let lh = logical_h as usize;
                for py in start_y..end_y {
                    let color = span_colors[(py - rect_y) as usize];
                    let py_phys = lh - 1 - py as usize;
                    let start_idx = py_phys * pw + (pw - end_x as usize);
                    if start_idx + draw_w <= buffer.len() {
                        let dst_span = &mut buffer[start_idx..start_idx + draw_w];
                        blend_solid_span(dst_span, color);
                    }
                }
            }
            _ => {
                let draw_w = (end_x - start_x) as usize;
                for py in start_y..end_y {
                    let color = span_colors[(py - rect_y) as usize];
                    let start_idx = (py as usize * pw) + start_x as usize;
                    if start_idx + draw_w <= buffer.len() {
                        let dst_span = &mut buffer[start_idx..start_idx + draw_w];
                        blend_solid_span(dst_span, color);
                    }
                }
            }
        }
    }
}

/// Per-pixel evaluator for radial and conic gradients over one box.
struct Shader<'a> {
    stops: &'a [(xerune::Color, f32)],
    cx: f32,
    cy: f32,
    kind: ShaderKind,
    repeating: bool,
}

enum ShaderKind {
    /// Unit direction divided by the gradient line length.
    Linear { dir_x: f32, dir_y: f32, dt: f32 },
    /// Inverse radii of the ending ellipse.
    Radial { inv_rx: f32, inv_ry: f32 },
    /// Start angle in turns.
    Conic { from: f32 },
}

impl<'a> Shader<'a> {
    fn new(gradient: &'a xerune::Gradient, x: f32, y: f32, w: f32, h: f32) -> Option<Self> {
        use xerune::{Gradient, RadialExtent, RadialShape};
        match gradient {
            Gradient::Linear(g) => {
                // CSS: 0deg points up, 90deg right; the gradient line is
                // long enough for the corners to hit the first/last stop.
                let rad = g.angle.to_radians();
                let (sin, cos) = (fast_sin(rad), fast_sin(rad + core::f32::consts::FRAC_PI_2));
                let len = (w * sin).abs() + (h * cos).abs();
                let inv = 1.0 / len.max(1.0);
                Some(Self {
                    stops: &g.stops,
                    cx: x + w * 0.5,
                    cy: y + h * 0.5,
                    kind: ShaderKind::Linear { dir_x: sin * inv, dir_y: -cos * inv, dt: (sin.abs() + cos.abs()) * inv },
                    repeating: false,
                })
            }
            Gradient::Radial(g) => {
                let cx = x + g.center.0 * w;
                let cy = y + g.center.1 * h;
                // Distances from the center to the box sides.
                let (dx_min, dx_max) = sorted(g.center.0 * w, (1.0 - g.center.0) * w);
                let (dy_min, dy_max) = sorted(g.center.1 * h, (1.0 - g.center.1) * h);
                let (rx, ry) = match (g.shape, g.extent) {
                    (RadialShape::Circle, RadialExtent::ClosestSide) => (dx_min.min(dy_min), dx_min.min(dy_min)),
                    (RadialShape::Circle, RadialExtent::FarthestSide) => (dx_max.max(dy_max), dx_max.max(dy_max)),
                    (RadialShape::Circle, RadialExtent::ClosestCorner) => {
                        let r = (dx_min * dx_min + dy_min * dy_min).sqrt();
                        (r, r)
                    }
                    (RadialShape::Circle, RadialExtent::FarthestCorner) => {
                        let r = (dx_max * dx_max + dy_max * dy_max).sqrt();
                        (r, r)
                    }
                    (RadialShape::Ellipse, RadialExtent::ClosestSide) => (dx_min, dy_min),
                    (RadialShape::Ellipse, RadialExtent::FarthestSide) => (dx_max, dy_max),
                    // Ellipses through a corner keep the side ratio: scale by sqrt(2).
                    (RadialShape::Ellipse, RadialExtent::ClosestCorner) => (dx_min * SQRT_2, dy_min * SQRT_2),
                    (RadialShape::Ellipse, RadialExtent::FarthestCorner) => (dx_max * SQRT_2, dy_max * SQRT_2),
                };
                Some(Self {
                    stops: &g.stops,
                    cx,
                    cy,
                    kind: ShaderKind::Radial { inv_rx: 1.0 / rx.max(0.5), inv_ry: 1.0 / ry.max(0.5) },
                    repeating: g.repeating,
                })
            }
            Gradient::Conic(g) => Some(Self {
                stops: &g.stops,
                cx: x + g.center.0 * w,
                cy: y + g.center.1 * h,
                kind: ShaderKind::Conic { from: g.from / 360.0 },
                repeating: g.repeating,
            }),
        }
    }

    /// Gradient position at pixel center (`px`, `py`) and the pixel's
    /// footprint along the gradient, both in stop units.
    #[inline]
    fn position(&self, px: f32, py: f32) -> (f32, f32) {
        let dx = px - self.cx;
        let dy = py - self.cy;
        match self.kind {
            ShaderKind::Linear { dir_x, dir_y, dt } => (dx * dir_x + dy * dir_y + 0.5, dt),
            ShaderKind::Radial { inv_rx, inv_ry } => {
                let u = dx * inv_rx;
                let v = dy * inv_ry;
                ((u * u + v * v).sqrt(), inv_rx.max(inv_ry))
            }
            ShaderKind::Conic { from } => {
                // Clockwise from 12 o'clock, in turns.
                let turn = fast_atan2(dx, -dy) * (1.0 / TAU);
                let t = fract(turn - from);
                let r = (dx * dx + dy * dy).sqrt().max(0.5);
                (t, 1.0 / (TAU * r))
            }
        }
    }

    /// Map `t` into the stop range for repeating gradients.
    #[inline]
    fn wrap(&self, t: f32) -> f32 {
        if !self.repeating {
            return t;
        }
        let start = self.stops[0].1;
        let period = self.stops[self.stops.len() - 1].1 - start;
        if period <= 0.0 {
            return t;
        }
        start + fract((t - start) / period) * period
    }

    /// Color at pixel center, box-filtered across the pixel's footprint when
    /// a hard edge (a stop, the wrap seam, or the conic start line) is
    /// inside it.
    fn color_at(&self, px: f32, py: f32) -> xerune::Color {
        let (t, dt) = self.position(px, py);
        let lo = t - dt * 0.5;
        let hi = t + dt * 0.5;
        let near_seam = matches!(self.kind, ShaderKind::Conic { .. }) && (lo < 0.0 || hi > 1.0);
        let near_stop = self.repeating
            || near_seam
            || self.stops.iter().any(|s| s.1 > lo && s.1 < hi);
        if !near_stop {
            return sample_gradient(self.stops, t);
        }

        const N: usize = 4;
        let mut acc = [0.0f32; 4];
        for k in 0..N {
            let mut ts = lo + dt * (k as f32 + 0.5) / N as f32;
            if matches!(self.kind, ShaderKind::Conic { .. }) {
                ts = fract(ts);
            }
            let c = sample_gradient(self.stops, self.wrap(ts));
            let a = c.a as f32;
            acc[0] += c.r as f32 * a;
            acc[1] += c.g as f32 * a;
            acc[2] += c.b as f32 * a;
            acc[3] += a;
        }
        if acc[3] <= 0.0 {
            return xerune::Color::new(0, 0, 0, 0);
        }
        let inv = 1.0 / acc[3];
        xerune::Color::new(
            (acc[0] * inv + 0.5) as u8,
            (acc[1] * inv + 0.5) as u8,
            (acc[2] * inv + 0.5) as u8,
            (acc[3] / N as f32 + 0.5) as u8,
        )
    }
}

const TAU: f32 = core::f32::consts::TAU;
const SQRT_2: f32 = core::f32::consts::SQRT_2;

#[inline]
fn sorted(a: f32, b: f32) -> (f32, f32) {
    if a < b { (a, b) } else { (b, a) }
}

#[inline]
fn fract(x: f32) -> f32 {
    let f = x - (x as i32) as f32;
    if f < 0.0 { f + 1.0 } else { f }
}

/// sin via Bhaskara-style parabola refinement (max error ~1e-3), without libm.
#[inline]
fn fast_sin(x: f32) -> f32 {
    use core::f32::consts::PI;
    // Wrap to [-pi, pi].
    let x = x - TAU * ((x + PI) / TAU - fract((x + PI) / TAU));
    let y = 4.0 / PI * x - 4.0 / (PI * PI) * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

/// atan2 via a minimax polynomial (max error ~1e-5 rad), in [-pi, pi].
/// Avoids libm so it works without `std`.
#[inline]
fn fast_atan2(y: f32, x: f32) -> f32 {
    let ax = x.abs();
    let ay = y.abs();
    let mx = ax.max(ay);
    if mx == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / mx;
    let s = a * a;
    let mut r = ((-0.046_496_475 * s + 0.159_314_22) * s - 0.327_622_76) * s * a + a;
    if ay > ax {
        r = core::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = core::f32::consts::PI - r;
    }
    if y < 0.0 { -r } else { r }
}

/// Fill a (rounded) box with any gradient, evaluated per pixel. Used for
/// radial/conic gradients and for linear ones the span path can't handle
/// (rounded corners, diagonal angles).
pub fn draw_shaded_rounded_rect(
    buffer: &mut [u32],
    logical_w: u32,
    logical_h: u32,
    physical_w: u32,
    rect: xerune::Rect,
    radius: f32,
    gradient: &xerune::Gradient,
    swap_rb: bool,
    clip_rect: Option<xerune::Rect>,
    rotation: u32,
) {
    let Some(shader) = Shader::new(gradient, rect.x, rect.y, rect.width, rect.height) else {
        return;
    };
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return;
    }

    let (clip_x1, clip_y1, clip_x2, clip_y2) = match clip_rect {
        Some(cr) => (cr.x.max(0.0), cr.y.max(0.0), (cr.x + cr.width).min(logical_w as f32), (cr.y + cr.height).min(logical_h as f32)),
        None => (0.0, 0.0, logical_w as f32, logical_h as f32),
    };
    // Both are >= 0 here, so truncation floors.
    let start_x = rect.x.max(clip_x1) as i32;
    let start_y = rect.y.max(clip_y1) as i32;
    let end_x = (rect.x + rect.width).min(clip_x2).ceil() as i32;
    let end_y = (rect.y + rect.height).min(clip_y2).ceil() as i32;

    // Signed distance to the rounded box, for anti-aliased edges.
    let half_w = rect.width * 0.5;
    let half_h = rect.height * 0.5;
    let r = radius.min(half_w).min(half_h).max(0.0);
    let box_cx = rect.x + half_w;
    let box_cy = rect.y + half_h;
    let inner_w = half_w - r;
    let inner_h = half_h - r;

    for py in start_y..end_y {
        let fy = py as f32 + 0.5;
        let qy = (fy - box_cy).abs() - inner_h;
        for px in start_x..end_x {
            let fx = px as f32 + 0.5;
            let qx = (fx - box_cx).abs() - inner_w;
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            let dist = (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r;
            let coverage = (0.5 - dist).clamp(0.0, 1.0);
            if coverage <= 0.0 {
                continue;
            }

            let mut color = shader.color_at(fx, fy);
            if color.a == 0 {
                continue;
            }
            if coverage < 1.0 {
                color.a = (color.a as f32 * coverage + 0.5) as u8;
            }
            let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);
            if idx < buffer.len() {
                blend_pixel(&mut buffer[idx], pack_color(color, swap_rb));
            }
        }
    }
}
