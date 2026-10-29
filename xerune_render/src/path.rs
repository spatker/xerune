//! Filling paths with anti-aliasing: glyph outlines, and anything else
//! drawn as lines and Bézier curves (icons).
//!
//! A [`Path`] flattens its curves into lines as they arrive, in pixels with
//! y pointing down, after its [`Transform`]. [`Path::fill`] then computes
//! each pixel's coverage exactly from the area the lines enclose in it:
//! every line adds, to the cells of the rows it crosses, its height there
//! split by where it passes through the cell (the part left of the line
//! into that cell, the rest into the next one), and a running sum along
//! the rows turns those into the winding of each pixel, partial at edges.

/// Which areas a path fills where its contours overlap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    /// Wherever the contours wind around at all (as font outlines are).
    NonZero,
    /// Where they wind around an odd number of times.
    EvenOdd,
}

/// An affine transform applied to a path's points: `x' = a x + c y + e`,
/// `y' = b x + d y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Transform {
    pub const IDENTITY: Self = Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// Scales by `sx`, `sy`, then moves by `tx`, `ty`.
    pub fn scale_translate(sx: f32, sy: f32, tx: f32, ty: f32) -> Self {
        Self { a: sx, b: 0.0, c: 0.0, d: sy, e: tx, f: ty }
    }

    fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }
}

/// How far a flattened curve may stray from the real one, in pixels.
const TOLERANCE: f32 = 0.1;

/// The most lines a curve is flattened into.
const MAX_CURVE_LINES: u32 = 64;

/// Lines and curves to fill, already flattened and transformed.
#[derive(Clone, Debug)]
pub struct Path {
    transform: Transform,
    /// (x0, y0, x1, y1) in pixels.
    lines: Vec<[f32; 4]>,
    /// The current contour's start and the current point, transformed.
    start: (f32, f32),
    current: (f32, f32),
    /// (min x, min y, max x, max y) of every point added.
    bounds: [f32; 4],
}

impl Default for Path {
    fn default() -> Self {
        Self::new()
    }
}

impl Path {
    pub fn new() -> Self {
        Self::with_transform(Transform::IDENTITY)
    }

    /// A path whose points are mapped by `transform` as they are added.
    pub fn with_transform(transform: Transform) -> Self {
        Self {
            transform,
            // A glyph has a few dozen lines.
            lines: Vec::with_capacity(64),
            start: (0.0, 0.0),
            current: (0.0, 0.0),
            bounds: [f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY],
        }
    }

    fn include(&mut self, (x, y): (f32, f32)) {
        self.bounds = [self.bounds[0].min(x), self.bounds[1].min(y), self.bounds[2].max(x), self.bounds[3].max(y)];
    }

    fn line_to_point(&mut self, p: (f32, f32)) {
        self.include(p);
        if p != self.current {
            self.lines.push([self.current.0, self.current.1, p.0, p.1]);
        }
        self.current = p;
    }

