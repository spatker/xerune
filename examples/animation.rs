use askama::Template;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use std::time::Instant;
use xerune::{Model, InputEvent, Runtime};
use skia_renderer::{TinySkiaRenderer, TinySkiaMeasurer};

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

    fn update(&mut self, msg: &str) {
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

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new()
        .with_title("RMTUI Animation Benchmark")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 600.0))
        .build(&event_loop)
        .unwrap());

    let context = softbuffer::Context::new(&window).unwrap();
    let mut surface = softbuffer::Surface::new(&context, &window).unwrap();

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
    let mut runtime = Runtime::new(model, measurer);
    
    runtime.set_size(800.0, 600.0);

    let window_clone = window.clone();
    let mut last_render_time: Option<f32> = None;

    event_loop.run(move |event, target| {
         // Force high refresh rate by not waiting too long, but let's effectively poll for max speed test
         target.set_control_flow(ControlFlow::Poll);

        match event {
            Event::AboutToWait => {
                 // Update logic
                if runtime.handle_event(InputEvent::Tick { render_time_ms: last_render_time }) {
                    window_clone.request_redraw();
                }
            },
            Event::WindowEvent { window_id, event: WindowEvent::RedrawRequested } if window_id == window_clone.id() => {
                let size = window_clone.inner_size();
                let width = size.width;
                let height = size.height;
                
                 if width == 0 || height == 0 { return; }

                surface.resize(
                    NonZeroU32::new(width).unwrap(),
                    NonZeroU32::new(height).unwrap(),
                ).unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                
                 runtime.set_size(width as f32, height as f32);

                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::from_rgba8(34, 34, 34, 255)); 

                let mut renderer = TinySkiaRenderer::new(&mut pixmap, fonts_ref);
                let start_render = Instant::now();
                runtime.render(&mut renderer);
                last_render_time = Some(start_render.elapsed().as_secs_f32() * 1000.0);

                let data = pixmap.data();
                for (i, chunk) in data.chunks_exact(4).enumerate() {
                    let r = chunk[0] as u32;
                    let g = chunk[1] as u32;
                    let b = chunk[2] as u32;
                    buffer[i] = (r << 16) | (g << 8) | b;
                }
                
                buffer.present().unwrap();
            },
            Event::WindowEvent { window_id, event: WindowEvent::CloseRequested } if window_id == window_clone.id() => {
                 target.exit();
            },
            Event::WindowEvent { window_id, event: WindowEvent::CursorMoved { .. } } if window_id == window_clone.id() => {
               // cursor_position = (position.x as f32, position.y as f32);
            },
            _ => {}
        }
    }).unwrap();
}
