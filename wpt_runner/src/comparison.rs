//! Pixel comparison with WPT semantics: images must be identical unless the
//! test allows a difference with `<meta name=fuzzy>`.

use tiny_skia::Pixmap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fuzzy {
    pub max_difference: [u32; 2],
    pub total_pixels: [u32; 2],
}

impl Fuzzy {
    /// Parses `maxDifference=1-2;totalPixels=3-4`, or the positional `1-2;3-4`.
    pub fn parse(value: &str) -> Result<Self, String> {
        let mut named = [None, None];
        let mut positional = Vec::new();
        let parts: Vec<_> = value.split(';').collect();
        if parts.len() != 2 {
            return Err("fuzzy metadata requires two ranges".into());
        }
        for part in parts {
            let (name, value) = match part.split_once('=') {
                Some((name, value)) => (Some(name.trim()), value),
                None => (None, part),
            };
            let (min, max) = value.trim().split_once('-').unwrap_or((value, value));
            let min: u32 = min.trim().parse().map_err(|_| "invalid fuzzy minimum")?;
            let max: u32 = max.trim().parse().map_err(|_| "invalid fuzzy maximum")?;
            if min > max {
                return Err("reversed fuzzy range".into());
            }
            let range = [min, max];
            match name {
                Some(name) => {
                    let index = match name {
                        "maxDifference" => 0,
                        "totalPixels" => 1,
                        _ => return Err(format!("unknown fuzzy property: {name}")),
                    };
                    if named[index].replace(range).is_some() {
                        return Err(format!("duplicate fuzzy property: {name}"));
                    }
                }
                None => positional.push(range),
            }
        }
        let mut positional = positional.into_iter();
        let result = Self {
            max_difference: named[0].or_else(|| positional.next()).ok_or("missing maxDifference")?,
            total_pixels: named[1].or_else(|| positional.next()).ok_or("missing totalPixels")?,
        };
        if result.max_difference[1] > 255 {
            return Err("maxDifference exceeds 255".into());
        }
        Ok(result)
    }

    pub fn matches(self, difference: Difference) -> bool {
        // Identical images match when either lower bound is zero, as in WPT.
        if difference.total_pixels == 0 {
            return self.total_pixels[0] == 0 || self.max_difference[0] == 0;
        }
        (self.max_difference[0]..=self.max_difference[1]).contains(&difference.max_difference)
            && (self.total_pixels[0]..=self.total_pixels[1]).contains(&difference.total_pixels)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Difference {
    pub max_difference: u32,
    pub total_pixels: u32,
}

/// Counts differing pixels and the largest RGB channel difference. Both
/// images are opaque screenshots, so alpha is ignored.
pub fn compare(a: &Pixmap, b: &Pixmap) -> Difference {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let mut result = Difference::default();
    for (a, b) in a.data().chunks_exact(4).zip(b.data().chunks_exact(4)) {
        let difference = (0..3).map(|i| a[i].abs_diff(b[i]) as u32).max().unwrap();
        result.max_difference = result.max_difference.max(difference);
        result.total_pixels += u32::from(difference != 0);
    }
    result
}

/// Largest per-pixel difference that still counts as anti-aliasing.
const EDGE_TOLERANCE: u8 = 64;

/// Whether `test` differs from `reference` only in the coverage of edge
/// pixels, which border another color in either image. Two rasterizers disagree there (Chromium hints Ahem's glyph boxes
/// onto the pixel grid, xerune doesn't hint), while anything missing, moved by a
/// pixel or recolored differs by far more than [`EDGE_TOLERANCE`].
pub fn differs_only_in_antialiasing(test: &Pixmap, reference: &Pixmap) -> bool {
    let (w, h) = (reference.width() as usize, reference.height() as usize);
    let (t, r) = (test.data(), reference.data());
    let pixel = |d: &[u8], x: usize, y: usize| { let i = (y * w + x) * 4; [d[i], d[i + 1], d[i + 2]] };
    for y in 0..h {
        for x in 0..w {
            let (a, b) = (pixel(t, x, y), pixel(r, x, y));
            if a == b {
                continue;
            }
            if (0..3).any(|i| a[i].abs_diff(b[i]) > EDGE_TOLERANCE) {
                return false;
            }
            // An edge in either image: where two lines of text touch, the
            // reference is solid but the render shows both glyphs' edge rows.
            let on_edge = (y.saturating_sub(1)..=(y + 1).min(h - 1))
                .flat_map(|ny| (x.saturating_sub(1)..=(x + 1).min(w - 1)).map(move |nx| (nx, ny)))
                .any(|(nx, ny)| pixel(r, nx, ny) != b || pixel(t, nx, ny) != a);
            if !on_edge {
                return false;
            }
        }
    }
    true
}

/// Differing pixels in red over a faded copy of `a`.
pub fn diff_image(a: &Pixmap, b: &Pixmap) -> Pixmap {
    let mut result = a.clone();
    for ((a, b), out) in a.data().chunks_exact(4)
        .zip(b.data().chunks_exact(4)).zip(result.data_mut().chunks_exact_mut(4))
    {
        if a[..3] != b[..3] {
            out.copy_from_slice(&[255, 0, 0, 255]);
        } else {
            out.copy_from_slice(&[a[0] / 5 + 200, a[1] / 5 + 200, a[2] / 5 + 200, 255]);
        }
    }
    result
}