    /// Starts a contour at (x, y), closing the one before.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        let p = self.transform.apply(x, y);
        self.include(p);
        (self.start, self.current) = (p, p);
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        let p = self.transform.apply(x, y);
        self.line_to_point(p);
    }

    /// A quadratic Bézier curve through control point (cx, cy) to (x, y).
    pub fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.current, self.transform.apply(cx, cy), self.transform.apply(x, y));
        // The curve strays at most a quarter of |p0 - 2 p1 + p2| from its
        // chord, and n lines cut that by n².
        let deviation = length(p0.0 - 2.0 * p1.0 + p2.0, p0.1 - 2.0 * p1.1 + p2.1) / 4.0;
        let n = lines_for(deviation);
        for i in 1..n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c) = (u * u, 2.0 * u * t, t * t);
            self.line_to_point((a * p0.0 + b * p1.0 + c * p2.0, a * p0.1 + b * p1.1 + c * p2.1));
        }
        self.line_to_point(p2);
    }

    /// A cubic Bézier curve through control points (c1x, c1y) and
    /// (c2x, c2y) to (x, y).
    pub fn cubic_to(&mut self, c1x: f32, c1y: f32, c2x: f32, c2y: f32, x: f32, y: f32) {
        let p0 = self.current;
        let (p1, p2, p3) = (self.transform.apply(c1x, c1y), self.transform.apply(c2x, c2y), self.transform.apply(x, y));
        // Strays at most 3/4 of the larger second difference from its chord.
        let second = |a: (f32, f32), b: (f32, f32), c: (f32, f32)| length(a.0 - 2.0 * b.0 + c.0, a.1 - 2.0 * b.1 + c.1);
        let deviation = 0.75 * second(p0, p1, p2).max(second(p1, p2, p3));
        let n = lines_for(deviation);
        for i in 1..n {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.line_to_point((
                a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0,
                a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1,
            ));
        }
        self.line_to_point(p3);
    }

    /// Closes the current contour with a line back to its start. Filling
    /// closes every contour anyway.
    pub fn close(&mut self) {
        if self.current != self.start {
            self.lines.push([self.current.0, self.current.1, self.start.0, self.start.1]);
            self.current = self.start;
        }
    }

    /// The whole pixels the path touches, as (left, top, right, bottom);
    /// None while it is empty.
    fn pixel_bounds(&self) -> Option<(i32, i32, i32, i32)> {
        let [x0, y0, x1, y1] = self.bounds;
        (x0 <= x1 && y0 <= y1).then(|| (x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32))
    }

    /// The coverage of every pixel the path touches; None if it encloses
    /// none.
    pub fn fill(&mut self, rule: FillRule) -> Option<Mask> {
        self.close();
        let (left, top, right, bottom) = self.pixel_bounds()?;
        let (width, height) = ((right - left) as usize, (bottom - top) as usize);
        if width == 0 || height == 0 || self.lines.is_empty() {
            return None;
        }
        // One cell more than the pixels: the last column's lines hand the
        // rest of their area to the cell after it, the next row's first.
        let mut cells = vec![0.0f32; width * height + 1];
        for &[x0, y0, x1, y1] in &self.lines {
            add_line(&mut cells, width, height, x0 - left as f32, y0 - top as f32, x1 - left as f32, y1 - top as f32);
        }
        let cells = &cells[..width * height];
        let coverage = match rule {
            FillRule::NonZero => coverage(cells, |winding| winding.abs().min(1.0)),
            FillRule::EvenOdd => coverage(cells, |winding| {
                let w = winding.abs() % 2.0;
                if w > 1.0 { 2.0 - w } else { w }
            }),
        };
        Some(Mask { left, top, width: width as u32, height: height as u32, coverage })
    }
}

/// The coverage of every cell from the running sum of `cells`, through
/// `rule` (winding to 0..=1).
fn coverage(cells: &[f32], rule: impl Fn(f32) -> f32) -> Vec<u8> {
    let mut winding = 0.0f32;
    cells
        .iter()
        .map(|&cell| {
            winding += cell;
            (rule(winding) * 255.0 + 0.5) as u8
        })
        .collect()
}

/// The length of (x, y): `hypot` is careful about overflow and much slower.
fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

/// How many lines keep a curve straying `deviation` pixels from its chord
/// within [`TOLERANCE`].
fn lines_for(deviation: f32) -> u32 {
    ((deviation / TOLERANCE).sqrt().ceil() as u32).clamp(1, MAX_CURVE_LINES)
}

/// Adds the line from (x0, y0) to (x1, y1), in the cells' pixels, to
/// `cells`: in every row it crosses, the signed height it spans there,
/// split between the cells it passes through.
#[allow(clippy::too_many_arguments)]
fn add_line(cells: &mut [f32], width: usize, height: usize, x0: f32, y0: f32, x1: f32, y1: f32) {
    if y0 == y1 {
        return;
    }
    // Downward lines wind one way, upward ones the other.
    let (sign, (xa, ya), (xb, yb)) = if y0 < y1 { (1.0, (x0, y0), (x1, y1)) } else { (-1.0, (x1, y1), (x0, y0)) };
    let dxdy = (xb - xa) / (yb - ya);
    let (top, bottom) = (ya.max(0.0), yb.min(height as f32));
    let (mut x, mut y) = (xa + (top - ya) * dxdy, top);
    // y < bottom <= height keeps the row in the cells.
    let mut row = top as usize;
    while y < bottom {
        let next_y = (row as f32 + 1.0).min(bottom);
        let next_x = x + (next_y - y) * dxdy;
        add_row_span(&mut cells[row * width..], width, x, next_x, sign * (next_y - y));
        (x, y, row) = (next_x, next_y, row + 1);
    }
}

