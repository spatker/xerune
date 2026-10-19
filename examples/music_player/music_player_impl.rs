use serde::{Serialize, Deserialize};
use xerune::{Model, XeruneMessage, XeruneTemplate};

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

/// The equalizer's bands: center frequency labels.
pub const EQ_BANDS: [&str; 5] = ["60 Hz", "230 Hz", "910 Hz", "3.6 kHz", "14 kHz"];
/// How far a band boosts or cuts, in dB.
pub const EQ_RANGE: i32 = 12;
/// The presets, as band gains in dB.
const EQ_PRESETS: [(&str, [i32; 5]); 4] = [
    ("Flat", [0, 0, 0, 0, 0]),
    ("Bass", [7, 4, 0, -1, -2]),
    ("Vocal", [-3, -1, 4, 3, 0]),
    ("Treble", [-2, -1, 0, 4, 7]),
];

/// One band of the equalizer, as the template shows it.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct EqBand {
    pub label: &'static str,
    pub gain: i32,
    /// The gain as text: "+3 dB", "0 dB".
    pub gain_text: String,
}

/// An equalizer preset button.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct EqPreset {
    pub name: &'static str,
    /// Whether the bands are set as this preset sets them.
    pub active: bool,
}

/// A band's new gain, from a band slider's `eq:<band>:<gain>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandGain {
    pub band: usize,
    pub gain: i32,
}

impl core::str::FromStr for BandGain {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        let (band, gain) = s.split_once(':').ok_or(())?;
        Ok(Self { band: band.parse().map_err(|_| ())?, gain: gain.parse().map_err(|_| ())? })
    }
}

#[derive(XeruneTemplate, Serialize, Deserialize, Clone, PartialEq)]
#[template(path = "examples/music_player/templates/music_player.html")]
pub struct MusicPlayerModel {
    pub tracks: Vec<Track>,
    pub current_track_index: Option<usize>,
    pub is_playing: bool,
    pub elapsed_seconds: u64,
    pub visualizer_data: Vec<f32>,
    pub hovered_track: String,
    pub active_list_index: usize,
    pub current_track: Track,
    pub elapsed_time: String,
    pub total_time: String,
    #[serde(default)]
    pub tick_count: u64,
    /// Whether the list and the player view are in the view. The screen's
    /// own one always is; the other stays until it has slid out
    /// (`data-on-transitionend`), so it isn't laid out off screen.
    #[serde(default = "default_true")]
    pub list_shown: bool,
    #[serde(default)]
    pub player_shown: bool,
    /// Playback volume, 0 to 100.
    #[serde(default = "default_volume")]
    pub volume: u32,
    /// The equalizer's gain per band of [`EQ_BANDS`], in dB.
    #[serde(default = "default_eq")]
    pub eq_gains: Vec<i32>,
    /// Whether the equalizer shows in place of the cover.
    #[serde(default)]
    pub eq_open: bool,
}

fn default_volume() -> u32 {
    70
}

fn default_eq() -> Vec<i32> {
    vec![0; EQ_BANDS.len()]
}

fn default_true() -> bool {
    true
}

/// Where the track list is looked for, relative to the working directory:
/// the repository root, then a deployed layout.
#[cfg(not(target_arch = "wasm32"))]
const TRACK_LISTS: [&str; 2] = ["examples/music_player/resources/music.json", "resources/music_player/music.json"];

/// Read the track list from the first of [`TRACK_LISTS`] that exists, or
/// panic naming every path tried and why it failed.
#[cfg(not(target_arch = "wasm32"))]
fn read_tracks() -> Vec<Track> {
    let mut failures = String::new();
    for path in TRACK_LISTS {
        match std::fs::read_to_string(path) {
            Ok(content) => return serde_json::from_str(&content).unwrap_or_else(|e| panic!("can't parse {path}: {e}")),
            Err(e) => failures.push_str(&format!("\n  {path}: {e}")),
        }
    }
    let dir = std::env::current_dir().map_or_else(|e| format!("? ({e})"), |d| d.display().to_string());
    panic!("can't read the track list; tried, in {dir}:{failures}");
}

