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

/// Entries in the finest level of a gradient's color table. Fine enough that
/// a hard stop lands within a fraction of a pixel on panel-sized boxes.
const LUT_N: usize = 2048;
/// Coarsest level has `LUT_N >> LUT_LEVELS - 1` entries.
const LUT_LEVELS: usize = 9;

#[inline]
fn premul(c: xerune::Color) -> [f32; 4] {
    let a = c.a as f32;
    [c.r as f32 * a, c.g as f32 * a, c.b as f32 * a, a]
}

/// Packed, ready-to-blend colors along the gradient line, mip-mapped: level
/// `k` averages `2^k` finest cells. A pixel reads the level whose cell
/// matches its footprint, which anti-aliases hard stops for one lookup.
struct ColorLut {
    levels: Vec<Vec<u32>>,
    start: f32,
    /// Finest cells per stop unit.
    inv_cell: f32,
    /// Cells per stop unit at each level.
    scale: [f32; LUT_LEVELS],
    /// floor(log2(finest cells / (2 pi))), for conic level selection.
    log2_cells_per_turn: i32,
    /// `opaque_before[i]`: finest cells before `i` that aren't transparent.
    opaque_before: Vec<u32>,
    /// `run_end[i]`: first finest cell after `i` with a different color.
    run_end: Vec<u32>,
    /// Repeat outside the domain instead of clamping (conic seam,
    /// `repeating-` gradients).
    periodic: bool,
}

#[cfg(feature = "std")]
type LutRef = std::rc::Rc<ColorLut>;
#[cfg(not(feature = "std"))]
type LutRef = ColorLut;

/// Built tables, keyed by stops and domain. The UI tree is rebuilt on every
/// change, so the same gradient arrives as a fresh value each frame; most
/// (dial faces, tick rings) never change and should not be rebuilt.
#[cfg(feature = "std")]
const LUT_CACHE_ENTRIES: usize = 64;

