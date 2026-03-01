use askama::Template;
use fontdue::Font;
use serde::Deserialize;
use std::fs;
use std::time::{Duration, Instant};

// Import from the library and renderer
use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;
use tiny_skia::{PixmapMut, Color, Paint, Rect, Transform, PathBuilder, FillRule};
use rand::Rng;

#[path = "support/mod.rs"]
mod support;

#[cfg(feature = "profile")]
use coarse_prof::profile;


#[derive(Debug, Deserialize, Clone)]
struct Track {
    id: String,
    title: String,
    artist: String,
    album: String,
    duration: String,
    cover_url: String,
}

impl Track {
    fn duration_seconds(&self) -> u64 {
        let parts: Vec<&str> = self.duration.split(':').collect();
        if parts.len() == 2 {
            let min: u64 = parts[0].parse().unwrap_or(0);
            let sec: u64 = parts[1].parse().unwrap_or(0);
            min * 60 + sec
        } else {
            0
        }
    }
}

#[derive(Template)]
#[template(path = "music_player.html")]
struct MusicPlayerTemplate<'a> {
    tracks: &'a [Track],
    current_track: &'a Track,
    is_playing: bool,
    elapsed_time: String,
    total_time: String,
    progress: f32,
    list_x: f32,
    player_x: f32,
    hovered_track: String,
}

struct MusicPlayerModel {
    tracks: Vec<Track>,
    current_track_index: Option<usize>,
    is_playing: bool,
    elapsed_seconds: u64,
    last_tick: Instant,
    visualizer_data: Vec<f32>,
    transition_progress: f32,
    hovered_track: String,
}

impl MusicPlayerModel {
    fn new() -> Self {
        // Load tracks from JSON
        let json_content = fs::read_to_string("resources/music_player/music.json")
            .expect("Failed to read music.json");
        let tracks: Vec<Track> = serde_json::from_str(&json_content)
            .expect("Failed to parse music.json");

        Self {
            tracks,
            current_track_index: None,
            is_playing: false,
            elapsed_seconds: 0,
            last_tick: Instant::now(),
            visualizer_data: vec![10.0; 30], // 30 bars
            transition_progress: 0.0,
            hovered_track: String::new(),
        }
    }

    fn format_time(seconds: u64) -> String {
        let min = seconds / 60;
        let sec = seconds % 60;
        format!("{}:{:02}", min, sec)
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    SelectTrack(String),
    Back,
    Stop,
    PlayPause,
    Next,
    Prev,
    Tick,
    HoverTrack(String),
    UnhoverTrack,
}

impl std::str::FromStr for Msg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
         if let Some(id_str) = s.strip_prefix("select_track:") {
             return Ok(Msg::SelectTrack(id_str.to_string()));
         }
         if let Some(id_str) = s.strip_prefix("hover_track:") {
             return Ok(Msg::HoverTrack(id_str.to_string()));
         }
         match s {
             "unhover_track" => Ok(Msg::UnhoverTrack),
             "back" => Ok(Msg::Back),
             "stop" => Ok(Msg::Stop),
             "play_pause" => Ok(Msg::PlayPause),
             "next" => Ok(Msg::Next),
             "prev" => Ok(Msg::Prev),
             "tick" => Ok(Msg::Tick),
             _ => Err(()),
         }
    }
}

impl Model for MusicPlayerModel {
    type Message = Msg;

