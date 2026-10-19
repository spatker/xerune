//! Rendering benchmark on the real example UIs, headless.
//!
//! For every scene it measures
//! - `raster`: a full repaint of the scene's draw commands (pure rasterizer cost),
//! - `frame`: a message, the rebuild it triggers and the damage-limited render,
//!   as one interaction in the event loop, with the area it repainted and
//!   the heap allocations of the update and of the render,
//!
//! and prints a hash of the final pixels so renderer changes can prove they
//! draw the same image. After the frames it checks that they left the
//! pixels of a full repaint, so damage that misses a change shows.
//!
//! ```sh
//! nice cargo run -j4 --release --example render_bench
//! RENDER_BENCH_SCENES=todo,calculator RENDER_BENCH_ITERS=200 cargo run --release --example render_bench
//! ```
//!
//! `RENDER_BENCH_PHASE=raster|frame` runs only one phase; with
//! `RENDER_BENCH_ITERS=0` and `N` under `qemu-arm` and an instruction-counting
//! plugin, the difference divided by `N` is the per-iteration instruction count
//! on ARM.

#[path = "../animation/animation_impl.rs"]
#[allow(dead_code)]
mod animation_impl;
#[path = "../calculator/calculator_impl.rs"]
#[allow(dead_code)]
mod calculator_impl;
#[path = "../music_player/music_player_impl.rs"]
#[allow(dead_code)]
mod music_player_impl;
#[path = "../showcase/showcase_impl.rs"]
#[allow(dead_code)]
mod showcase_impl;
#[path = "../todo/todo_impl.rs"]
#[allow(dead_code)]
mod todo_impl;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use xerune::images::{DefaultImageLoader, ImageError, ImageLoader};
use xerune::{Model, Pixels, RenderResources, Runtime, Surface, TemplateLayout};
use xerune_render::{Fonts, SoftwareRenderer};

/// Counts heap allocations, to report them per frame.
struct CountingAlloc;

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn allocations() -> usize {
    ALLOCATIONS.load(Ordering::Relaxed)
}

/// Loads local images only, so results do not depend on the network.
struct LocalImages(DefaultImageLoader);

impl ImageLoader for LocalImages {
    fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
        if xerune::images::is_remote(src) {
            return Err(ImageError::HttpDisabled);
        }
        self.0.fetch(src)
    }
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

fn median(samples: &mut [Duration]) -> f64 {
    samples.sort();
    samples.get(samples.len() / 2).copied().unwrap_or_default().as_secs_f64() * 1e6
}

struct Bench {
    width: u32,
    height: u32,
    iters: usize,
    phase: Option<String>,
    fonts: Fonts,
}

impl Bench {
    /// Runs one scene; `messages` alternate, one per `frame` iteration.
    fn run<M: Model + TemplateLayout + 'static>(&self, name: &str, model: M, messages: [&str; 2]) {
        let (width, height) = (self.width, self.height);
        let mut runtime = Runtime::new(model, self.fonts.clone());
        runtime.set_image_loader(LocalImages(DefaultImageLoader::default()));
        runtime.set_size(width as f32, height as f32);
        runtime.sync_view();
        runtime.wait_for_images(Duration::from_secs(10));
        runtime.poll();

        let mut buffer = vec![0u32; (width * height) as usize];
        let mut renderer = SoftwareRenderer::new(self.fonts.clone());
        // Warm the glyph and image caches as a running app has them.
        runtime.render(&mut renderer, &mut Surface::new(Pixels::Xrgb8888(&mut buffer), width, height));

        let wants = |phase: &str| self.phase.as_deref().is_none_or(|p| p == phase);
        let mut line = format!("{name:<13}");

