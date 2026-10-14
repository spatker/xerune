#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use std::f32::consts::PI;
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;


const GAME_WIDTH: f32 = 800.0;
const GAME_HEIGHT: f32 = 480.0;

const PADDLE_WIDTH: f32 = 110.0;
const PADDLE_HEIGHT: f32 = 14.0;
/// Paddle distance from the bottom edge.
const PADDLE_BOTTOM: f32 = 46.0;
const PADDLE_SPEED: f32 = 460.0; // px per second

const BALL_SIZE: f32 = 14.0;
const INITIAL_BALL_SPEED: f32 = 320.0;
const LIVES: u32 = 3;

const COLS: usize = 10;
const ROWS: usize = 5;
const BLOCK_HEIGHT: f32 = 22.0;
const BLOCK_GAP: f32 = 8.0;
const BOARD_MARGIN_X: f32 = 24.0;
/// Space above the bricks for the score bar.
const BOARD_OFFSET_Y: f32 = 64.0;

/// Per row, top to bottom: highlight color, base color, points.
const ROW_STYLE: [(&str, &str, u32); ROWS] = [
    ("#fbcfe8", "#ec4899", 50),
    ("#fed7aa", "#f97316", 40),
    ("#fef08a", "#eab308", 30),
    ("#bbf7d0", "#22c55e", 20),
    ("#a5f3fc", "#06b6d4", 10),
];

struct LcgRng {
    state: u64,
}

impl LcgRng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn gen_range(&mut self, min: f32, max: f32) -> f32 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let x = (self.state >> 32) as u32;
        let pct = (x as f32) / (u32::MAX as f32);
        min + pct * (max - min)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Block {
    pub x: f32,
    pub y: f32,
    pub alive: bool,
    pub color: String,
    /// Lighter top color for the brick's gloss.
    pub shine: String,
    pub points: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub dx: f32,
    pub dy: f32,
    pub life: f32, // 1.0 down to 0.0
    pub color: String,
}

#[derive(XeruneTemplate, Serialize, Deserialize)]
#[template(path = "breakout.html")]
pub struct BreakoutModel {
    pub paddle_x: f32,
    pub ball_x: f32,
    pub ball_y: f32,
    pub ball_dx: f32,
    pub ball_dy: f32,
    pub blocks: Vec<Block>,
    pub particles: Vec<Particle>,
    pub keys_held: HashSet<String>,
    #[serde(skip, default = "xerune::runtime::time::Instant::now")]
    pub last_tick: xerune::runtime::time::Instant,
    pub game_over: bool,
    pub won: bool,
    pub score: u32,
    pub lives: u32,
    pub paddle_y: f32,
    pub paddle_w: f32,
    pub paddle_h: f32,
    pub ball_s: f32,
    pub block_w: f32,
    pub block_h: f32,
    pub game_width: f32,
    pub game_height: f32,
}

impl Default for BreakoutModel {
    fn default() -> Self {
        Self::new(GAME_WIDTH, GAME_HEIGHT)
    }
}

impl BreakoutModel {
    /// A fresh game laid out for a `width` x `height` board.
    pub fn new(width: f32, height: f32) -> Self {
        let block_w = (width - 2.0 * BOARD_MARGIN_X - (COLS - 1) as f32 * BLOCK_GAP) / COLS as f32;
        let mut blocks = Vec::new();
        for (row, &(shine, color, points)) in ROW_STYLE.iter().enumerate() {
            for col in 0..COLS {
                blocks.push(Block {
                    x: BOARD_MARGIN_X + col as f32 * (block_w + BLOCK_GAP),
                    y: BOARD_OFFSET_Y + row as f32 * (BLOCK_HEIGHT + BLOCK_GAP),
                    alive: true,
                    color: color.to_string(),
                    shine: shine.to_string(),
                    points,
                });
            }
        }

        let mut model = Self {
            paddle_x: width / 2.0 - PADDLE_WIDTH / 2.0,
            ball_x: 0.0,
            ball_y: 0.0,
            ball_dx: 0.0,
            ball_dy: 0.0,
            blocks,
            particles: Vec::new(),
            keys_held: HashSet::new(),
            last_tick: xerune::runtime::time::Instant::now(),
            game_over: false,
            won: false,
            score: 0,
            lives: LIVES,
            paddle_y: height - PADDLE_BOTTOM,
            paddle_w: PADDLE_WIDTH,
            paddle_h: PADDLE_HEIGHT,
            ball_s: BALL_SIZE,
            block_w,
            block_h: BLOCK_HEIGHT,
            game_width: width,
            game_height: height,
        };
        model.serve();
        model
    }

