use xerune::{Model, Runtime};
use fontdue::Font;
use askama::Template;
use tiny_skia::{PixmapMut, Paint, Color, Transform, Rect};
use rand::Rng;

mod support;

#[derive(Debug, Clone)]
struct RowData {
    id: String,
    name: String,
    status: String,
}

#[derive(Template)]
#[template(path = "showcase.html")]
struct ShowcaseTemplate<'a> {
    system_load_value: f32,
    table_data: &'a [RowData],
    user_counter: i32,
}

struct ShowcaseModel {
    system_load_value: f32,
    table_data: Vec<RowData>,
    user_counter: i32,
}

#[derive(Debug, Clone)]
enum ShowcaseMsg {
    IncrementProgress,
    IncrementUserCounter,
    Tick,
}

impl std::str::FromStr for ShowcaseMsg {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "increment_progress" => Ok(ShowcaseMsg::IncrementProgress),
            "increment_user_counter" => Ok(ShowcaseMsg::IncrementUserCounter),
            "tick" => Ok(ShowcaseMsg::Tick),
            _ => Err(()),
        }
    }
}

impl Model for ShowcaseModel {
    type Message = ShowcaseMsg;

    fn view(&self) -> String {
        let template = ShowcaseTemplate {
            system_load_value: self.system_load_value,
            table_data: &self.table_data,
            user_counter: self.user_counter,
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
        match msg {
            ShowcaseMsg::IncrementProgress => {
                self.system_load_value += 10.0;
                if self.system_load_value > 100.0 {
                    self.system_load_value = 0.0;
                }
            },
            ShowcaseMsg::IncrementUserCounter => {
                self.user_counter += 1;
            },
            ShowcaseMsg::Tick => {
                 // Update simulated CPU loads
                 let mut cpu_loads = vec![0.0; 4];

                 for load in cpu_loads.iter_mut() {
                     let noise = rand::thread_rng().gen_range(-10.0..10.0);
                     
                     *load = (self.system_load_value + noise).clamp(0.0, 100.0);
                 }

                 if let Some(canvas) = context.canvas_mut("load_chart") {
                     let w = canvas.width;
                     let h = canvas.height;
                     
                     if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, w, h) {
                         pixmap.fill(Color::from_rgba8(0, 0, 0, 0)); // Transparent background

                         let num_cores = cpu_loads.len();
                         let padding_top = 10.0;
                         let padding_bottom = 10.0;
                         let padding_left = 60.0; // Space for "CPU X"
                         let padding_right = 20.0;
                         
                         let total_h = h as f32 - padding_top - padding_bottom;
                         let bar_gap = 10.0;
                         let bar_height = (total_h - (bar_gap * (num_cores as f32 - 1.0))) / num_cores as f32;
                         
                         for (i, load) in cpu_loads.iter().enumerate() {
                             let y = padding_top + i as f32 * (bar_height + bar_gap);
                             
                             // Draw "CPU {i}" label dots
                             let mut text_paint = Paint::default();
                             text_paint.set_color_rgba8(180, 180, 180, 255);
                             let dot_size = 3.0;
                             for d in 0..(i+1).min(4) {
                                  let dot_rect = Rect::from_xywh(15.0 + (d as f32 * 6.0), y + bar_height/2.0 - 1.5, dot_size, dot_size).unwrap();
                                  pixmap.fill_rect(dot_rect, &text_paint, Transform::identity(), None);
                             }

                             // Draw stepped bars
                             // 20 steps total (10% = 2 steps)
                             let steps = 40; 
                             let track_width = w as f32 - padding_left - padding_right;
                             let step_gap = 2.0;
                             let step_width = (track_width - (step_gap * (steps as f32 - 1.0))) / steps as f32;
                             
                             let active_steps = (*load / 100.0 * steps as f32).round() as usize;
                             
                             for s in 0..steps {
                                 let sx = padding_left + s as f32 * (step_width + step_gap);
                                 let step_rect = Rect::from_xywh(sx, y, step_width, bar_height).unwrap();
                                 let mut paint = Paint::default();
                                 
                                 if s < active_steps {
                                     // Htop colors: Green -> Cyan -> Blue -> Purple/Red
                                     // Let's do: Green -> Cyan -> Orange -> Red
                                     let pct = s as f32 / steps as f32;
                                     if pct < 0.4 {
                                         paint.set_color_rgba8(20, 220, 20, 255); // Green
                                     } else if pct < 0.6 {
                                          paint.set_color_rgba8(20, 220, 220, 255); // Cyan
                                     } else if pct < 0.8 {
                                          paint.set_color_rgba8(220, 160, 20, 255); // Orange
                                     } else {
                                          paint.set_color_rgba8(220, 40, 40, 255); // Red
                                     }
                                 } else {
                                     // Empty track color
                                     paint.set_color_rgba8(40, 45, 50, 255); 
                                 }
                                 
                                 pixmap.fill_rect(step_rect, &paint, Transform::identity(), None);
                             }
                             
                             // Optional: Draw percentage text logic would go here if we had text rendering on canvas
                         }
                     }
                     
                     canvas.dirty = true;
                 }
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

    let measurer = skia_renderer::TinySkiaMeasurer { fonts: fonts_ref };

    let model = ShowcaseModel {
        system_load_value: 30.0,
        table_data: vec![
            RowData { id: "001".into(), name: "System Core".into(), status: "Online".into() },
            RowData { id: "002".into(), name: "Render Engine".into(), status: "Active".into() },
            RowData { id: "003".into(), name: "Network".into(), status: "Idle".into() },
            RowData { id: "004".into(), name: "Storage".into(), status: "Checking".into() },
            RowData { id: "005".into(), name: "Audio".into(), status: "Muted".into() },
        ],
        user_counter: 1234,
    };

    let runtime = Runtime::new(model, measurer);

    support::winit_backend::run_app(
        "Xerune Showcase",
        900,
        900,
        runtime,
        fonts_ref,
        move |proxy| {
            std::thread::spawn(move || {
                loop {
                    let _ = proxy.send_event("tick".to_string());
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            });
        }
    )?;

    Ok(())
}