        if wants("raster") {
            let commands = runtime.ui.build_commands(&runtime.context().canvases, None);
            let resources = RenderResources::new(&runtime.context().canvases, runtime.images());
            let mut samples = Vec::with_capacity(self.iters);
            for _ in 0..self.iters {
                let t = Instant::now();
                let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), width, height);
                let area = target.area();
                xerune::Renderer::render(&mut renderer, &mut target, &commands, &resources, area);
                samples.push(t.elapsed());
            }
            let hash = xerune::model::hash_bytes(bytemuck::cast_slice(&buffer));
            line += &format!(" raster {:>8.1} us ({} cmds, pixels {hash:016x})", median(&mut samples), commands.len());
        }

        if wants("frame") {
            let (mut update, mut render) = (Vec::with_capacity(self.iters), Vec::with_capacity(self.iters));
            let mut area = 0.0;
            let (mut update_allocs, mut render_allocs) = (0, 0);
            for i in 0..self.iters {
                let message = messages[i % 2].to_string();
                let (t, a) = (Instant::now(), allocations());
                runtime.handle_messages([message]);
                update.push(t.elapsed());
                update_allocs += allocations() - a;

                let (t, a) = (Instant::now(), allocations());
                let damage = runtime.render(&mut renderer, &mut Surface::new(Pixels::Xrgb8888(&mut buffer), width, height));
                render.push(t.elapsed());
                render_allocs += allocations() - a;
                let screen = xerune::Rect::new(0.0, 0.0, width as f32, height as f32);
                area += damage.rects().iter().filter_map(|d| d.intersect(&screen)).map(|d| d.width * d.height).sum::<f32>();
            }
            // Damage-limited frames must leave what a full repaint draws.
            let commands = runtime.ui.build_commands(&runtime.context().canvases, None);
            let resources = RenderResources::new(&runtime.context().canvases, runtime.images());
            let mut full = vec![0u32; buffer.len()];
            let mut target = Surface::new(Pixels::Xrgb8888(&mut full), width, height);
            let screen_area = target.area();
            xerune::Renderer::render(&mut renderer, &mut target, &commands, &resources, screen_area);
            let stale = buffer.iter().zip(&full).filter(|(a, b)| a != b).count();
            if stale > 0 {
                eprintln!("{name}: {stale} pixels differ from a full repaint after the frames");
            }
            let screen_share = area / self.iters.max(1) as f32 / (width * height) as f32 * 100.0;
            let iters = self.iters.max(1);
            line += &format!(
                " | frame: update {:>8.1} us render {:>8.1} us, repaints {screen_share:>5.1}% of the screen, allocations {}+{}",
                median(&mut update),
                median(&mut render),
                update_allocs / iters,
                render_allocs / iters
            );
        }
        println!("{line}");
    }
}

fn main() {
    let (width, height) = std::env::var("RENDER_BENCH_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?))
        })
        .unwrap_or((800, 480));
    let bench = Bench {
        width,
        height,
        iters: env_or("RENDER_BENCH_ITERS", 100),
        phase: std::env::var("RENDER_BENCH_PHASE").ok(),
        fonts: xerune_render::Fonts::ttf(
            include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
            Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
        )
        .unwrap(),
    };
    let scenes = std::env::var("RENDER_BENCH_SCENES").unwrap_or_else(|_| "todo,calculator,showcase,vehicle,music_player,animation".into());

    println!("render_bench: {width}x{height}, {} iterations, medians", bench.iters);
    for scene in scenes.split(',') {
        match scene {
            "todo" => bench.run(scene, todo_impl::TodoList::default(), ["toggle:0", "toggle:0"]),
            "calculator" => bench.run(scene, calculator_impl::CalculatorModel::default(), ["digit:7", "clear"]),
            "showcase" => bench.run(scene, showcase_impl::ShowcaseModel::default(), ["toggle_wifi", "toggle_wifi"]),
            "animation" => bench.run(scene, animation_impl::AnimationModel::new(100), ["frame:0.0166", "frame:0.0166"]),
            "vehicle" => bench.run(scene, showcase_impl::ShowcaseModel { active_tab: 2, ..Default::default() }, ["tick", "tick"]),
            "music_player" => bench.run(scene, music_player_impl::MusicPlayerModel::new(None), ["hover_track:2", "unhover_track"]),
            other => eprintln!("unknown scene {other}"),
        }
    }
}