    /// Put the ball above the paddle, heading up and to the right.
    fn serve(&mut self) {
        self.ball_x = self.paddle_x + PADDLE_WIDTH / 2.0 - BALL_SIZE / 2.0;
        self.ball_y = self.paddle_y - BALL_SIZE - 2.0;
        self.ball_dx = INITIAL_BALL_SPEED * 0.707;
        self.ball_dy = -INITIAL_BALL_SPEED * 0.707;
    }

    pub fn bricks_left(&self) -> usize {
        self.blocks.iter().filter(|b| b.alive).count()
    }

    /// One slot per life for the HUD; `true` while the life is unused.
    pub fn life_slots(&self) -> Vec<Life> {
        (0..LIVES).map(|i| Life { left: i < self.lives }).collect()
    }
}

pub struct Life {
    pub left: bool,
}

#[derive(Debug, Clone, PartialEq, XeruneMessage)]
pub enum Msg {
    Tick,
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
    #[xerune(prefix = "keyup:")]
    KeyUp(String),
}

impl Model for BreakoutModel {
    type Message = Msg;

    fn on_resize(&mut self, width: f32, height: f32, _context: &mut xerune::Context) {
        if width != self.game_width || height != self.game_height {
            // The board is laid out for the screen; start over on a new size.
            *self = BreakoutModel::new(width, height);
        }
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
        match msg {
            Msg::Tick => {
                let now = xerune::runtime::time::Instant::now();
                let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
                self.last_tick = now;

                if self.game_over || self.won {
                    context.clear_interval("tick");
                    return;
                }
                let (width, _height) = (self.game_width, self.game_height);

                // --- Paddle Movement ---
                let mut paddle_dir = 0.0;
                if self.keys_held.contains("ArrowLeft") 
                    || self.keys_held.contains("KEY_PREVIOUSSONG") 
                    || self.keys_held.contains("KEY_LEFT") 
                    || self.keys_held.contains("KEY_VOLUMEDOWN") 
                    || self.keys_held.contains("Prev")
                    || self.keys_held.contains("Left")
                { 
                    paddle_dir -= 1.0; 
                }
                if self.keys_held.contains("ArrowRight") 
                    || self.keys_held.contains("KEY_NEXTSONG") 
                    || self.keys_held.contains("KEY_RIGHT") 
                    || self.keys_held.contains("KEY_VOLUMEUP") 
                    || self.keys_held.contains("Next")
                    || self.keys_held.contains("Right")
                { 
                    paddle_dir += 1.0; 
                }

                self.paddle_x += paddle_dir * PADDLE_SPEED * dt;
                self.paddle_x = self.paddle_x.clamp(0.0, width - PADDLE_WIDTH);

                // --- Ball Movement ---
                self.ball_x += self.ball_dx * dt;
                self.ball_y += self.ball_dy * dt;

                // --- Wall Collisions ---
                if self.ball_x <= 0.0 {
                    self.ball_x = 0.0;
                    self.ball_dx = self.ball_dx.abs();
                } else if self.ball_x >= width - BALL_SIZE {
                    self.ball_x = width - BALL_SIZE;
                    self.ball_dx = -self.ball_dx.abs();
                }

                if self.ball_y <= 0.0 {
                    self.ball_y = 0.0;
                    self.ball_dy = self.ball_dy.abs();
                } else if self.ball_y >= self.game_height {
                    self.lives = self.lives.saturating_sub(1);
                    if self.lives == 0 {
                        self.game_over = true;
                    } else {
                        self.serve();
                    }
                }

                // --- Paddle Collision ---
                let paddle_y = self.paddle_y;
                if self.ball_y + BALL_SIZE >= paddle_y 
                    && self.ball_y <= paddle_y + PADDLE_HEIGHT 
                    && self.ball_x + BALL_SIZE >= self.paddle_x 
                    && self.ball_x <= self.paddle_x + PADDLE_WIDTH 
                    && self.ball_dy > 0.0
                {
                    self.ball_y = paddle_y - BALL_SIZE;
                    
                    let hit_factor = ((self.ball_x + BALL_SIZE / 2.0) - (self.paddle_x + PADDLE_WIDTH / 2.0)) / (PADDLE_WIDTH / 2.0);
                    
                    let speed = (self.ball_dx * self.ball_dx + self.ball_dy * self.ball_dy).sqrt();
                    let max_bounce_angle = PI / 3.0; 
                    let bounce_angle = hit_factor * max_bounce_angle;
                    
                    self.ball_dx = speed * bounce_angle.sin();
                    self.ball_dy = -speed * bounce_angle.cos();
                }

                // --- Block Collisions ---
                let mut hit_block = false;
                let mut rng = LcgRng::new(self.ball_x.to_bits() as u64 ^ self.ball_y.to_bits() as u64);
                let (block_w, block_h) = (self.block_w, self.block_h);
                
                for block in self.blocks.iter_mut() {
                    if !block.alive { continue; }

                    if self.ball_x + BALL_SIZE >= block.x 
                        && self.ball_x <= block.x + block_w 
                        && self.ball_y + BALL_SIZE >= block.y 
                        && self.ball_y <= block.y + block_h 
                    {
                        block.alive = false;
                        hit_block = true;
                        self.score += block.points;

                        // Spawn particles using custom LcgRng
                        for _ in 0..10 {
                            let angle = rng.gen_range(0.0, PI * 2.0);
                            let speed = rng.gen_range(50.0, 150.0);
                            self.particles.push(Particle {
                                x: block.x + block_w / 2.0,
                                y: block.y + block_h / 2.0,
                                dx: angle.cos() * speed,
                                dy: angle.sin() * speed,
                                life: 1.0,
                                color: block.color.clone(),
                            });
                        }

                        let overlap_left = (self.ball_x + BALL_SIZE) - block.x;
                        let overlap_right = (block.x + block_w) - self.ball_x;
                        let overlap_top = (self.ball_y + BALL_SIZE) - block.y;
                        let overlap_bottom = (block.y + block_h) - self.ball_y;

                        let min_overlap = overlap_left.min(overlap_right).min(overlap_top).min(overlap_bottom);

                        if min_overlap == overlap_left || min_overlap == overlap_right {
                            self.ball_dx *= -1.0;
                        } else {
                            self.ball_dy *= -1.0;
                        }
                        
                        break;
                    }
                }

                if hit_block {
                    if self.blocks.iter().all(|b| !b.alive) {
                        self.won = true;
                    }
                }

                // --- Update Particles ---
                for particle in self.particles.iter_mut() {
                    particle.x += particle.dx * dt;
                    particle.y += particle.dy * dt;
                    particle.life -= 1.5 * dt;
                }
                self.particles.retain(|p| p.life > 0.0);

                if !self.game_over && !self.won {
                    context.set_timeout("tick".to_string(), 16);
                }
            },
            Msg::KeyDown(key) => {
                if self.game_over || self.won {
                    if matches!(key.as_str(), "KEY_PLAYPAUSE" | "KEY_UP" | "Space" | "Enter" | "KEY_NEXTSONG" | "KEY_PREVIOUSSONG") {
                        *self = BreakoutModel::new(self.game_width, self.game_height);
                        context.set_timeout("tick".to_string(), 16);
                        return;
                    }
                }
                self.keys_held.insert(key);
            },
            Msg::KeyUp(key) => {
                self.keys_held.remove(&key);
            },
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<BreakoutModel, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = BreakoutModel::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let mut runtime = Runtime::new(model, measurer);
    runtime.set_timeout("tick".to_string(), 16);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Breakout", GAME_WIDTH as u32, GAME_HEIGHT as u32, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Breakout", GAME_WIDTH as u32, GAME_HEIGHT as u32, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Breakout", GAME_WIDTH as u32, GAME_HEIGHT as u32, runtime, render_frame, |_| {})?;
    }

    Ok(())
}