/// Adds a line's span through one row, from x `x0` to `x1` over signed
/// height `dy`, to the row's cells (and the one after them): each cell it
/// passes through gets the part of its height left of the line there, the
/// next cell the rest, so that the running sum is the coverage.
fn add_row_span(cells: &mut [f32], width: usize, x0: f32, x1: f32, dy: f32) {
    let w = width as f32;
    // Left of the cells covers the whole row; right of them, nothing.
    let (lo, hi) = if x0 < x1 { (x0, x1) } else { (x1, x0) };
    let (lo, hi) = (lo.max(0.0).min(w), hi.max(0.0).min(w));
    let mut cell = (lo as usize).min(width - 1);
    let mut cell_end = cell as f32 + 1.0;
    if hi <= cell_end {
        // Within one cell, as steep edges (stems) are.
        let inside = (lo + hi) * 0.5 - (cell_end - 1.0);
        cells[cell] += dy * (1.0 - inside);
        cells[cell + 1] += dy * inside;
        return;
    }
    // The height is spread evenly over the width.
    let dy_per_x = dy / (hi - lo);
    let mut x = lo;
    while x < hi {
        let next = cell_end.min(hi);
        let (part, inside) = (dy_per_x * (next - x), (x + next) * 0.5 - (cell_end - 1.0));
        cells[cell] += part * (1.0 - inside);
        cells[cell + 1] += part * inside;
        (x, cell, cell_end) = (next, cell + 1, cell_end + 1.0);
    }
}

