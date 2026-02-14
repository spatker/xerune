use askama::Template;
use fontdue::Font;
use std::time::Instant;
use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

// Simple LCG for random numbers to avoid 'rand' dependency
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_f32(&mut self) -> f32 {
        // Linear Congruential Generator
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = (self.state >> 32) as u32;
        (x as f32) / (u32::MAX as f32)
    }

    fn next_u8(&mut self) -> u8 {
        self.next_f32().mul_add(255.0, 0.0) as u8
    }
}

#[derive(Clone, Debug)]
struct Item {
    x: f32,
    y: f32,
    vx: f32,
    vy: f32,
    size: f32,
    color: String,
    text: String,
}

#[derive(Template)]
#[template(path = "animation.html")]
struct AnimationTemplate<'a> {
    items: &'a [Item],
    fps: u32,
    item_count: usize,
    render_time_ms: String,
}

struct AnimationModel {
    items: Vec<Item>,
    last_frame: Instant,
    frame_count: u32,
    fps: u32,
    render_time: Option<f32>,
}

impl AnimationModel {
    fn new(count: usize) -> Self {
        let mut rng = Rng::new(12345);
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            items.push(Item {
                x: rng.next_f32() * 780.0,
                y: rng.next_f32() * 580.0,
                vx: (rng.next_f32() - 0.5) * 5.0,
                vy: (rng.next_f32() - 0.5) * 5.0,
                size: 10.0 + rng.next_f32() * 30.0,
                color: format!("rgba({}, {}, {}, 0.8)", 
                    rng.next_u8(), 
                    rng.next_u8(), 
                    rng.next_u8()),
                text: format!("{}", i + 1),
            });
        }
        
        Self {
            items,
            last_frame: Instant::now(),
            frame_count: 0,
            fps: 0,
            render_time: None,
        }
    }
}

impl Model for AnimationModel {
    fn view(&self) -> String {
        let template = AnimationTemplate {
            items: &self.items,
            fps: self.fps,
            item_count: self.items.len(),
            render_time_ms: self.render_time.map(|t| format!("{:.2}", t)).unwrap_or_else(|| "0.00".to_string()),
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: &str, _context: &mut xerune::Context) {
        if let Some(val) = msg.strip_prefix("render_time_ms:") {
            if let Ok(ms) = val.parse::<f32>() {
                self.render_time = Some(ms);
            }
        } else if msg == "tick" {
            // Update items
            for item in &mut self.items {
                item.x += item.vx;
                item.y += item.vy;
                
                if item.x < 0.0 || item.x > 780.0 { item.vx *= -1.0; }
                if item.y < 0.0 || item.y > 580.0 { item.vy *= -1.0; }
            }
            
            self.frame_count += 1;
            let now = Instant::now();
            let elapsed = now.duration_since(self.last_frame);
            if elapsed.as_secs() >= 1 {
                self.fps = self.frame_count;
                self.frame_count = 0;
                self.last_frame = now;
                
                // Dump profile to stdout
                println!("--- 1 Second Profile Dump ---");
                coarse_prof::write(&mut std::io::stdout()).unwrap();
                coarse_prof::reset();
            }
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    // Load fonts
    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular, roboto_bold];
    let fonts_ref: &'static [Font] = Box::leak(fonts.into_boxed_slice());

    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    
    // Create 100 items for benchmark
    let model = AnimationModel::new(100);
    let runtime = Runtime::new(model, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app("RMTUI Animation Benchmark", 800, 600, runtime, fonts_ref, Some(std::time::Duration::ZERO))
    }
    
    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
        support::linux_backend::run_app("RMTUI Animation Benchmark", 800, 600, runtime, fonts_ref, Some(std::time::Duration::ZERO))
    }
}