impl MusicPlayerModel {
    pub fn new(json_content: Option<&str>) -> Self {
        let tracks: Vec<Track> = if let Some(content) = json_content {
            serde_json::from_str(content).unwrap_or_else(|e| panic!("can't parse the track list: {e}"))
        } else {
            #[cfg(not(target_arch = "wasm32"))]
            {
                read_tracks()
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
            visualizer_data: vec![10.0; 30], // 30 bars
            hovered_track: String::new(),
            active_list_index: 0,
            current_track: current_track.clone(),
            elapsed_time: "0:00".to_string(),
            total_time: if current_track.id.is_empty() { "0:00".to_string() } else { current_track.duration.clone() },
            tick_count: 0,
            list_shown: true,
            player_shown: false,
            volume: default_volume(),
            eq_gains: default_eq(),
            eq_open: false,
        }
    }

    pub fn total_minutes(&self) -> u64 {
        self.tracks.iter().map(Track::duration_seconds).sum::<u64>() / 60
    }

    /// The current track's length, the seek bar's end.
    pub fn duration_seconds(&self) -> u64 {
        self.current_track.duration_seconds()
    }

    /// The visualizer's bar heights (px): the simulated levels, louder
    /// with the volume and with the gain of each bar's equalizer band, as
    /// the music would sound. At most the visualizer's 64px.
    pub fn viz_heights(&self) -> Vec<u32> {
        let bars = self.visualizer_data.len().max(1);
        let volume = self.volume as f32 / default_volume() as f32;
        self.visualizer_data.iter().enumerate().map(|(i, level)| {
            let gain = self.eq_gains.get(i * EQ_BANDS.len() / bars).copied().unwrap_or(0);
            let boost = 10f32.powf(gain as f32 / 20.0);
            (level * volume * boost).clamp(2.0, 64.0) as u32
        }).collect()
    }

    pub fn eq_bands(&self) -> Vec<EqBand> {
        EQ_BANDS.iter().zip(&self.eq_gains).map(|(&label, &gain)| EqBand {
            label,
            gain,
            gain_text: if gain > 0 { format!("+{gain} dB") } else { format!("{gain} dB") },
        }).collect()
    }

    pub fn eq_presets(&self) -> Vec<EqPreset> {
        EQ_PRESETS.iter().map(|(name, gains)| EqPreset { name, active: self.eq_gains == gains }).collect()
    }

    /// Three bar heights (px) for the "now playing" marker in the list.
    pub fn mini_eq(&self) -> Vec<u32> {
        self.visualizer_data.iter().step_by(7).take(3).map(|v| (4.0 + v * 0.24) as u32).collect()
    }

    /// Whether the stopped visualizer's bars are still falling.
    fn is_decaying(&self) -> bool {
        self.visualizer_data.iter().any(|&val| val > 2.05)
    }

    fn format_time(seconds: u64) -> String {
        let min = seconds / 60;
        let sec = seconds % 60;
        format!("{}:{:02}", min, sec)
    }

    fn update_derived_fields(&mut self) {
        match self.current_track_index {
            Some(_) => self.player_shown = true,
            None => self.list_shown = true,
        }
        if self.tracks.is_empty() { return; }
        let dummy_track = self.tracks[0].clone();
        let current = match self.current_track_index {
            Some(i) => &self.tracks[i],
            None if self.active_list_index < self.tracks.len() => &self.tracks[self.active_list_index],
            None => &dummy_track,
        };
        self.current_track = current.clone();

        self.elapsed_time = Self::format_time(self.elapsed_seconds);
        self.total_time = current.duration.clone();
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
    /// Visualizer step.
    Tick,
    /// The list (player) view finished sliding.
    ListSlid,
    PlayerSlid,
    /// A second of the track played.
    Second,
    #[xerune(prefix = "hover_track:")]
    HoverTrack(String),
    UnhoverTrack,
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
    /// Jump to a second of the track (the seek bar).
    #[xerune(prefix = "seek:")]
    Seek(u64),
    #[xerune(prefix = "volume:")]
    Volume(u32),
    /// Show or hide the equalizer.
    ToggleEq,
    #[xerune(prefix = "eq:")]
    EqBand(BandGain),
    #[xerune(prefix = "eq_preset:")]
    EqPreset(String),
}

impl Model for MusicPlayerModel {
    type Message = Msg;

    fn subscriptions(&self) -> xerune::Subscriptions<Msg> {
        let mut subscriptions = xerune::Subscriptions::none();
        if self.is_playing || self.is_decaying() {
            subscriptions = subscriptions.every(core::time::Duration::from_millis(33), Msg::Tick);
        }
        if self.is_playing {
            subscriptions = subscriptions.every(core::time::Duration::from_secs(1), Msg::Second);
        }
        subscriptions
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context<Msg>) {
         if self.tracks.is_empty() { return; }
         match msg {
             Msg::SelectTrack(id_str) => {
                 if let Some(index) = self.tracks.iter().position(|t| t.id == id_str) {
                     self.current_track_index = Some(index);
                     self.active_list_index = index;
                     self.is_playing = true;
                     self.elapsed_seconds = 0;
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
                 if self.current_track_index.is_none() && !self.tracks.is_empty() {
                     self.current_track_index = Some(self.active_list_index);
                 }
                 self.is_playing = !self.is_playing;
                 if self.is_playing {
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
                             context.scroll_into_view(&format!("track-{}", self.tracks[self.active_list_index].id));
                         }
                     }
                     "Down" | "KEY_DOWN" | "ArrowDown" => {
                         if self.current_track_index.is_none() && self.active_list_index + 1 < self.tracks.len() {
                             self.active_list_index += 1;
                             context.scroll_into_view(&format!("track-{}", self.tracks[self.active_list_index].id));
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
                     "VolumeUp" | "KEY_VOLUMEUP" | "AudioVolumeUp" => {
                         self.update(Msg::Volume((self.volume + 5).min(100)), context);
                     }
                     "VolumeDown" | "KEY_VOLUMEDOWN" | "AudioVolumeDown" => {
                         self.update(Msg::Volume(self.volume.saturating_sub(5)), context);
                     }
                     _ => {}
                 }
             },
             Msg::Tick => {
                  self.tick_count = self.tick_count.wrapping_add(1);
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

             }
             Msg::Seek(second) => {
                 if let Some(idx) = self.current_track_index {
                     self.elapsed_seconds = second.min(self.tracks[idx].duration_seconds());
                 }
             }
             Msg::Volume(volume) => self.volume = volume.min(100),
             Msg::ToggleEq => self.eq_open = !self.eq_open,
             Msg::EqBand(BandGain { band, gain }) => {
                 if let Some(slot) = self.eq_gains.get_mut(band) {
                     *slot = gain.clamp(-EQ_RANGE, EQ_RANGE);
                 }
             }
             Msg::EqPreset(name) => {
                 if let Some((_, gains)) = EQ_PRESETS.iter().find(|(preset, _)| *preset == name) {
                     self.eq_gains = gains.to_vec();
                 }
             }
             Msg::ListSlid => self.list_shown &= self.current_track_index.is_none(),
             Msg::PlayerSlid => self.player_shown &= self.current_track_index.is_some(),
             Msg::Second => {
                 if let Some(idx) = self.current_track_index {
                     let duration = self.tracks[idx].duration_seconds();
                     if self.elapsed_seconds < duration {
                         self.elapsed_seconds += 1;
                     } else {
                         self.update(Msg::Next, context);
                     }
                 }
             }
         }
         self.update_derived_fields();
    }
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
        assert_eq!(Msg::from_str("seek:42"), Ok(Msg::Seek(42)));
        assert_eq!(Msg::from_str("eq:2:-3"), Ok(Msg::EqBand(BandGain { band: 2, gain: -3 })));
        assert_eq!(Msg::from_str("eq:2"), Err(()));
    }

    /// Dragging the seek bar seeks, and a band slider sets its band.
    #[test]
    fn dragging_the_bars() {
        let tracks = include_str!("resources/music.json");
        let mut runtime = xerune::Runtime::new(MusicPlayerModel::new(Some(tracks)), Measurer);
        let clock = xerune::runtime::ManualClock::new();
        runtime.set_clock(clock.clone());
        runtime.set_size(800.0, 480.0);
        runtime.handle_messages(["select_track:1".to_string(), "toggle_eq".to_string()]);
        // The player slides in.
        for _ in 0..30 {
            clock.advance(core::time::Duration::from_millis(16));
            runtime.poll();
        }
        // The ranges in view order: five bands, the seek bar, the volume.
        let bars: Vec<xerune::Rect> = runtime.ui.render_data.iter()
            .filter(|(_, data)| matches!(data.kind, xerune::style::RenderKind::Slider(..)))
            .filter_map(|(node, _)| runtime.ui.absolute_rect(node))
            .collect();
        assert_eq!(bars.len(), 7);
        let drag = |runtime: &mut xerune::Runtime<MusicPlayerModel, Measurer>, bar: xerune::Rect, fraction: f32| {
            let y = bar.y + bar.height / 2.0;
            runtime.handle_event(xerune::InputEvent::TouchStart { id: 0, x: bar.x + 1.0, y });
            runtime.handle_event(xerune::InputEvent::TouchMove { id: 0, x: bar.x + 7.0 + fraction * (bar.width - 14.0), y });
            runtime.handle_event(xerune::InputEvent::TouchEnd { id: 0, x: bar.x + 7.0 + fraction * (bar.width - 14.0), y });
        };
        drag(&mut runtime, bars[5], 0.5);
        let duration = runtime.model().current_track.duration_seconds();
        assert!(runtime.model().elapsed_seconds.abs_diff(duration / 2) <= 1, "{} of {duration}", runtime.model().elapsed_seconds);
        drag(&mut runtime, bars[1], 1.0);
        assert_eq!(runtime.model().eq_gains, [0, EQ_RANGE, 0, 0, 0]);
        drag(&mut runtime, bars[6], 0.0);
        assert_eq!(runtime.model().volume, 0);
    }

    /// Seeking stays within the track; the equalizer keeps to its range,
    /// and its presets set every band.
    #[test]
    fn seek_volume_and_equalizer() {
        let mut model = MusicPlayerModel::new(Some(include_str!("resources/music.json")));
        let mut context = xerune::Context::default();
        model.update(Msg::SelectTrack(model.tracks[0].id.clone()), &mut context);
        let duration = model.tracks[0].duration_seconds();
        model.update(Msg::Seek(30), &mut context);
        assert_eq!((model.elapsed_seconds, model.elapsed_time.as_str()), (30, "0:30"));
        model.update(Msg::Seek(duration + 100), &mut context);
        assert_eq!(model.elapsed_seconds, duration);

        model.update(Msg::EqBand(BandGain { band: 0, gain: 40 }), &mut context);
        model.update(Msg::EqBand(BandGain { band: 9, gain: 1 }), &mut context);
        assert_eq!(model.eq_gains, [EQ_RANGE, 0, 0, 0, 0]);
        assert_eq!(model.eq_bands()[0].gain_text, "+12 dB");
        model.update(Msg::EqPreset("Bass".into()), &mut context);
        assert!(model.eq_presets().iter().find(|p| p.name == "Bass").unwrap().active);

        // Silence flattens the visualizer.
        model.update(Msg::Volume(0), &mut context);
        assert!(model.viz_heights().iter().all(|&h| h == 2));
    }

    struct Measurer;
    impl xerune::TextMeasurer for Measurer {
        fn measure_text(&self, _: &str, _: f32, _: u16) -> (f32, f32) {
            (10.0, 10.0)
        }
    }

    /// The view that slid out leaves the view once its transition ends,
    /// and comes back to slide in.
    #[test]
    fn the_hidden_view_leaves_after_sliding_out() {
        // The list as an outside source would hand it over: the studio runs
        // these tests from its own directory, where the file path doesn't resolve.
        let tracks = include_str!("resources/music.json");
        let mut runtime = xerune::Runtime::new(MusicPlayerModel::new(Some(tracks)), Measurer);
        let clock = xerune::runtime::ManualClock::new();
        runtime.set_clock(clock.clone());
        runtime.set_size(800.0, 480.0);
        let shown = |r: &xerune::Runtime<MusicPlayerModel, Measurer>| (r.model().list_shown, r.model().player_shown);
        assert_eq!(shown(&runtime), (true, false));

        runtime.handle_messages(["select_track:1".to_string()]);
        assert_eq!(shown(&runtime), (true, true), "both while they slide");
        for _ in 0..30 {
            clock.advance(core::time::Duration::from_millis(16));
            runtime.poll();
        }
        assert_eq!(shown(&runtime), (false, true));

        runtime.handle_messages(["back".to_string()]);
        assert_eq!(shown(&runtime), (true, true));
        for _ in 0..30 {
            clock.advance(core::time::Duration::from_millis(16));
            runtime.poll();
        }
        assert_eq!(shown(&runtime), (true, false));
    }
}
