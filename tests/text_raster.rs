//! Glyph pixels against Chromium's: `tests/text/glyphs.html`, rows of text
//! in the bundled fonts, drawn by xerune and compared with
//! `tests/text/glyphs-chromium.png` (`tools/chromium-shot.sh`).
//!
//! Each 40 px row is compared on its own, at the shift (up to 2 px either
//! way) that fits it best, so a row's numbers are about how glyphs are
//! drawn rather than where: the shift says where. Per row:
//!
//! - error: the summed difference of the pixels' brightness at that shift,
//!   relative to how much ink (difference from the row's background)
//!   Chromium's row has;
//! - ink: xerune's ink relative to Chromium's, below 1 when xerune's text
//!   looks lighter or thinner;
//! - off: pixels whose brightness differs by more than 64 of 255.
//!
//! `cargo test --test text_raster -- --nocapture` prints them, and the
//! images are kept in `target/tmp/text_raster/`.

use std::collections::HashMap;
use xerune::{Context, Model, Pixels, Renderer, Runtime, Surface, XeruneTemplate};
use xerune_render::SoftwareRenderer;

#[derive(XeruneTemplate)]
#[template(path = "tests/text/glyphs.html")]
struct Glyphs;
impl Model for Glyphs {
    type Message = String;
    fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
}

const WIDTH: usize = 800;
const HEIGHT: usize = 480;
const ROW: usize = 40;
const ROWS: [&str; 12] = [
    "11px", "12px", "14px", "16px", "20px", "24px", "32px", "14px bold", "24px bold", "16px Hangul", "16px Hangul bold", "16px dark",
];

/// The total error of all rows may not grow past this: lower it when
/// glyphs get closer to Chromium's. fontdue's glyphs were at 0.298.
const MAX_ERROR: f64 = 0.30;

fn fonts() -> xerune_render::Fonts {
    xerune_render::Fonts::ttf(
        include_bytes!("../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../resources/fonts/Roboto-Bold.ttf")),
    )
    .unwrap()
    .with_fallback(
        include_bytes!("../resources/fonts/XeruneHangul-Regular.otf"),
        Some(include_bytes!("../resources/fonts/XeruneHangul-Bold.otf")),
    )
    .unwrap()
}

/// Brightness, 0 to 255.
fn luma(r: u8, g: u8, b: u8) -> i32 {
    (r as i32 * 299 + g as i32 * 587 + b as i32 * 114) / 1000
}

fn render() -> Vec<i32> {
    let fonts = fonts();
    let mut runtime = Runtime::new(Glyphs, fonts.clone());
    runtime.set_size(WIDTH as f32, HEIGHT as f32);
    let commands = runtime.ui.build_commands(&HashMap::new(), None);
    let (canvases, images) = (HashMap::new(), xerune::ImageStore::new());
    let mut buffer = vec![0u32; WIDTH * HEIGHT];
    let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), WIDTH as u32, HEIGHT as u32);
    let area = target.area();
    SoftwareRenderer::new(fonts).render(&mut target, &commands, &xerune::RenderResources::new(&canvases, &images), area);
    buffer.iter().map(|&p| luma((p >> 16) as u8, (p >> 8) as u8, p as u8)).collect()
}

fn save(path: &std::path::Path, luma: &[i32]) {
    let mut pixmap = tiny_skia::Pixmap::new(WIDTH as u32, HEIGHT as u32).unwrap();
    for (pixel, &l) in pixmap.data_mut().chunks_exact_mut(4).zip(luma) {
        let l = l.clamp(0, 255) as u8;
        pixel.copy_from_slice(&[l, l, l, 255]);
    }
    pixmap.save_png(path).unwrap();
}

struct RowMatch {
    shift: (i32, i32),
    /// Summed brightness difference at `shift`.
    difference: i64,
    /// Chromium's and xerune's ink: summed difference from the background.
    ink: (i64, i64),
    off: usize,
}

/// Compares row `row` of `test` with `reference` at the best shift.
fn compare_row(test: &[i32], reference: &[i32], row: usize) -> RowMatch {
    let (top, bottom) = (row * ROW, (row + 1) * ROW);
    // The background: the right end of the row, where no text reaches.
    let background = reference[(top + ROW / 2) * WIDTH + WIDTH - 1];
    let at = |image: &[i32], x: i32, y: i32| {
        if x < 0 || x >= WIDTH as i32 || y < top as i32 || y >= bottom as i32 { background } else { image[y as usize * WIDTH + x as usize] }
    };
    let ink = |image: &[i32]| -> i64 {
        (top..bottom).flat_map(|y| (0..WIDTH).map(move |x| (x, y))).map(|(x, y)| (image[y * WIDTH + x] - background).abs() as i64).sum()
    };
    let difference = |dx: i32, dy: i32| -> (i64, usize) {
        let (mut sum, mut off) = (0, 0);
        for y in top as i32..bottom as i32 {
            for x in 0..WIDTH as i32 {
                let d = (at(test, x, y) - at(reference, x + dx, y + dy)).abs();
                sum += d as i64;
                off += usize::from(d > 64);
            }
        }
        (sum, off)
    };
    let shifts = (-2..=2).flat_map(|dy| (-2..=2).map(move |dx| (dx, dy)));
    let (shift, (difference, off)) = shifts.map(|(dx, dy)| ((dx, dy), difference(dx, dy))).min_by_key(|(_, (sum, _))| *sum).unwrap();
    RowMatch { shift, difference, ink: (ink(reference), ink(test)), off }
}

#[test]
fn glyphs_match_chromium() {
    let test = render();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("text_raster");
    std::fs::create_dir_all(&dir).unwrap();
    save(&dir.join("xerune.png"), &test);

    let reference = tiny_skia::Pixmap::load_png(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/text/glyphs-chromium.png"))
        .expect("tests/text/glyphs-chromium.png (tools/chromium-shot.sh tests/text/glyphs.html tests/text/glyphs-chromium.png)");
    assert_eq!((reference.width() as usize, reference.height() as usize), (WIDTH, HEIGHT));
    let reference: Vec<i32> = reference.data().chunks_exact(4).map(|p| luma(p[0], p[1], p[2])).collect();
    save(&dir.join("chromium.png"), &reference);
    let diff: Vec<i32> = test.iter().zip(&reference).map(|(a, b)| 255 - (a - b).abs()).collect();
    save(&dir.join("difference.png"), &diff);

    println!("{:<18} {:>7} {:>7} {:>6} {:>6}", "row", "shift", "error", "ink", "off");
    let (mut difference, mut ink, mut off) = (0, 0, 0);
    for (row, name) in ROWS.iter().enumerate() {
        let m = compare_row(&test, &reference, row);
        println!(
            "{name:<18} {:>7} {:>7.3} {:>6.3} {:>6}",
            format!("{},{}", m.shift.0, m.shift.1),
            m.difference as f64 / m.ink.0 as f64,
            m.ink.1 as f64 / m.ink.0 as f64,
            m.off
        );
        (difference, ink, off) = (difference + m.difference, ink + m.ink.0, off + m.off);
    }
    let error = difference as f64 / ink as f64;
    println!("{:<18} {:>7} {error:>7.3} {:>6} {off:>6}", "all", "", "");
    assert!(error <= MAX_ERROR, "glyphs are further from Chromium's: error {error:.3} > {MAX_ERROR}");
}
