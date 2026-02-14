use askama::Template;
use fontdue::Font;
use serde::Deserialize;
use std::fs;
use std::time::{Duration, Instant};

// Import from the library and renderer
use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

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
    last_tick: Instant,
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
}

impl std::str::FromStr for Msg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
         if let Some(id_str) = s.strip_prefix("select_track:") {
             return Ok(Msg::SelectTrack(id_str.to_string()));
         }
         match s {
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
             Msg::Tick => {
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
    let model = MusicPlayerModel::new();
    let runtime = Runtime::new(model, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app(
            "RMTUI Music Player", 
            800, 
            480, 
            runtime, 
            fonts_ref, 
            move |proxy| {
                std::thread::spawn(move || {
                     loop {
                         let _ = proxy.send_event("tick".to_string());
                         std::thread::sleep(std::time::Duration::from_secs(1));
                     }
                });
            }
        )
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
         support::linux_backend::run_app(
             "RMTUI Music Player", 
             800, 
             480, 
             runtime, 
             fonts_ref, 
             |_| {}
         )
    }
}