/// A filled path's coverage, 0 to 255 per pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mask {
    /// Where its top-left pixel is.
    pub left: i32,
    pub top: i32,
    pub width: u32,
    pub height: u32,
    /// Row by row, `width` per row.
    pub coverage: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(path: &mut Path, x0: f32, y0: f32, x1: f32, y1: f32) {
        path.move_to(x0, y0);
        path.line_to(x1, y0);
        path.line_to(x1, y1);
        path.line_to(x0, y1);
        path.close();
    }

    impl Mask {
        fn at(&self, x: i32, y: i32) -> u8 {
            self.coverage[(y - self.top) as usize * self.width as usize + (x - self.left) as usize]
        }
    }

    fn total(mask: &Mask) -> f32 {
        mask.coverage.iter().map(|&c| c as f32 / 255.0).sum()
    }

    #[test]
    fn whole_pixels_are_covered_fully() {
        let mut path = Path::new();
        rect(&mut path, 1.0, 2.0, 5.0, 4.0);
        let mask = path.fill(FillRule::NonZero).unwrap();
        assert_eq!((mask.left, mask.top, mask.width, mask.height), (1, 2, 4, 2));
        assert!(mask.coverage.iter().all(|&c| c == 255));
    }

    #[test]
    fn edges_cover_their_share_of_a_pixel() {
        let mut path = Path::new();
        rect(&mut path, 0.5, 0.25, 3.5, 2.0);
        let mask = path.fill(FillRule::NonZero).unwrap();
        // Half of the left and right columns, three quarters of the top row.
        assert_eq!(mask.at(0, 1), 128);
        assert_eq!(mask.at(3, 1), 128);
        assert_eq!(mask.at(1, 0), 191);
        assert_eq!(mask.at(0, 0), 96);
        assert_eq!(mask.at(1, 1), 255);
    }

    #[test]
    fn slanted_edges_cover_the_area_under_them() {
        // A right triangle of 6 x 4 pixels: area 12.
        let mut path = Path::new();
        path.move_to(0.0, 0.0);
        path.line_to(6.0, 4.0);
        path.line_to(0.0, 4.0);
        let mask = path.fill(FillRule::NonZero).unwrap();
        assert!((total(&mask) - 12.0).abs() < 0.05, "{}", total(&mask));
        // The edge enters pixel (3, 2) at its top-left corner and leaves
        // at 2/3 of its right side, below which the triangle is: 2/3 of it.
        assert_eq!(mask.at(3, 2), 170);
    }

    #[test]
    fn curves_enclose_the_area_of_the_shape() {
        // A circle of radius 10 from four cubics (κ = 0.5523).
        let (r, k, c) = (10.0f32, 10.0 * 0.552_284_8, 12.3f32);
        let mut path = Path::new();
        path.move_to(c + r, c);
        path.cubic_to(c + r, c + k, c + k, c + r, c, c + r);
        path.cubic_to(c - k, c + r, c - r, c + k, c - r, c);
        path.cubic_to(c - r, c - k, c - k, c - r, c, c - r);
        path.cubic_to(c + k, c - r, c + r, c - k, c + r, c);
        let mask = path.fill(FillRule::NonZero).unwrap();
        // Flattened, it is a polygon inside the circle: 24 lines, each
        // missing r²/2 (θ - sin θ) of it for its angle θ = π/12.
        let theta = core::f32::consts::PI / 12.0;
        let area = core::f32::consts::PI * r * r - 24.0 * r * r / 2.0 * (theta - theta.sin());
        assert!((total(&mask) - area).abs() < 0.2, "{} vs {area}", total(&mask));
        // A parabola's segment is 2/3 of its bounding box; n chords lose
        // 1/n² of it, and this one is flattened into 10.
        let mut path = Path::new();
        path.move_to(0.0, 10.0);
        path.quad_to(5.0, -10.0, 10.0, 10.0);
        let mask = path.fill(FillRule::NonZero).unwrap();
        assert!((total(&mask) - 2.0 / 3.0 * 100.0 * (1.0 - 1.0 / 100.0)).abs() < 0.05, "{}", total(&mask));
    }

    #[test]
    fn fill_rules_decide_overlaps() {
        // Two squares wound the same way, overlapping in the middle.
        let mut path = Path::new();
        rect(&mut path, 0.0, 0.0, 4.0, 4.0);
        rect(&mut path, 2.0, 2.0, 6.0, 6.0);
        let nonzero = path.clone().fill(FillRule::NonZero).unwrap();
        let even_odd = path.fill(FillRule::EvenOdd).unwrap();
        assert_eq!((nonzero.at(3, 3), even_odd.at(3, 3)), (255, 0));
        assert_eq!((nonzero.at(1, 1), even_odd.at(1, 1)), (255, 255));
        // A hole wound the other way is a hole under both rules.
        let mut path = Path::new();
        rect(&mut path, 0.0, 0.0, 6.0, 6.0);
        path.move_to(2.0, 2.0);
        path.line_to(2.0, 4.0);
        path.line_to(4.0, 4.0);
        path.line_to(4.0, 2.0);
        assert_eq!(path.clone().fill(FillRule::NonZero).unwrap().at(3, 3), 0);
        assert_eq!(path.fill(FillRule::EvenOdd).unwrap().at(3, 3), 0);
    }

    #[test]
    fn transforms_map_points_before_filling() {
        // Font units, y up, at 1/100 px per unit: a 2 x 1 px box with its
        // bottom on the baseline at y = 10.
        let mut path = Path::with_transform(Transform::scale_translate(0.01, -0.01, 3.0, 10.0));
        rect(&mut path, 0.0, 0.0, 200.0, 100.0);
        let mask = path.fill(FillRule::NonZero).unwrap();
        assert_eq!((mask.left, mask.top, mask.width, mask.height), (3, 9, 2, 1));
    }

    #[test]
    fn nothing_to_fill() {
        assert_eq!(Path::new().fill(FillRule::NonZero), None);
        let mut line = Path::new();
        line.move_to(0.0, 0.0);
        line.line_to(5.0, 0.0);
        assert_eq!(line.fill(FillRule::NonZero), None);
    }
}