    fn view(&self) -> String {
        let dummy_track = &self.tracks[0]; 
        let current = self.current_track_index.map(|i| &self.tracks[i]).unwrap_or(dummy_track);
        let duration = current.duration_seconds();
        
        // Easing function: smoothstep
        let p = self.transition_progress;
        let t = p * p * (3.0 - 2.0 * p);
        
        let template = MusicPlayerTemplate {
            tracks: &self.tracks,
            current_track: current,
            is_playing: self.is_playing,
            elapsed_time: Self::format_time(self.elapsed_seconds),
            total_time: current.duration.clone(),
            progress: if duration > 0 { self.elapsed_seconds as f32 / duration as f32 } else { 0.0 },
            list_x: -t * 800.0,
            player_x: 800.0 - (t * 800.0),
            hovered_track: self.hovered_track.clone(),
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
         match msg {
             Msg::SelectTrack(id_str) => {
                 if let Some(index) = self.tracks.iter().position(|t| t.id == id_str) {
                     self.current_track_index = Some(index);
                     self.is_playing = true;
                     self.elapsed_seconds = 0;
                     self.last_tick = Instant::now();
                 }
             },
             Msg::Back => {
                 self.current_track_index = None;
             },
             Msg::Stop => {
                 self.is_playing = false;
                 self.elapsed_seconds = 0;
                 self.current_track_index = None;
             },
             Msg::PlayPause => {
                 self.is_playing = !self.is_playing;
                 if self.is_playing {
                     self.last_tick = Instant::now();
                 }
             },
             Msg::Next => {
                 if let Some(mut idx) = self.current_track_index {
                     idx = (idx + 1) % self.tracks.len();
                     self.current_track_index = Some(idx);
                     self.elapsed_seconds = 0;
                     self.last_tick = Instant::now();
                 }
             },
             Msg::Prev => {
                  if let Some(mut idx) = self.current_track_index {
                     if idx > 0 {
                         idx -= 1;
                     } else {
                         idx = self.tracks.len() - 1;
                     }
                     self.current_track_index = Some(idx);
                     self.elapsed_seconds = 0;
                     self.last_tick = Instant::now();
                 }
             },
             Msg::HoverTrack(id_str) => {
                 self.hovered_track = id_str;
             },
             Msg::UnhoverTrack => {
                 self.hovered_track.clear();
             },
             Msg::Tick => {
                 // Transition animation
                 let target = if self.current_track_index.is_some() { 1.0 } else { 0.0 };
                 if self.transition_progress < target {
                     self.transition_progress = (self.transition_progress + 0.1).min(1.0);
                 } else if self.transition_progress > target {
                     self.transition_progress = (self.transition_progress - 0.1).max(0.0);
                 }

                 // Update visualizer
                 if self.is_playing {
                     let mut rng = rand::thread_rng();
                     for val in self.visualizer_data.iter_mut() {
                        let change = rng.gen_range(-5.0..5.0);
                        *val = (*val + change).clamp(5.0, 50.0);
                     }
                 } else {
                     // Decay
                     for val in self.visualizer_data.iter_mut() {
                         *val = (*val * 0.9).max(2.0);
                     }
                 }

                 // Draw Visualizer
                 if let Some(canvas) = context.canvas_mut("visualizer") {
                     let w = canvas.width as f32;
                     let h = canvas.height as f32;
                     
                     // Create a PixmapMut wrapping the canvas data
                     // Canvas data is RGBA u8
                     if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, canvas.width, canvas.height) {
                         pixmap.fill(Color::TRANSPARENT);
                         
                         let bars = self.visualizer_data.len();
                         let gap = 4.0;
                         let bar_width = (w - (bars as f32 - 1.0) * gap) / bars as f32;
                         
                         let mut paint = Paint::default();
                         
                         // Create linear gradient
                         let gradient = tiny_skia::LinearGradient::new(
                             tiny_skia::Point::from_xy(0.0, 0.0),
                             tiny_skia::Point::from_xy(0.0, h),
                             vec![
                                 tiny_skia::GradientStop::new(0.0, tiny_skia::Color::from_rgba8(30, 215, 96, 255)), // Green
                                 tiny_skia::GradientStop::new(1.0, tiny_skia::Color::from_rgba8(10, 100, 200, 200)), // Darker/Transparent Blue
                             ],
                             tiny_skia::SpreadMode::Pad,
                             Transform::identity(),
                         ).unwrap();
                         
                         paint.shader = gradient;
                         
                         for (i, &height) in self.visualizer_data.iter().enumerate() {
                             let x = i as f32 * (bar_width + gap);
                             let y = h - height;
                             
                             if let Some(rect) = Rect::from_xywh(x, y, bar_width, height) {
                                 if let Some(path) = rounded_rect_path(rect, 4.0) {
                                     pixmap.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
                                 }
                             }
                         }
                         canvas.dirty = true;
                     }
                 }

                 if self.is_playing {
                     if self.last_tick.elapsed() >= Duration::from_secs(1) {
                         if let Some(idx) = self.current_track_index {
                             let duration = self.tracks[idx].duration_seconds();
                             if self.elapsed_seconds < duration {
                                 self.elapsed_seconds += 1;
                                 self.last_tick = Instant::now();
                             } else {
                                 // Auto next
                                 self.update(Msg::Next, context); 
                             }
                         }
                     }
                 }
                 
                 #[cfg(feature = "profile")]
                 {
                     static mut TICK_COUNT: usize = 0;
                     unsafe {
                         TICK_COUNT += 1;
                         if TICK_COUNT % 100 == 0 {
                             coarse_prof::write(&mut std::io::stdout()).unwrap();
                             println!("--------------------------------------------------");
                         }
                     }
                 }
             }
         }
    }
}

fn rounded_rect_path(rect: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    
    // Clamp radius to ensure it doesn't exceed half the rectangle's dimensions
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0).max(0.0);
    
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    
    // The factor for approximating a circle quadrant with a cubic Bezier curve.
    let bezier_circle_factor = 0.55228475; // (4.0 / 3.0) * (std::f32::consts::PI / 8.0).tan();
    let handle_offset = r * bezier_circle_factor;
    
    let left = rect.x();
    let top = rect.y();
    let right = rect.x() + rect.width();
    let bottom = rect.y() + rect.height();

    // Start at the top edge, just after the top-left corner
    pb.move_to(left + r, top);
    
    // Top edge
    pb.line_to(right - r, top);
    
    // Top-right corner
    pb.cubic_to(
        right - r + handle_offset, top,            // Control point 1
        right, top + r - handle_offset,            // Control point 2
        right, top + r                             // End point
    );
    
    // Right edge
    pb.line_to(right, bottom - r);
    
    // Bottom-right corner
    pb.cubic_to(
        right, bottom - r + handle_offset,
        right - r + handle_offset, bottom,
        right - r, bottom
    );
    
    // Bottom edge
    pb.line_to(left + r, bottom);
    
    // Bottom-left corner
    pb.cubic_to(
        left + r - handle_offset, bottom,
        left, bottom - r + handle_offset,
        left, bottom - r
    );
    
    // Left edge
    pb.line_to(left, top + r);
    
    // Top-left corner
    pb.cubic_to(
        left, top + r - handle_offset,
        left + r - handle_offset, top,
        left + r, top
    );
    
    pb.close();
    pb.finish()
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
    let model = MusicPlayerModel::new();
    let runtime = Runtime::new(model, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app(
            "Xerune Music Player", 
            800, 
            480, 
            runtime, 
            fonts_ref, 
            move |proxy| {
                std::thread::spawn(move || {
                     loop {
                         let _ = proxy.send_event("tick".to_string());
                         std::thread::sleep(std::time::Duration::from_millis(33));
                     }
                });
            }
        )
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
         support::linux_backend::run_app(
             "Xerune Music Player", 
             800, 
             480, 
             runtime, 
             fonts_ref, 
             |_| {}
         )
    }
}
