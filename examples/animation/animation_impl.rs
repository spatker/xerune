use serde::{Serialize, Deserialize};
use xerune::{Model, XeruneMessage, XeruneTemplate};

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
#[template(path = "examples/animation/templates/animation.html")]
pub struct AnimationModel {
    pub items: Vec<Item>,
    /// Frames per second, from the runtime's stats.
    pub fps: u32,
    pub item_count: usize,
    /// Milliseconds per frame, all phases, and drawing alone.
    pub frame_ms: String,
    pub render_ms: String,
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
            fps: 0,
            item_count: count,
            frame_ms: "0.00".to_string(),
            render_ms: "0.00".to_string(),
            bounds: default_bounds(),
        }
    }
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum AnimationMsg {
    /// What the last second's frames cost.
    #[xerune(skip)]
    Stats(xerune::FrameStats),
    /// Seconds since the previous frame.
    Frame(f32),
}

impl Model for AnimationModel {
    type Message = AnimationMsg;

    fn subscriptions(&self) -> xerune::Subscriptions<AnimationMsg> {
        xerune::Subscriptions::none().frames(AnimationMsg::Frame).stats(core::time::Duration::from_secs(1), AnimationMsg::Stats)
    }

    fn on_resize(&mut self, width: f32, height: f32, _context: &mut xerune::Context<AnimationMsg>) {
        self.bounds = (width, height);
    }

    fn update(&mut self, msg: Self::Message, _context: &mut xerune::Context<AnimationMsg>) {
        match msg {
            AnimationMsg::Stats(stats) => {
                self.fps = stats.fps().round() as u32;
                self.frame_ms = format!("{:.2}", stats.frame_time().as_secs_f32() * 1000.0);
                self.render_ms = format!("{:.2}", stats.raster.avg.as_secs_f32() * 1000.0);
            }
            AnimationMsg::Frame(dt) => {
                // Velocities are in pixels per 60 Hz frame.
                let steps = dt * 60.0;
                for item in &mut self.items {
                    item.x += item.vx * steps;
                    item.y += item.vy * steps;
                    
                    let (w, h) = self.bounds;
                    if item.x < 0.0 { item.vx = item.vx.abs(); }
                    if item.x + item.size > w { item.vx = -item.vx.abs(); }
                    if item.y < 0.0 { item.vy = item.vy.abs(); }
                    if item.y + item.size > h { item.vy = -item.vy.abs(); }
                }
            }
        }
    }
}

