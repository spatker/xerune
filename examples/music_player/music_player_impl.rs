#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use serde::{Serialize, Deserialize};
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use xerune_render::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;


// Simple self-contained LCG random generator to guarantee 100% WASM compatibility
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub cover_url: String,
    /// Theme color for this track's player screen.
    #[serde(default = "default_accent")]
    pub accent: String,
}

fn default_width() -> f32 {
    800.0
}

fn default_accent() -> String {
    "#a78bfa".to_string()
}

impl Track {
    /// The accent mixed 22% into the page background, as an opaque color.
    /// The page background must be opaque: frames are drawn over the
    /// previous one, so a translucent base would leave ghost images.
    pub fn glow(&self) -> String {
        let hex = self.accent.trim_start_matches('#');
        let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32;
        let base = [0x11 as f32, 0x0e as f32, 0x1a as f32];
        let mix = |i: usize| (base[i] + (channel(i * 2) - base[i]) * 0.22).round() as u8;
        format!("#{:02x}{:02x}{:02x}", mix(0), mix(1), mix(2))
    }

    pub fn duration_seconds(&self) -> u64 {
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

#[derive(XeruneTemplate, Serialize, Deserialize, Clone, PartialEq)]
#[template(path = "music_player.html")]
pub struct MusicPlayerModel {
    pub tracks: Vec<Track>,
    pub current_track_index: Option<usize>,
    pub is_playing: bool,
    pub elapsed_seconds: u64,
    #[serde(skip, default = "xerune::runtime::time::Instant::now")]
    pub last_tick: xerune::runtime::time::Instant,
    pub visualizer_data: Vec<f32>,
    pub transition_progress: f32,
    pub hovered_track: String,
    pub active_list_index: usize,
    pub current_track: Track,
    pub elapsed_time: String,
    pub total_time: String,
    pub progress: f32,
    pub list_x: f32,
    pub player_x: f32,
    /// Viewport width; the list and player views slide by this much.
    #[serde(default = "default_width")]
    pub width: f32,
    #[serde(default)]
    pub tick_count: u64,
}

impl MusicPlayerModel {
    pub fn new(json_content: Option<&str>) -> Self {
        let tracks: Vec<Track> = if let Some(content) = json_content {
            serde_json::from_str(content).expect("Failed to parse music json")
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                let content = std::fs::read_to_string("examples/music_player/resources/music.json")
                    .or_else(|_| std::fs::read_to_string("resources/music_player/music.json"))
                    .expect("Failed to read music.json");
                serde_json::from_str(&content).expect("Failed to parse music json")
            }
            #[cfg(target_arch = "wasm32")]
            {
                Vec::new()
            }
        };

        let dummy_track = Track {
            id: "".to_string(),
            title: "".to_string(),
            artist: "".to_string(),
            album: "".to_string(),
            duration: "0:00".to_string(),
            cover_url: "".to_string(),
            accent: default_accent(),
        };
        let current_track = if tracks.is_empty() { dummy_track } else { tracks[0].clone() };

        Self {
            tracks,
            current_track_index: None,
            is_playing: false,
            elapsed_seconds: 0,
            last_tick: xerune::runtime::time::Instant::now(),
            visualizer_data: vec![10.0; 30], // 30 bars
            transition_progress: 0.0,
            hovered_track: String::new(),
            active_list_index: 0,
            current_track: current_track.clone(),
            elapsed_time: "0:00".to_string(),
            total_time: if current_track.id.is_empty() { "0:00".to_string() } else { current_track.duration.clone() },
            progress: 0.0,
            list_x: 0.0,
            player_x: default_width(),
            width: default_width(),
            tick_count: 0,
        }
    }

    pub fn total_minutes(&self) -> u64 {
        self.tracks.iter().map(Track::duration_seconds).sum::<u64>() / 60
    }

    pub fn progress_pct(&self) -> f32 {
        (self.progress * 100.0).clamp(0.0, 100.0)
    }

    /// Three bar heights (px) for the "now playing" marker in the list.
    pub fn mini_eq(&self) -> Vec<u32> {
        self.visualizer_data.iter().step_by(7).take(3).map(|v| (4.0 + v * 0.24) as u32).collect()
    }

    fn ensure_ticking(&self, context: &mut xerune::Context) {
        context.set_timeout("tick".to_string(), 33);
    }

    fn format_time(seconds: u64) -> String {
        let min = seconds / 60;
        let sec = seconds % 60;
        format!("{}:{:02}", min, sec)
    }

    fn update_derived_fields(&mut self) {
        if self.tracks.is_empty() { return; }
        let dummy_track = self.tracks[0].clone();
        let current = match self.current_track_index {
            Some(i) => &self.tracks[i],
            None if self.active_list_index < self.tracks.len() => &self.tracks[self.active_list_index],
            None => &dummy_track,
        };
        self.current_track = current.clone();
        let duration = current.duration_seconds();
        
        let p = self.transition_progress;
        let t = p * p * (3.0 - 2.0 * p);
        
        self.elapsed_time = Self::format_time(self.elapsed_seconds);
        self.total_time = current.duration.clone();
        self.progress = if duration > 0 { self.elapsed_seconds as f32 / duration as f32 } else { 0.0 };
        self.list_x = -t * self.width;
        self.player_x = self.width - (t * self.width);
    }
}

#[derive(Debug, Clone, PartialEq, XeruneMessage)]
pub enum Msg {
    #[xerune(prefix = "select_track:")]
    SelectTrack(String),
    Back,
    Stop,
    PlayPause,
    Next,
    Prev,
    Tick,
    #[xerune(prefix = "hover_track:")]
    HoverTrack(String),
    UnhoverTrack,
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
}

impl Model for MusicPlayerModel {
    type Message = Msg;

