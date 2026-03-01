use askama::Template;
use fontdue::Font;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;
use std::f32::consts::PI;

#[path = "support/mod.rs"]
mod support;

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

#[derive(Clone, Debug)]
struct Block {
    x: f32,
    y: f32,
    alive: bool,
    color: String,
}

#[derive(Clone, Debug)]
struct Particle {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    life: f32, // 1.0 down to 0.0
    color: String,
}

struct BreakoutModel {
    paddle_x: f32,
    ball_x: f32,
    ball_y: f32,
    ball_dx: f32,
    ball_dy: f32,
    blocks: Vec<Block>,
    particles: Vec<Particle>,
    keys_held: HashSet<String>,
    last_tick: Instant,
    game_over: bool,
    won: bool,
}

#[derive(Template)]
#[template(path = "breakout.html")]
struct BreakoutTemplate<'a> {
    paddle_x: f32,
    paddle_y: f32,
    paddle_w: f32,
    paddle_h: f32,
    ball_x: f32,
    ball_y: f32,
    ball_s: f32,
    blocks: &'a [Block],
    block_w: f32,
    block_h: f32,
    particles: &'a [Particle],
    game_width: f32,
    game_height: f32,
    game_over: bool,
    won: bool,
}

impl BreakoutModel {
    fn new() -> Self {
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
            last_tick: Instant::now(),
            game_over: false,
            won: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Tick,
    KeyDown(String),
    KeyUp(String),
}

impl std::str::FromStr for Msg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(key) = s.strip_prefix("keydown:") {
            return Ok(Msg::KeyDown(key.to_string()));
        }
        if let Some(key) = s.strip_prefix("keyup:") {
            return Ok(Msg::KeyUp(key.to_string()));
        }
        match s {
            "tick" => Ok(Msg::Tick),
            _ => Err(()),
        }
    }
}

impl Model for BreakoutModel {
    type Message = Msg;