#[cfg(feature = "std")]
thread_local! {
    static LUT_CACHE: std::cell::RefCell<std::collections::HashMap<u64, std::rc::Rc<ColorLut>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

impl ColorLut {
    #[cfg(feature = "std")]
    fn cached(stops: &[(xerune::Color, f32)], start: f32, end: f32, periodic: bool, swap_rb: bool) -> LutRef {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (c, p) in stops {
            (c.r, c.g, c.b, c.a, p.to_bits()).hash(&mut h);
        }
        (start.to_bits(), end.to_bits(), periodic, swap_rb).hash(&mut h);
        let key = h.finish();
        LUT_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(lut) = cache.get(&key) {
                return lut.clone();
            }
            if cache.len() >= LUT_CACHE_ENTRIES {
                cache.clear();
            }
            let lut = std::rc::Rc::new(Self::new(stops, start, end, periodic, swap_rb));
            cache.insert(key, lut.clone());
            lut
        })
    }

    #[cfg(not(feature = "std"))]
    fn cached(stops: &[(xerune::Color, f32)], start: f32, end: f32, periodic: bool, swap_rb: bool) -> LutRef {
        Self::new(stops, start, end, periodic, swap_rb)
    }

    fn new(stops: &[(xerune::Color, f32)], start: f32, end: f32, periodic: bool, swap_rb: bool) -> Self {
        let span = (end - start).max(1e-6);
        let step = span / LUT_N as f32;

        // Running integral of premultiplied color at cell boundaries.
        let mut cum = Vec::with_capacity(LUT_N + 1);
        let mut acc = [0.0f32; 4];
        cum.push(acc);
        // Sample positions only increase, so walk the stops instead of
        // searching them for every cell.
        let mut next = 0;
        for i in 0..LUT_N {
            let t = start + (i as f32 + 0.5) * step;
            while next < stops.len() && stops[next].1 <= t {
                next += 1;
            }
            let lo = next.saturating_sub(1);
            let c = premul(sample_gradient(&stops[lo..(next + 1).min(stops.len())], t));
            for k in 0..4 {
                acc[k] += c[k];
            }
            cum.push(acc);
        }

        let levels: Vec<Vec<u32>> = (0..LUT_LEVELS)
            .map(|level| {
                let width = 1usize << level;
                (0..LUT_N / width)
                    .map(|i| {
                        let (lo, hi) = (cum[i * width], cum[(i + 1) * width]);
                        let a = (hi[3] - lo[3]) / width as f32;
                        if a < 0.5 {
                            return 0;
                        }
                        let inv = 1.0 / (a * width as f32);
                        let ch = |k: usize| ((hi[k] - lo[k]) * inv + 0.5).min(255.0) as u8;
                        pack_color(xerune::Color::new(ch(0), ch(1), ch(2), (a + 0.5).min(255.0) as u8), swap_rb)
                    })
                    .collect()
            })
            .collect();

        let inv_cell = 1.0 / step;
        let scale = core::array::from_fn(|level| inv_cell / (1u32 << level) as f32);
        let log2_cells_per_turn = ((inv_cell / TAU).max(1.0).to_bits() >> 23) as i32 - 127;

        let finest = &levels[0];
        let mut opaque_before = Vec::with_capacity(LUT_N + 1);
        let mut count = 0u32;
        opaque_before.push(0);
        for &c in finest {
            count += (c >> 24 != 0) as u32;
            opaque_before.push(count);
        }
        let mut run_end: Vec<u32> = (0..LUT_N).map(|_| LUT_N as u32).collect();
        for i in (0..LUT_N - 1).rev() {
            run_end[i] = if finest[i + 1] == finest[i] { run_end[i + 1] } else { (i + 1) as u32 };
        }

        Self { levels, start, inv_cell, scale, log2_cells_per_turn, opaque_before, run_end, periodic }
    }

    /// Mip level for a footprint of `dt` stop units.
    #[inline(always)]
    fn level_for(&self, dt: f32) -> usize {
        let cells = dt * self.inv_cell;
        if cells < 2.0 {
            return 0;
        }
        // floor(log2(cells)) from the float exponent.
        let log2 = ((cells.to_bits() >> 23) & 0xff) as usize - 127;
        log2.min(LUT_LEVELS - 1)
    }

    /// What a periodic table holds over `[lo, hi]` (stop units, `lo <= hi`):
    /// `Some(0)` if all transparent, `Some(color)` if one color throughout,
    /// `None` if it varies.
    fn range_color(&self, lo: f32, hi: f32) -> Option<u32> {
        let n = LUT_N as i64;
        let floor = |x: f32| {
            let i = x as i64;
            if (i as f32) > x { i - 1 } else { i }
        };
        let i0 = floor((lo - self.start) * self.inv_cell);
        let i1 = floor((hi - self.start) * self.inv_cell);
        if i1 - i0 >= n {
            return self.segment_color(0, LUT_N - 1);
        }
        let s = i0.rem_euclid(n) as usize;
        let e = s + (i1 - i0) as usize;
        if e < LUT_N {
            return self.segment_color(s, e);
        }
        let a = self.segment_color(s, LUT_N - 1)?;
        let b = self.segment_color(0, e - LUT_N)?;
        (a == b).then_some(a)
    }

    /// Like [`ColorLut::range_color`] for finest cells `s..=e`.
    fn segment_color(&self, s: usize, e: usize) -> Option<u32> {
        if self.opaque_before[e + 1] == self.opaque_before[s] {
            return Some(0);
        }
        (self.run_end[s] as usize > e).then(|| self.levels[0][s])
    }

    #[inline(always)]
    fn get(&self, t: f32, level: usize) -> u32 {
        let table = &self.levels[level];
        // Table lengths are powers of two: wrap with a mask, not a divide.
        let mask = (table.len() - 1) as i32;
        // Floor by biasing into positive range so truncation rounds down;
        // indices this negative never occur.
        let i = ((t - self.start) * self.scale[level] + 65536.0) as i32 - 65536;
        let i = if self.periodic { i & mask } else { i.clamp(0, mask) };
        table[i as usize]
    }
}

/// Per-pixel evaluator for a gradient over one box.
struct Shader {
    colors: LutRef,
    cx: f32,
    cy: f32,
    kind: ShaderKind,
}

enum ShaderKind {
    /// Unit direction divided by the gradient line length.
    Linear { dir_x: f32, dir_y: f32, dt: f32 },
    /// Inverse radii of the ending ellipse.
    Radial { inv_rx: f32, inv_ry: f32 },
    /// Start angle in turns.
    Conic { from: f32 },
}

