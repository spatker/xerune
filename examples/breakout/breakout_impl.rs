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

const PADDLE_WIDTH: f32 = 100.0;
const PADDLE_HEIGHT: f32 = 16.0;
const PADDLE_Y: f32 = GAME_HEIGHT - 40.0;
const PADDLE_SPEED: f32 = 400.0; // px per second

const BALL_SIZE: f32 = 12.0;
const INITIAL_BALL_SPEED: f32 = 300.0;

const COLS: usize = 10;
const ROWS: usize = 5;
const BLOCK_WIDTH: f32 = 64.0;
const BLOCK_HEIGHT: f32 = 24.0;
const BLOCK_PADDING: f32 = 8.0;
const BOARD_OFFSET_Y: f32 = 50.0;
const BOARD_OFFSET_X: f32 = (GAME_WIDTH - (COLS as f32 * (BLOCK_WIDTH + BLOCK_PADDING))) / 2.0;

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
        let mut blocks = Vec::new();
        let colors = ["#ff5555", "#ffaa00", "#55ff55", "#5555ff", "#aa00ff"];
        
        for row in 0..ROWS {
            for col in 0..COLS {
                blocks.push(Block {
                    x: BOARD_OFFSET_X + col as f32 * (BLOCK_WIDTH + BLOCK_PADDING),
                    y: BOARD_OFFSET_Y + row as f32 * (BLOCK_HEIGHT + BLOCK_PADDING),
                    alive: true,
                    color: colors[row % colors.len()].to_string(),
                });
            }
        }

        Self {
            paddle_x: GAME_WIDTH / 2.0 - PADDLE_WIDTH / 2.0,
            ball_x: GAME_WIDTH / 2.0 - BALL_SIZE / 2.0,
            ball_y: PADDLE_Y - BALL_SIZE - 2.0,
            ball_dx: INITIAL_BALL_SPEED * 0.707, // 45 degrees up-right
            ball_dy: -INITIAL_BALL_SPEED * 0.707,
            blocks,
            particles: Vec::new(),
            keys_held: HashSet::new(),
            last_tick: xerune::runtime::time::Instant::now(),
            game_over: false,
            won: false,
            paddle_y: PADDLE_Y,
            paddle_w: PADDLE_WIDTH,
            paddle_h: PADDLE_HEIGHT,
            ball_s: BALL_SIZE,
            block_w: BLOCK_WIDTH,
            block_h: BLOCK_HEIGHT,
            game_width: GAME_WIDTH,
            game_height: GAME_HEIGHT,
        }
    }
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

    fn update(&mut self, msg: Self::Message, _context: &mut xerune::Context) {
        match msg {
            Msg::Tick => {
                let now = xerune::runtime::time::Instant::now();
                let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.1);
                self.last_tick = now;

                if self.game_over || self.won { return; }

                // --- Paddle Movement ---
                let mut paddle_dir = 0.0;
                if self.keys_held.contains("ArrowLeft") 
                    || self.keys_held.contains("KEY_PREVIOUSSONG") 
                    || self.keys_held.contains("KEY_LEFT") 
                    || self.keys_held.contains("KEY_VOLUMEDOWN") 
                { 
                    paddle_dir -= 1.0; 
                }
                if self.keys_held.contains("ArrowRight") 
                    || self.keys_held.contains("KEY_NEXTSONG") 
                    || self.keys_held.contains("KEY_RIGHT") 
                    || self.keys_held.contains("KEY_VOLUMEUP") 
                { 
                    paddle_dir += 1.0; 
                }

                self.paddle_x += paddle_dir * PADDLE_SPEED * dt;
                self.paddle_x = self.paddle_x.clamp(0.0, GAME_WIDTH - PADDLE_WIDTH);

                // --- Ball Movement ---
                self.ball_x += self.ball_dx * dt;
                self.ball_y += self.ball_dy * dt;

                // --- Wall Collisions ---
                if self.ball_x <= 0.0 {
                    self.ball_x = 0.0;
                    self.ball_dx *= -1.0;
                } else if self.ball_x >= GAME_WIDTH - BALL_SIZE {
                    self.ball_x = GAME_WIDTH - BALL_SIZE;
                    self.ball_dx *= -1.0;
                }

                if self.ball_y <= 0.0 {
                    self.ball_y = 0.0;
                    self.ball_dy *= -1.0;
                } else if self.ball_y >= GAME_HEIGHT {
                    self.game_over = true;
                }

                // --- Paddle Collision ---
                if self.ball_y + BALL_SIZE >= PADDLE_Y 
                    && self.ball_y <= PADDLE_Y + PADDLE_HEIGHT 
                    && self.ball_x + BALL_SIZE >= self.paddle_x 
                    && self.ball_x <= self.paddle_x + PADDLE_WIDTH 
                    && self.ball_dy > 0.0
                {
                    self.ball_y = PADDLE_Y - BALL_SIZE;
                    
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
                
                for block in self.blocks.iter_mut() {
                    if !block.alive { continue; }

                    if self.ball_x + BALL_SIZE >= block.x 
                        && self.ball_x <= block.x + BLOCK_WIDTH 
                        && self.ball_y + BALL_SIZE >= block.y 
                        && self.ball_y <= block.y + BLOCK_HEIGHT 
                    {
                        block.alive = false;
                        hit_block = true;

                        // Spawn particles using custom LcgRng
                        for _ in 0..10 {
                            let angle = rng.gen_range(0.0, PI * 2.0);
                            let speed = rng.gen_range(50.0, 150.0);
                            self.particles.push(Particle {
                                x: block.x + BLOCK_WIDTH / 2.0,
                                y: block.y + BLOCK_HEIGHT / 2.0,
                                dx: angle.cos() * speed,
                                dy: angle.sin() * speed,
                                life: 1.0,
                                color: block.color.clone(),
                            });
                        }

                        let overlap_left = (self.ball_x + BALL_SIZE) - block.x;
                        let overlap_right = (block.x + BLOCK_WIDTH) - self.ball_x;
                        let overlap_top = (self.ball_y + BALL_SIZE) - block.y;
                        let overlap_bottom = (block.y + BLOCK_HEIGHT) - self.ball_y;

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
            },
            Msg::KeyDown(key) => {
                if self.game_over || self.won {
                    if matches!(key.as_str(), "KEY_PLAYPAUSE" | "KEY_UP" | "Space" | "Enter" | "KEY_NEXTSONG" | "KEY_PREVIOUSSONG") {
                        *self = BreakoutModel::default();
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
pub fn run_native(render_frame: impl FnMut(&mut Runtime<BreakoutModel, Measurer>, &mut [u32], u32, u32) + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = BreakoutModel::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let mut runtime = Runtime::new(model, measurer);
    runtime.set_interval("tick".to_string(), 16);

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