    fn view(&self) -> String {
        let template = BreakoutTemplate {
            paddle_x: self.paddle_x,
            paddle_y: PADDLE_Y,
            paddle_w: PADDLE_WIDTH,
            paddle_h: PADDLE_HEIGHT,
            ball_x: self.ball_x,
            ball_y: self.ball_y,
            ball_s: BALL_SIZE,
            blocks: &self.blocks,
            block_w: BLOCK_WIDTH,
            block_h: BLOCK_HEIGHT,
            particles: &self.particles,
            game_width: GAME_WIDTH,
            game_height: GAME_HEIGHT,
            game_over: self.game_over,
            won: self.won,
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: Self::Message, _context: &mut xerune::Context) {
        match msg {
            Msg::Tick => {
                let now = Instant::now();
                let dt = now.duration_since(self.last_tick).as_secs_f32();
                self.last_tick = now;

                if self.game_over || self.won { return; }

                // --- Paddle Movement ---
                let mut paddle_dir = 0.0;
                if self.keys_held.contains("ArrowLeft") { paddle_dir -= 1.0; }
                if self.keys_held.contains("ArrowRight") { paddle_dir += 1.0; }

                self.paddle_x += paddle_dir * PADDLE_SPEED * dt;
                self.paddle_x = self.paddle_x.clamp(0.0, GAME_WIDTH - PADDLE_WIDTH);

                // --- Ball Movement ---
                self.ball_x += self.ball_dx * dt;
                self.ball_y += self.ball_dy * dt;

                // --- Wall Collisions ---
                if self.ball_x <= 0.0 {
                    self.ball_x = 0.0;
                    self.ball_dx *= -1.0; // Bounce left wall
                } else if self.ball_x >= GAME_WIDTH - BALL_SIZE {
                    self.ball_x = GAME_WIDTH - BALL_SIZE;
                    self.ball_dx *= -1.0; // Bounce right wall
                }

                if self.ball_y <= 0.0 {
                    self.ball_y = 0.0;
                    self.ball_dy *= -1.0; // Bounce top wall
                } else if self.ball_y >= GAME_HEIGHT {
                    // Ball fell through the bottom
                    self.game_over = true;
                }

                // --- Paddle Collision ---
                if self.ball_y + BALL_SIZE >= PADDLE_Y 
                    && self.ball_y <= PADDLE_Y + PADDLE_HEIGHT 
                    && self.ball_x + BALL_SIZE >= self.paddle_x 
                    && self.ball_x <= self.paddle_x + PADDLE_WIDTH 
                    && self.ball_dy > 0.0 // Only if ball is heading down
                {
                    self.ball_y = PADDLE_Y - BALL_SIZE; // Push ball out of paddle
                    
                    // Change angle based on where it hit the paddle
                    let hit_factor = ((self.ball_x + BALL_SIZE / 2.0) - (self.paddle_x + PADDLE_WIDTH / 2.0)) / (PADDLE_WIDTH / 2.0);
                    // hit_factor is -1.0 (left edge) to 1.0 (right edge)
                    
                    let speed = (self.ball_dx * self.ball_dx + self.ball_dy * self.ball_dy).sqrt();
                    // Max bounce angle is 60 degrees (PI/3)
                    let max_angle = PI / 3.0; 
                    let bounce_angle = hit_factor * max_angle;
                    
                    self.ball_dx = speed * bounce_angle.sin();
                    self.ball_dy = -speed * bounce_angle.cos();
                }

                // --- Block Collisions ---
                let mut hit_block = false;
                for block in self.blocks.iter_mut() {
                    if !block.alive { continue; }

                    if self.ball_x + BALL_SIZE >= block.x 
                        && self.ball_x <= block.x + BLOCK_WIDTH 
                        && self.ball_y + BALL_SIZE >= block.y 
                        && self.ball_y <= block.y + BLOCK_HEIGHT 
                    {
                        // Collision!
                        block.alive = false;
                        hit_block = true;

                        // Spawn particles
                        use rand::Rng;
                        let mut rng = rand::thread_rng();
                        for _ in 0..10 {
                            let angle = rng.gen_range(0.0..PI * 2.0);
                            let speed = rng.gen_range(50.0..150.0);
                            self.particles.push(Particle {
                                x: block.x + BLOCK_WIDTH / 2.0,
                                y: block.y + BLOCK_HEIGHT / 2.0,
                                dx: angle.cos() * speed,
                                dy: angle.sin() * speed,
                                life: 1.0,
                                color: block.color.clone(),
                            });
                        }

                        // Determine bounce direction based on overlap
                        // Very simple AABB response:
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
                        
                        break; // Only hit one block per frame to avoid weird multi-bounces
                    }
                }

                if hit_block {
                    // Check win condition
                    if self.blocks.iter().all(|b| !b.alive) {
                        self.won = true;
                    }
                }

                // --- Update Particles ---
                for particle in self.particles.iter_mut() {
                    particle.x += particle.dx * dt;
                    particle.y += particle.dy * dt;
                    particle.life -= 1.5 * dt; // Die off
                }
                self.particles.retain(|p| p.life > 0.0);
            },
            Msg::KeyDown(key) => {
                self.keys_held.insert(key);
            },
            Msg::KeyUp(key) => {
                self.keys_held.remove(&key);
            },
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
    let model = BreakoutModel::new();
    let runtime = Runtime::new(model, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app(
            "Xerune Breakout", 
            GAME_WIDTH as u32, 
            GAME_HEIGHT as u32, 
            runtime, 
            fonts_ref, 
            move |proxy| {
                std::thread::spawn(move || {
                     loop {
                         let _ = proxy.send_event("tick".to_string());
                         std::thread::sleep(std::time::Duration::from_millis(16)); // ~60fps
                     }
                });
            }
        )
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
         support::linux_backend::run_app(
             "Xerune Breakout", 
             GAME_WIDTH as u32, 
             GAME_HEIGHT as u32, 
             runtime, 
             fonts_ref, 
             |_| {}
         )
    }
}