impl Shader {
    fn new(gradient: &xerune::Gradient, x: f32, y: f32, w: f32, h: f32, swap_rb: bool) -> Option<Self> {
        use xerune::{Gradient, RadialExtent, RadialShape};
        let stops = match gradient {
            Gradient::Linear(g) => &g.stops,
            Gradient::Radial(g) => &g.stops,
            Gradient::Conic(g) => &g.stops,
        };
        let (first, last) = (stops.first()?.1, stops.last()?.1);
        let repeating = match gradient {
            Gradient::Linear(_) => false,
            Gradient::Radial(g) => g.repeating,
            Gradient::Conic(g) => g.repeating,
        };
        let colors = match gradient {
            _ if repeating => ColorLut::cached(stops, first, last, true, swap_rb),
            // A full turn wraps around: the seam is just another edge.
            Gradient::Conic(_) => ColorLut::cached(stops, 0.0, 1.0, true, swap_rb),
            _ => ColorLut::cached(stops, first.min(0.0), last.max(1.0), false, swap_rb),
        };

        match gradient {
            Gradient::Linear(g) => {
                // CSS: 0deg points up, 90deg right; the gradient line is
                // long enough for the corners to hit the first/last stop.
                let rad = g.angle.to_radians();
                let (sin, cos) = (fast_sin(rad), fast_sin(rad + core::f32::consts::FRAC_PI_2));
                let len = (w * sin).abs() + (h * cos).abs();
                let inv = 1.0 / len.max(1.0);
                Some(Self {
                    colors,
                    cx: x + w * 0.5,
                    cy: y + h * 0.5,
                    kind: ShaderKind::Linear { dir_x: sin * inv, dir_y: -cos * inv, dt: (sin.abs() + cos.abs()) * inv },
                })
            }
            Gradient::Radial(g) => {
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
                    colors,
                    cx: x + g.center.0 * w,
                    cy: y + g.center.1 * h,
                    kind: ShaderKind::Radial { inv_rx: 1.0 / rx.max(0.5), inv_ry: 1.0 / ry.max(0.5) },
                })
            }
            Gradient::Conic(g) => Some(Self {
                colors,
                cx: x + g.center.0 * w,
                cy: y + g.center.1 * h,
                kind: ShaderKind::Conic { from: g.from / 360.0 },
            }),
        }
    }

    /// Mip level for linear and radial gradients, whose pixel footprint is
    /// the same everywhere; conic ones pick per pixel.
    fn fixed_level(&self) -> usize {
        match self.kind {
            ShaderKind::Linear { dt, .. } => self.colors.level_for(dt),
            ShaderKind::Radial { inv_rx, inv_ry } => self.colors.level_for(inv_rx.max(inv_ry)),
            ShaderKind::Conic { .. } => 0,
        }
    }

    /// Mip level for a conic pixel at squared distance `r2` from the center:
    /// its footprint is 1 / (2 pi r) turns. Uses the float exponent of `r2`
    /// as 2 * log2(r) to avoid a square root per pixel.
    #[inline(always)]
    fn conic_level(&self, r2: f32) -> usize {
        let log2_r2 = ((r2.max(0.25).to_bits() >> 23) & 0xff) as i32 - 127;
        let log2_cells = self.colors.log2_cells_per_turn - (log2_r2 + 1) / 2;
        log2_cells.clamp(0, LUT_LEVELS as i32 - 1) as usize
    }

    /// For linear gradients: the table position at pixel `x` of row `fy`
    /// and its per-pixel increment, both in cells of `level`.
    #[inline]
    fn linear_row(&self, x: i32, fy: f32, level: usize) -> Option<(f32, f32)> {
        let ShaderKind::Linear { dir_x, dir_y, .. } = self.kind else {
            return None;
        };
        if self.colors.periodic {
            return None;
        }
        let scale = self.colors.scale[level];
        let t = (x as f32 + 0.5 - self.cx) * dir_x + (fy - self.cy) * dir_y + 0.5;
        Some(((t - self.colors.start) * scale, dir_x * scale))
    }

    /// Packed color at pixel center (`px`, `py`), averaged over the pixel's
    /// footprint along the gradient so hard stops come out anti-aliased.
    #[inline(always)]
    fn color_at(&self, px: f32, py: f32, level: usize) -> u32 {
        let dx = px - self.cx;
        let dy = py - self.cy;
        match self.kind {
            ShaderKind::Linear { dir_x, dir_y, .. } => self.colors.get(dx * dir_x + dy * dir_y + 0.5, level),
            ShaderKind::Radial { inv_rx, inv_ry } => {
                let u = dx * inv_rx;
                let v = dy * inv_ry;
                self.colors.get((u * u + v * v).sqrt(), level)
            }
            ShaderKind::Conic { from } => {
                // Clockwise from 12 o'clock, in turns.
                let turn = fast_atan2(dx, -dy) * (1.0 / TAU);
                self.colors.get(turn - from, self.conic_level(dx * dx + dy * dy))
            }
        }
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
#[inline(always)]
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
    let Some(shader) = Shader::new(gradient, rect.x, rect.y, rect.width, rect.height, swap_rb) else {
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
    let level = shader.fixed_level();


    // Destination step between horizontally adjacent logical pixels.
    let stride: isize = match rotation {
        90 => physical_w as isize,
        180 => -1,
        270 => -(physical_w as isize),
        _ => 1,
    };

    if let ShaderKind::Conic { from } = shader.kind {
        draw_conic_blocks(
            buffer, &shader, from, (start_x, start_y, end_x, end_y), (box_cx, box_cy, inner_w, inner_h, r),
            (logical_w, logical_h, physical_w, rotation),
        );
        return;
    }

    let edge_pixel = |buffer: &mut [u32], px: i32, py: i32, fy: f32, qy: f32| {
        let fx = px as f32 + 0.5;
        let qx = (fx - box_cx).abs() - inner_w;
        let dist = if qx <= 0.0 && qy <= 0.0 {
            qx.max(qy) - r
        } else {
            let ox = qx.max(0.0);
            let oy = qy.max(0.0);
            (ox * ox + oy * oy).sqrt() - r
        };
        let coverage = (0.5 - dist).clamp(0.0, 1.0);
        if coverage <= 0.0 {
            return;
        }
        let mut color = shader.color_at(fx, fy, level);
        if color >> 24 == 0 {
            return;
        }
        if coverage < 1.0 {
            let a = ((color >> 24) as f32 * coverage + 0.5) as u32;
            color = (color & 0x00ff_ffff) | (a << 24);
        }
        let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);
        if idx < buffer.len() {
            blend_pixel(&mut buffer[idx], color);
        }
    };

    for py in start_y..end_y {
        let fy = py as f32 + 0.5;
        let qy = (fy - box_cy).abs() - inner_h;

        // Pixels with any coverage, and the fully covered span between
        // them that skips the edge math (dist <= -0.5).
        let (Some(outer), full) = (row_half_width(qy, r + 0.5), row_half_width(qy, r - 0.5)) else {
            continue;
        };
        let row_lo = ((box_cx - inner_w - outer - 0.5) as i32).max(start_x);
        let row_hi = ((box_cx + inner_w + outer + 0.5).ceil() as i32).min(end_x);
        let (span_lo, span_hi) = match full {
            Some(half) => {
                let lo = ((box_cx - inner_w - half - 0.5).ceil() as i32).max(row_lo);
                let hi = ((box_cx + inner_w + half - 0.5) as i32 + 1).min(row_hi);
                if lo < hi { (lo, hi) } else { (row_hi, row_hi) }
            }
            None => (row_hi, row_hi),
        };

        for px in row_lo..span_lo {
            edge_pixel(buffer, px, py, fy, qy);
        }
        if span_lo < span_hi {
            let mut idx = calc_pixel_index(span_lo, py, rotation, physical_w, logical_w, logical_h) as isize;
            if let ShaderKind::Radial { inv_rx, inv_ry } = shader.kind {
                // Radial: step the x term, one square root per pixel.
                let table = &shader.colors.levels[level];
                let max = (table.len() - 1) as f32;
                let scale = shader.colors.scale[level];
                let offset = shader.colors.start * scale;
                let v = (fy - shader.cy) * inv_ry;
                let vv = v * v;
                let mut u = (span_lo as f32 + 0.5 - shader.cx) * inv_rx;
                for _ in span_lo..span_hi {
                    let t = (u * u + vv).sqrt();
                    let color = table[(t * scale - offset).clamp(0.0, max) as usize];
                    if let Some(dst) = buffer.get_mut(idx as usize) {
                        blend_pixel(dst, color);
                    }
                    u += inv_rx;
                    idx += stride;
                }
            } else if let Some((mut u, du)) = shader.linear_row(span_lo, fy, level) {
                // Linear: the table index advances by a constant per pixel.
                let table = &shader.colors.levels[level];
                let max = (table.len() - 1) as f32;
                for _ in span_lo..span_hi {
                    let color = table[u.clamp(0.0, max) as usize];
                    if let Some(dst) = buffer.get_mut(idx as usize) {
                        blend_pixel(dst, color);
                    }
                    u += du;
                    idx += stride;
                }
            } else {
                for px in span_lo..span_hi {
                    let color = shader.color_at(px as f32 + 0.5, fy, level);
                    if let Some(dst) = buffer.get_mut(idx as usize) {
                        blend_pixel(dst, color);
                    }
                    idx += stride;
                }
            }
        }
        for px in span_hi.max(row_lo)..row_hi {
            edge_pixel(buffer, px, py, fy, qy);
        }
    }
}