    fn on_resize(&mut self, width: f32, _height: f32, _context: &mut xerune::Context) {
        self.width = width;
        self.update_derived_fields();
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
         if self.tracks.is_empty() { return; }
         match msg {
             Msg::SelectTrack(id_str) => {
                 if let Some(index) = self.tracks.iter().position(|t| t.id == id_str) {
                     self.current_track_index = Some(index);
                     self.active_list_index = index;
                     self.is_playing = true;
                     self.elapsed_seconds = 0;
                     self.last_tick = xerune::runtime::time::Instant::now();
                     self.ensure_ticking(context);
                 }
             },
             Msg::Back => {
                 self.current_track_index = None;
                 self.ensure_ticking(context);
             },
             Msg::Stop => {
                 self.is_playing = false;
                 self.elapsed_seconds = 0;
                 self.current_track_index = None;
                 self.ensure_ticking(context);
             },
             Msg::PlayPause => {
                 if self.current_track_index.is_none() && !self.tracks.is_empty() {
                     self.current_track_index = Some(self.active_list_index);
                 }
                 self.is_playing = !self.is_playing;
                 if self.is_playing {
                     self.last_tick = xerune::runtime::time::Instant::now();
                     self.ensure_ticking(context);
                 }
             },
             Msg::Next => {
                 let next_idx = match self.current_track_index {
                     Some(idx) => (idx + 1) % self.tracks.len(),
                     None => self.active_list_index,
                 };
                 self.current_track_index = Some(next_idx);
                 self.active_list_index = next_idx;
                 self.is_playing = true;
                 self.elapsed_seconds = 0;
                 self.last_tick = xerune::runtime::time::Instant::now();
                 self.ensure_ticking(context);
             },
             Msg::Prev => {
                 let prev_idx = match self.current_track_index {
                     Some(idx) => if idx > 0 { idx - 1 } else { self.tracks.len() - 1 },
                     None => self.active_list_index,
                 };
                 self.current_track_index = Some(prev_idx);
                 self.active_list_index = prev_idx;
                 self.is_playing = true;
                 self.elapsed_seconds = 0;
                 self.last_tick = xerune::runtime::time::Instant::now();
                 self.ensure_ticking(context);
             },
             Msg::HoverTrack(id_str) => {
                 self.hovered_track = id_str;
             },
             Msg::UnhoverTrack => {
                 self.hovered_track.clear();
             },
             Msg::KeyDown(key) => {
                 match key.as_str() {
                     "Up" | "KEY_UP" | "ArrowUp" => {
                         if self.current_track_index.is_some() {
                             self.update(Msg::Back, context);
                         } else if self.active_list_index > 0 {
                             self.active_list_index -= 1;
                             context.scroll_into_view(&format!("select_track:{}", self.tracks[self.active_list_index].id));
                         }
                     }
                     "Down" | "KEY_DOWN" | "ArrowDown" => {
                         if self.current_track_index.is_none() && self.active_list_index + 1 < self.tracks.len() {
                             self.active_list_index += 1;
                             context.scroll_into_view(&format!("select_track:{}", self.tracks[self.active_list_index].id));
                         }
                     }
                     "PlayPause" | "KEY_PLAYPAUSE" | "MediaPlayPause" | "Enter" | "Return" | "NumpadEnter" | "Space" => {
                         if self.current_track_index.is_none() && !self.tracks.is_empty() {
                             let id = self.tracks[self.active_list_index].id.clone();
                             self.update(Msg::SelectTrack(id), context);
                         } else {
                             self.update(Msg::PlayPause, context);
                         }
                     }
                     "Next" | "KEY_NEXTSONG" | "MediaTrackNext" | "ArrowRight" => {
                         if self.current_track_index.is_none() && !self.tracks.is_empty() {
                             let id = self.tracks[self.active_list_index].id.clone();
                             self.update(Msg::SelectTrack(id), context);
                         } else {
                             self.update(Msg::Next, context);
                         }
                     }
                     "Prev" | "KEY_PREVIOUSSONG" | "MediaTrackPrevious" | "ArrowLeft" => {
                         if self.current_track_index.is_none() && !self.tracks.is_empty() {
                             let id = self.tracks[self.active_list_index].id.clone();
                             self.update(Msg::SelectTrack(id), context);
                         } else {
                             self.update(Msg::Prev, context);
                         }
                     }
                     "Back" | "KEY_SUSPEND" | "Escape" => {
                         self.update(Msg::Back, context);
                     }
                     _ => {}
                 }
             },
             Msg::Tick => {
                  self.tick_count = self.tick_count.wrapping_add(1);
                  // Transition animation
                  let target = if self.current_track_index.is_some() { 1.0 } else { 0.0 };
                  if self.transition_progress < target {
                      self.transition_progress = (self.transition_progress + 0.1).min(1.0);
                  } else if self.transition_progress > target {
                      self.transition_progress = (self.transition_progress - 0.1).max(0.0);
                  }
                  let is_animating = (self.transition_progress - target).abs() > 0.001;

                  // Update visualizer simulation using LCG Rng
                  if self.is_playing {
                      let mut rng = LcgRng::new(self.tick_count);
                      for val in self.visualizer_data.iter_mut() {
                         let change = rng.gen_range(-5.0, 5.0);
                         *val = (*val + change).clamp(5.0, 50.0);
                      }
                  } else {
                     // Decay
                     for val in self.visualizer_data.iter_mut() {
                         *val = (*val * 0.9).max(2.0);
                     }
                 }

                 if self.is_playing {
                     if self.last_tick.elapsed() >= core::time::Duration::from_secs(1) {
                         if let Some(idx) = self.current_track_index {
                             let duration = self.tracks[idx].duration_seconds();
                             if self.elapsed_seconds < duration {
                                 self.elapsed_seconds += 1;
                                 self.last_tick = xerune::runtime::time::Instant::now();
                             } else {
                                 self.update(Msg::Next, context); 
                             }
                         }
                     }
                 }

                  let is_decaying = self.visualizer_data.iter().any(|&val| val > 2.05);

                  // Schedule next tick if music is playing, view is animating, or visualizer is decaying
                  if self.is_playing || is_animating || is_decaying {
                      self.ensure_ticking(context);
                  }
             }
         }
         self.update_derived_fields();
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<MusicPlayerModel, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = MusicPlayerModel::new(None);
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let runtime = Runtime::new(model, measurer);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Music Player", 800, 480, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Music Player", 800, 480, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Music Player", 800, 480, runtime, render_frame, |_| {})?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_message_from_str() {
        assert_eq!(Msg::from_str("play_pause"), Ok(Msg::PlayPause));
        assert_eq!(Msg::from_str("back"), Ok(Msg::Back));
        assert_eq!(Msg::from_str("stop"), Ok(Msg::Stop));
        assert_eq!(Msg::from_str("next"), Ok(Msg::Next));
        assert_eq!(Msg::from_str("prev"), Ok(Msg::Prev));
        assert_eq!(Msg::from_str("select_track:song1"), Ok(Msg::SelectTrack("song1".to_string())));
        assert_eq!(Msg::from_str("hover_track:song1"), Ok(Msg::HoverTrack("song1".to_string())));
    }
}
