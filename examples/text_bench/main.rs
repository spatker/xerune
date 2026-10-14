//! Text layout benchmark: rebuilds, lays out and renders a text-heavy feed
//! `ITERS` times headlessly and prints best-of-N timings per phase.
//!
//! ```sh
//! nice cargo run -j4 --release --example text_bench
//! TEXT_BENCH_CARDS=40 TEXT_BENCH_ITERS=200 TEXT_BENCH_SIZE=480x800 cargo run --release --example text_bench
//! ```

use std::time::{Duration, Instant};

use fast_renderer::{FastMeasurer, FastRenderer};
use xerune::{Context, Model, Runtime, XeruneTemplate};

#[derive(Clone)]
pub struct Card {
    pub title: String,
    pub author: String,
    pub body: String,
}

#[derive(XeruneTemplate)]
#[template(path = "text_bench.html")]
pub struct TextBench {
    pub cards: Vec<Card>,
}

impl Model for TextBench {
    type Message = String;
    fn update(&mut self, _msg: String, _context: &mut Context) {}
}

const BODIES: [&str; 4] = [
    "The quick brown fox jumps over the lazy dog while the committee debates whether foxes should be allowed to jump at all.",
    "Embedded displays are small, so every line of text has to earn its place; wrapping keeps long sentences readable without shrinking the font.",
    "A software rasterizer draws each glyph from a cached coverage bitmap, blending spans of pixels into the frame buffer row by row.",
    "Short note.",
];

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn best(samples: &[Duration]) -> Duration {
    samples.iter().copied().min().unwrap_or_default()
}

fn median(samples: &mut [Duration]) -> Duration {
    samples.sort();
    samples.get(samples.len() / 2).copied().unwrap_or_default()
}

fn main() {
    let cards: usize = env_or("TEXT_BENCH_CARDS", 12);
    let iters: usize = env_or("TEXT_BENCH_ITERS", 100);
    let (width, height) = std::env::var("TEXT_BENCH_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((800, 480));

    let regular = fontdue::Font::from_bytes(
        include_bytes!("../../resources/fonts/Roboto-Regular.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let bold = fontdue::Font::from_bytes(
        include_bytes!("../../resources/fonts/Roboto-Bold.ttf") as &[u8],
        fontdue::FontSettings::default(),
    )
    .unwrap();
    let fonts: &'static [fontdue::Font] = Box::leak(Box::new([regular, bold]));

    let model = TextBench {
        cards: (0..cards)
            .map(|i| Card {
                title: format!("Story number {i}: text layout on small screens"),
                author: format!("Author {}", i % 5),
                body: BODIES[i % BODIES.len()].to_string(),
            })
            .collect(),
    };
    let mut runtime = Runtime::new(model, FastMeasurer { fonts: fonts.into() });
    runtime.set_size(width as f32, height as f32);

    let mut buffer = vec![0u32; (width * height) as usize];
    let mut image_cache = Default::default();
    let mut glyph_cache = Default::default();

    let mut rebuild = Vec::with_capacity(iters);
    let mut commands = Vec::with_capacity(iters);
    let mut render = Vec::with_capacity(iters);
    let mut command_count = 0;
    for _ in 0..iters {
        // Build + style resolution + layout.
        let t = Instant::now();
        runtime.sync_view();
        rebuild.push(t.elapsed());

        let t = Instant::now();
        command_count = runtime.ui.build_commands(&Default::default(), None).len();
        commands.push(t.elapsed());

        // A rebuild forgets the previous frame, so this is a full repaint
        // (command building, diffing and drawing).
        let t = Instant::now();
        let mut renderer = FastRenderer::new(&mut buffer, width, height, fonts, &mut image_cache, &mut glyph_cache);
        runtime.render(&mut renderer);
        render.push(t.elapsed());
    }

    println!("text_bench: {cards} cards, {width}x{height}, {iters} iterations, {command_count} draw commands");
    for (name, samples) in [("rebuild+layout", &mut rebuild), ("build_commands", &mut commands), ("render", &mut render)] {
        println!(
            "  {name:<15} best {:>8.1} us   median {:>8.1} us",
            best(samples).as_secs_f64() * 1e6,
            median(samples).as_secs_f64() * 1e6
        );
    }
}