/// Half-width beyond the inner box, at row offset `qy` past the inner box's
/// top/bottom, of the region within `reach` of the inner box: the corner
/// circle of radius `reach`. `None` if the row misses it.
#[inline(always)]
fn row_half_width(qy: f32, reach: f32) -> Option<f32> {
    if qy <= 0.0 {
        // Beside the inner box; a negative reach shrinks the band.
        (qy <= reach).then_some(reach)
    } else if qy < reach {
        Some((reach * reach - qy * qy).sqrt())
    } else {
        None
    }
}

/// Signed distance from (`fx`, `fy`) to a rounded box given by its center,
/// inner half-size and corner radius.
#[inline(always)]
fn rounded_box_dist(fx: f32, fy: f32, (cx, cy, inner_w, inner_h, r): (f32, f32, f32, f32, f32)) -> f32 {
    let qx = (fx - cx).abs() - inner_w;
    let qy = (fy - cy).abs() - inner_h;
    if qx <= 0.0 && qy <= 0.0 {
        qx.max(qy) - r
    } else {
        let ox = qx.max(0.0);
        let oy = qy.max(0.0);
        (ox * ox + oy * oy).sqrt() - r
    }
}

/// Conic fill in 8x8 blocks. A block's angular range comes from its four
/// corners; if the gradient is transparent there the block is skipped, and
/// if it is one color the block is filled without per-pixel atan2. Gauges
/// and rings are mostly such blocks.
fn draw_conic_blocks(
    buffer: &mut [u32],
    shader: &Shader,
    from: f32,
    (start_x, start_y, end_x, end_y): (i32, i32, i32, i32),
    shape: (f32, f32, f32, f32, f32),
    (logical_w, logical_h, physical_w, rotation): (u32, u32, u32, u32),
) {
    const B: i32 = 8;
    // Half diagonal of a block, plus the anti-aliasing half pixel.
    let block_reach = B as f32 * core::f32::consts::FRAC_1_SQRT_2 + 0.5;
    let turn_at = |x: f32, y: f32| fast_atan2(x - shader.cx, shader.cy - y) * (1.0 / TAU) - from;

    let mut by = start_y;
    while by < end_y {
        let by1 = (by + B).min(end_y);
        let mut bx = start_x;
        while bx < end_x {
            let bx1 = (bx + B).min(end_x);
            let (x0, y0, x1, y1) = (bx as f32, by as f32, bx1 as f32, by1 as f32);

            // Coverage: skip blocks outside the shape, skip edge math inside.
            let center_dist = rounded_box_dist((x0 + x1) * 0.5, (y0 + y1) * 0.5, shape);
            if center_dist > block_reach {
                bx = bx1;
                continue;
            }
            let fully_inside = center_dist < -block_reach;

            // Angular range of the block, unless it is near the center.
            let near_x = shader.cx.clamp(x0, x1) - shader.cx;
            let near_y = shader.cy.clamp(y0, y1) - shader.cy;
            let r_min = (near_x * near_x + near_y * near_y).sqrt();
            let uniform = if r_min > 2.0 {
                let t0 = turn_at(x0, y0);
                let mut lo = 0.0f32;
                let mut hi = 0.0f32;
                for (x, y) in [(x1, y0), (x0, y1), (x1, y1)] {
                    let mut d = turn_at(x, y) - t0;
                    d -= (d + 0.5 + 65536.0) as i32 as f32 - 65536.0; // wrap into [-0.5, 0.5)
                    lo = lo.min(d);
                    hi = hi.max(d);
                }
                // Widen by the largest pixel footprint in the block.
                let margin = 1.0 / (TAU * r_min) + 1.0 / shader.colors.inv_cell;
                shader.colors.range_color(t0 + lo - margin, t0 + hi + margin)
            } else {
                None
            };
            if uniform == Some(0) {
                bx = bx1;
                continue;
            }

            for py in by..by1 {
                let fy = py as f32 + 0.5;
                for px in bx..bx1 {
                    let fx = px as f32 + 0.5;
                    let coverage = if fully_inside { 1.0 } else { (0.5 - rounded_box_dist(fx, fy, shape)).clamp(0.0, 1.0) };
                    if coverage <= 0.0 {
                        continue;
                    }
                    let mut color = match uniform {
                        Some(c) => c,
                        None => shader.color_at(fx, fy, 0),
                    };
                    if color >> 24 == 0 {
                        continue;
                    }
                    if coverage < 1.0 {
                        let a = ((color >> 24) as f32 * coverage + 0.5) as u32;
                        color = (color & 0x00ff_ffff) | (a << 24);
                    }
                    let idx = calc_pixel_index(px, py, rotation, physical_w, logical_w, logical_h);
                    if let Some(dst) = buffer.get_mut(idx) {
                        blend_pixel(dst, color);
                    }
                }
            }
            bx = bx1;
        }
        by = by1;
    }
}
