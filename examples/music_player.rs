use askama::Template;
// use taffy::prelude::*;
use fontdue::Font;
use tiny_skia::{Pixmap, Color};
use std::num::NonZeroU32;
use std::rc::Rc;
use winit::event::{Event, WindowEvent, ElementState, MouseButton};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use serde::Deserialize;
use std::fs;
// use std::path::Path;

use std::time::{Duration, Instant};

// Import from the library and renderer
use xerune::{Model, InputEvent, Runtime};
use skia_renderer::{TinySkiaRenderer, TinySkiaMeasurer};

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
    show_player: bool,
    elapsed_time: String,
    total_time: String,
    progress: f32,
}

struct MusicPlayerModel {
    tracks: Vec<Track>,
    current_track_index: Option<usize>,
    is_playing: bool,
    elapsed_seconds: u64,
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
        }
    }

    fn format_time(seconds: u64) -> String {
        let min = seconds / 60;
        let sec = seconds % 60;
        format!("{}:{:02}", min, sec)
    }
}

impl Model for MusicPlayerModel {
    fn view(&self) -> String {
        let dummy_track = &self.tracks[0]; 
        let current = self.current_track_index.map(|i| &self.tracks[i]).unwrap_or(dummy_track);
        let duration = current.duration_seconds();
        
        let template = MusicPlayerTemplate {
            tracks: &self.tracks,
            current_track: current,
            is_playing: self.is_playing,
            show_player: self.current_track_index.is_some(),
            elapsed_time: Self::format_time(self.elapsed_seconds),
            total_time: current.duration.clone(),
            progress: if duration > 0 { self.elapsed_seconds as f32 / duration as f32 } else { 0.0 },
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: &str) {
        if let Some(id_str) = msg.strip_prefix("select_track:") {
             if let Some(index) = self.tracks.iter().position(|t| t.id == id_str) {
                 self.current_track_index = Some(index);
                 self.is_playing = true;
                 self.elapsed_seconds = 0;
             }
        } else {
             match msg {
                 "back" => {
                     // Back now acts as minimized or just list view 
                     // User said "Selecting music brings user to player, stopping music brings them back"
                     // So specific "Stop" button brings back.
                     // "Back" button on player functionality
                     // Let's make "back" go back to list, keep playing.
                     // And "stop" go back to list, stop playing.
                     self.current_track_index = None;
                 },
                 "stop" => {
                     self.is_playing = false;
                     self.elapsed_seconds = 0;
                     self.current_track_index = None;
                 },
                 "play_pause" => {
                     self.is_playing = !self.is_playing;
                 },
                 "next" => {
                     if let Some(mut idx) = self.current_track_index {
                         idx = (idx + 1) % self.tracks.len();
                         self.current_track_index = Some(idx);
                         self.elapsed_seconds = 0;
                         // Keep playing state
                     }
                 },
                 "prev" => {
                      if let Some(mut idx) = self.current_track_index {
                         if idx > 0 {
                             idx -= 1;
                         } else {
                             idx = self.tracks.len() - 1;
                         }
                         self.current_track_index = Some(idx);
                         self.elapsed_seconds = 0;
                     }
                 },
                 "tick" => {
                     if self.is_playing {
                         if let Some(idx) = self.current_track_index {
                             let duration = self.tracks[idx].duration_seconds();
                             if self.elapsed_seconds < duration {
                                 self.elapsed_seconds += 1;
                             } else {
                                 // Auto next
                                 self.update("next"); 
                             }
                         }
                     }
                 }
                 _ => {}
             }
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let window = Rc::new(WindowBuilder::new()
        .with_title("RMTUI Music Player")
        .with_inner_size(winit::dpi::LogicalSize::new(800.0, 480.0))
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
    // Leak fonts to get 'static lifetime for the closure
    let fonts_ref: &'static [Font] = Box::leak(fonts.into_boxed_slice());

    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    let model = MusicPlayerModel::new();
    let mut runtime = Runtime::new(model, measurer);

    let mut cursor_position = (0.0, 0.0);
    
    // Initial compute
    runtime.set_size(800.0, 480.0);

    let window_clone = window.clone();
    let mut next_tick = Instant::now() + Duration::from_secs(1);

    event_loop.run(move |event, target| {
        target.set_control_flow(ControlFlow::WaitUntil(next_tick));

        match event {
            Event::NewEvents(winit::event::StartCause::ResumeTimeReached { .. }) => {
                 if runtime.handle_event(InputEvent::Tick) {
                     window_clone.request_redraw();
                 }
                 next_tick = Instant::now() + Duration::from_secs(1);
                 target.set_control_flow(ControlFlow::WaitUntil(next_tick));
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
                
                // Re-compute layout with window size constraint
                 runtime.set_size(width as f32, height as f32);

                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::from_rgba8(18, 18, 18, 255)); // Dark background

                let mut renderer = TinySkiaRenderer::new(&mut pixmap, fonts_ref);
                runtime.render(&mut renderer);

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
           Event::WindowEvent { window_id, event: WindowEvent::CursorMoved { position, .. } } if window_id == window_clone.id() => {
               cursor_position = (position.x as f32, position.y as f32);
            },
            Event::WindowEvent { window_id, event: WindowEvent::MouseInput { state, button, .. } } if window_id == window_clone.id() => {
                if state == ElementState::Pressed && button == MouseButton::Left {
                     let (x, y) = cursor_position;
                     if runtime.handle_event(InputEvent::Click { x, y }) {
                         window_clone.request_redraw();
                     }
                }
            },
            Event::WindowEvent { window_id, event: WindowEvent::MouseWheel { delta, .. } } if window_id == window_clone.id() => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => (x * 30.0, y * 30.0),
                    winit::event::MouseScrollDelta::PixelDelta(p) => (p.x as f32, p.y as f32),
                };
                
                let (cx, cy) = cursor_position;
                if runtime.handle_event(InputEvent::Scroll { x: cx, y: cy, delta_x: dx, delta_y: dy }) {
                    window_clone.request_redraw();
                }
            },
             _ => {}
        }
    }).unwrap();
}

