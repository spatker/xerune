#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use serde::{Serialize, Deserialize};
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use xerune_render::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;


// Simple LCG for random numbers to avoid 'rand' dependency
#[derive(Serialize, Deserialize, Clone, Debug)]
struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_f32(&mut self) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = (self.state >> 32) as u32;
        (x as f32) / (u32::MAX as f32)
    }

}

/// Item fills: one palette instead of random RGB, same translucency.
const PALETTE: [&str; 8] = [
    "rgba(244, 114, 182, 0.8)",
    "rgba(167, 139, 250, 0.8)",
    "rgba(96, 165, 250, 0.8)",
    "rgba(34, 211, 238, 0.8)",
    "rgba(52, 211, 153, 0.8)",
    "rgba(163, 230, 53, 0.8)",
    "rgba(251, 191, 36, 0.8)",
    "rgba(251, 113, 133, 0.8)",
];

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Item {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub size: f32,
    pub color: String,
    pub text: String,
}

#[derive(XeruneTemplate, Serialize, Deserialize)]
#[template(path = "animation.html")]
pub struct AnimationModel {
    pub items: Vec<Item>,
    #[serde(skip, default = "xerune::runtime::time::Instant::now")]
    pub last_frame: xerune::runtime::time::Instant,
    pub frame_count: u32,
    pub fps: u32,
    pub render_time: Option<f32>,
    pub item_count: usize,
    pub render_time_ms: String,
    /// Viewport the items bounce in, from `on_resize`.
    #[serde(default = "default_bounds")]
    pub bounds: (f32, f32),
}

fn default_bounds() -> (f32, f32) {
    (800.0, 480.0)
}

impl AnimationModel {
    pub fn new(count: usize) -> Self {
        let mut rng = LcgRng::new(12345);
        let mut items = Vec::with_capacity(count);
        for i in 0..count {
            let size = 10.0 + rng.next_f32() * 30.0;
            items.push(Item {
                x: rng.next_f32() * (800.0 - size),
                y: rng.next_f32() * (480.0 - size),
                vx: (rng.next_f32() - 0.5) * 5.0,
                vy: (rng.next_f32() - 0.5) * 5.0,
                size,
                color: PALETTE[(rng.next_f32() * PALETTE.len() as f32) as usize % PALETTE.len()].to_string(),
                text: format!("{}", i + 1),
            });
        }
        
        Self {
            items,
            last_frame: xerune::runtime::time::Instant::now(),
            frame_count: 0,
            fps: 0,
            render_time: None,
            item_count: count,
            render_time_ms: "0.00".to_string(),
            bounds: default_bounds(),
        }
    }
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum AnimationMsg {
    #[xerune(prefix = "render_time_ms:")]
    RenderTime(f32),
    Tick,
}

impl Model for AnimationModel {
    type Message = AnimationMsg;

    fn on_resize(&mut self, width: f32, height: f32, _context: &mut xerune::Context) {
        self.bounds = (width, height);
    }

    fn update(&mut self, msg: Self::Message, _context: &mut xerune::Context) {
        match msg {
            AnimationMsg::RenderTime(ms) => {
                self.render_time = Some(ms);
                self.render_time_ms = format!("{:.2}", ms);
            },
            AnimationMsg::Tick => {
                for item in &mut self.items {
                    item.x += item.vx;
                    item.y += item.vy;
                    
                    let (w, h) = self.bounds;
                    if item.x < 0.0 { item.vx = item.vx.abs(); }
                    if item.x + item.size > w { item.vx = -item.vx.abs(); }
                    if item.y < 0.0 { item.vy = item.vy.abs(); }
                    if item.y + item.size > h { item.vy = -item.vy.abs(); }
                }
                
                self.frame_count += 1;
                let now = xerune::runtime::time::Instant::now();
                let elapsed = now.duration_since(self.last_frame);
                if elapsed.as_secs() >= 1 {
                    self.fps = self.frame_count;
                    self.frame_count = 0;
                    self.last_frame = now;
                    
                    #[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
                    {
                        println!("--- 1 Second Profile Dump ---");
                        coarse_prof::write(&mut std::io::stdout()).unwrap();
                        coarse_prof::reset();
                    }
                }
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<AnimationModel, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = AnimationModel::new(100);
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let mut runtime = Runtime::new(model, measurer);
    runtime.set_interval("tick".to_string(), 16);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Animation Benchmark", 800, 480, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Animation Benchmark", 800, 480, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Animation Benchmark", 800, 480, runtime, render_frame, |_| {})?;
    }

    Ok(())
}
