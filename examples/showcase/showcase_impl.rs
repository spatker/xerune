#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use serde::{Serialize, Deserialize};
use tiny_skia::{Color, LineCap, LineJoin, Paint, PathBuilder, PixmapMut, Rect, Stroke, Transform};
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;

/// Simulation step; the vehicle needles and monitor traces advance per tick.
pub const TICK_MS: u32 = 50;

/// Samples kept per monitor trace (one per tick, ~6 s of history).
const WAVE_LEN: usize = 120;

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Service {
    pub name: String,
    pub detail: String,
    pub state: String,
    /// Status chip tone; neither means neutral.
    pub ok: bool,
    pub warn: bool,
}

impl Service {
    fn new(name: &str, detail: &str, state: &str, ok: bool, warn: bool) -> Self {
        Self { name: name.into(), detail: detail.into(), state: state.into(), ok, warn }
    }
}

pub struct Segment {
    pub on: bool,
}

#[derive(XeruneTemplate, Serialize, Deserialize)]
#[template(path = "showcase.html")]
pub struct ShowcaseModel {
    pub active_tab: usize, // 1 = System, 2 = Vehicle, 3 = Monitor, 4 = Controls
    /// Set from the viewport in `on_resize`; picks the monitor canvas size.
    #[serde(default)]
    pub portrait: bool,

    // System
    pub cpu: f32,
    pub cpu_ghz: f32,
    pub mem: f32,
    pub soc_temp: f32,
    pub cores: Vec<f32>,
    pub user_counter: i32,
    pub services: Vec<Service>,
    pub auto_scale: bool,
    pub maintenance_mode: bool,

    // Vehicle
    pub speed_kmh: f32,
    pub target_speed: f32,
    pub engine_rpm: f32,
    pub battery_pct: f32,
    pub target_temp: f32,
    pub engine_running: bool,

    // Patient monitor
    pub heart_rate_bpm: u32,
    pub spo2_pct: u32,
    pub sys_bp: u32,
    pub dia_bp: u32,
    pub resp_rate: u32,

    // Controls
    pub volume: u32,
    pub wifi: bool,
    pub bluetooth: bool,

    #[serde(default)]
    pub tick_count: u64,
    #[serde(skip)]
    pub ecg: Vec<f32>,
    #[serde(skip)]
    pub pleth: Vec<f32>,
}

impl Default for ShowcaseModel {
    fn default() -> Self {
        Self {
            active_tab: 1,
            portrait: false,

            cpu: 38.0,
            cpu_ghz: 1.42,
            mem: 61.0,
            soc_temp: 52.0,
            cores: vec![34.0, 52.0, 21.0, 45.0],
            user_counter: 1234,
            services: vec![
                Service::new("render-engine", "fast_renderer · 60 fps", "RUNNING", true, false),
                Service::new("telemetry", "mqtt://fleet.local", "SYNCING", false, true),
                Service::new("ota-updater", "next check 02:00", "IDLE", false, false),
            ],
            auto_scale: true,
            maintenance_mode: false,

            speed_kmh: 0.0,
            target_speed: 75.0,
            engine_rpm: 900.0,
            battery_pct: 88.0,
            target_temp: 21.5,
            engine_running: true,

            heart_rate_bpm: 74,
            spo2_pct: 98,
            sys_bp: 120,
            dia_bp: 80,
            resp_rate: 16,

            volume: 6,
            wifi: true,
            bluetooth: false,

            tick_count: 0,
            ecg: vec![0.0; WAVE_LEN],
            pleth: vec![0.0; WAVE_LEN],
        }
    }
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum ShowcaseMsg {
    SelectTab(usize),
    IncrementProgress,
    IncrementUserCounter,
    DecrementUserCounter,
    ResetCounter,
    IncrementSpeed,
    DecrementSpeed,
    ToggleEngine,
    ToggleAutoScale,
    ToggleMaintenance,
    ToggleWifi,
    ToggleBluetooth,
    VolumeUp,
    VolumeDown,
    Tick,
}

/// Gauge sweep in degrees for the 270° dials.
const GAUGE_SWEEP: f32 = 270.0;
const MAX_SPEED: f32 = 240.0;
const MAX_RPM: f32 = 7000.0;
/// Upper speed of each forward gear.
const GEAR_TOPS: [f32; 6] = [30.0, 60.0, 95.0, 135.0, 180.0, MAX_SPEED];

impl ShowcaseModel {
    pub fn subtitle(&self) -> &'static str {
        match self.active_tab {
            1 => "system overview",
            2 => "vehicle cluster",
            3 => "patient monitor",
            _ => "widget gallery",
        }
    }

    pub fn speed_deg(&self) -> f32 {
        (self.speed_kmh / MAX_SPEED).clamp(0.0, 1.0) * GAUGE_SWEEP
    }

    pub fn rpm_deg(&self) -> f32 {
        (self.engine_rpm / MAX_RPM).clamp(0.0, 1.0) * GAUGE_SWEEP
    }

    pub fn gear(&self) -> usize {
        GEAR_TOPS.iter().position(|&top| self.speed_kmh < top).unwrap_or(GEAR_TOPS.len() - 1) + 1
    }

    pub fn mem_gb(&self) -> f32 {
        self.mem / 100.0 * 4.0
    }

    /// SoC temperature mapped onto 30..90 °C for the donut.
    pub fn temp_pct(&self) -> f32 {
        ((self.soc_temp - 30.0) / 60.0 * 100.0).clamp(0.0, 100.0)
    }

    pub fn volume_pct(&self) -> u32 {
        self.volume * 10
    }

    pub fn volume_segments(&self) -> Vec<Segment> {
        (0..10).map(|i| Segment { on: i < self.volume }).collect()
    }

    /// Needle wedge start: 1.5° before the arc end so the 3° wedge centers on it.
    pub fn speed_needle(&self) -> f32 {
        225.0 + self.speed_deg() - 1.5
    }

    pub fn rpm_needle(&self) -> f32 {
        225.0 + self.rpm_deg() - 1.5
    }

    pub fn rpm_k(&self) -> f32 {
        self.engine_rpm / 1000.0
    }

    pub fn range_km(&self) -> f32 {
        self.battery_pct * 4.6
    }

    /// Cabin temperature mapped onto 16..26 °C for the bar.
    pub fn cabin_pct(&self) -> f32 {
        ((self.target_temp - 16.0) * 10.0).clamp(0.0, 100.0)
    }

    /// Engine speed for the current road speed: climbs through each gear and
    /// drops back when the next one engages.
    fn rpm_for_speed(&self) -> f32 {
        if !self.engine_running {
            return 0.0;
        }
        let gear = self.gear() - 1;
        let low = if gear == 0 { 0.0 } else { GEAR_TOPS[gear - 1] };
        let within = (self.speed_kmh - low) / (GEAR_TOPS[gear] - low);
        900.0 + within.clamp(0.0, 1.0) * 4800.0
    }

    fn step_system(&mut self, rng: &mut LcgRng) {
        // System numbers change slowly; update them twice a second.
        if self.tick_count % 10 != 0 {
            return;
        }
        self.cpu = (self.cpu + rng.gen_range(-4.0, 4.0)).clamp(8.0, 97.0);
        self.cpu_ghz = 0.6 + self.cpu / 100.0 * 1.2;
        self.mem = (self.mem + rng.gen_range(-0.6, 0.6)).clamp(40.0, 90.0);
        self.soc_temp = (self.soc_temp + (35.0 + self.cpu * 0.4 - self.soc_temp) * 0.1).clamp(30.0, 90.0);
        for core in self.cores.iter_mut() {
            *core = (self.cpu + rng.gen_range(-18.0, 18.0)).clamp(2.0, 100.0);
        }
    }

    fn step_vehicle(&mut self, rng: &mut LcgRng) {
        if !self.engine_running {
            self.target_speed = 0.0;
        }
        // Ease toward the requested speed so the needles sweep smoothly.
        self.speed_kmh += (self.target_speed - self.speed_kmh) * 0.08;
        if (self.target_speed - self.speed_kmh).abs() < 0.05 {
            self.speed_kmh = self.target_speed;
        }
        let rpm = self.rpm_for_speed();
        let idle_wobble = if self.engine_running { rng.gen_range(-25.0, 25.0) } else { 0.0 };
        self.engine_rpm += (rpm + idle_wobble - self.engine_rpm) * 0.25;
        if self.engine_running && self.speed_kmh > 1.0 {
            self.battery_pct = (self.battery_pct - self.speed_kmh * 0.00002).max(0.0);
        }
    }

    fn step_monitor(&mut self, rng: &mut LcgRng, context: &mut xerune::Context) {
        // One heartbeat every `beat` ticks, derived from the heart rate.
        let beat = (60_000.0 / self.heart_rate_bpm as f32 / TICK_MS as f32).round() as u64;
        let phase = self.tick_count % beat.max(1);
        let ecg = match phase {
            2 => 0.15,  // P wave
            3 => 0.1,
            5 => -0.25, // Q
            6 => 1.6,   // R
            7 => -0.5,  // S
            10 => 0.25, // T wave
            11 => 0.35,
            12 => 0.2,
            _ => rng.gen_range(-0.03, 0.03),
        };
        // Pleth: a smooth pulse trailing the R wave.
        let t = phase as f32 / beat as f32;
        let pleth = (t * core::f32::consts::TAU).sin() * 0.5 + (t * 2.0 * core::f32::consts::TAU).sin() * 0.15;

        self.ecg.remove(0);
        self.ecg.push(ecg);
        self.pleth.remove(0);
        self.pleth.push(pleth);

        if self.tick_count % 20 == 0 {
            let hr = self.heart_rate_bpm as f32 + rng.gen_range(-1.5, 1.5);
            self.heart_rate_bpm = hr.round().clamp(66.0, 84.0) as u32;
            self.spo2_pct = (97.0 + rng.gen_range(0.0, 2.0)).round() as u32;
            self.resp_rate = (15.0 + rng.gen_range(0.0, 3.0)).round() as u32;
        }

        let id = if self.portrait { "waves_tall" } else { "waves_wide" };
        let Some(canvas) = context.canvas_mut(id) else {
            return;
        };
        let (w, h) = (canvas.width, canvas.height);
        if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, w, h) {
            draw_waves(&mut pixmap, &self.ecg, &self.pleth);
        }
        canvas.dirty = true;
    }
}

/// Draw the ECG (top) and pleth (bottom) traces with a faint grid.
fn draw_waves(pixmap: &mut PixmapMut, ecg: &[f32], pleth: &[f32]) {
    let w = pixmap.width() as f32;
    let h = pixmap.height() as f32;
    pixmap.fill(Color::from_rgba8(3, 7, 10, 255));

    let mut grid = Paint::default();
    grid.set_color_rgba8(16, 34, 30, 255);
    let mut x = 0.0;
    while x < w {
        if let Some(r) = Rect::from_xywh(x, 0.0, 1.0, h) {
            pixmap.fill_rect(r, &grid, Transform::identity(), None);
        }
        x += 24.0;
    }
    let mut y = 0.0;
    while y < h {
        if let Some(r) = Rect::from_xywh(0.0, y, w, 1.0) {
            pixmap.fill_rect(r, &grid, Transform::identity(), None);
        }
        y += 24.0;
    }

    let ecg_base = h * 0.36;
    let pleth_base = h * 0.80;
    stroke_trace(pixmap, ecg, ecg_base, h * 0.17, (52, 211, 153));
    stroke_trace(pixmap, pleth, pleth_base, h * 0.12, (34, 211, 238));
}

/// Stroke `samples` across the full width: a wide dim pass for glow, then
/// the bright line on top. The newest sample sits at the right edge.
fn stroke_trace(pixmap: &mut PixmapMut, samples: &[f32], base: f32, scale: f32, rgb: (u8, u8, u8)) {
    let w = pixmap.width() as f32;
    let step = w / (samples.len() - 1) as f32;
    let mut pb = PathBuilder::new();
    for (i, s) in samples.iter().enumerate() {
        let (x, y) = (i as f32 * step, base - s * scale);
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    let Some(path) = pb.finish() else { return };

    let mut paint = Paint::default();
    paint.anti_alias = true;
    let mut stroke = Stroke { line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };

    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 50);
    stroke.width = 6.0;
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);

    paint.set_color_rgba8(rgb.0, rgb.1, rgb.2, 255);
    stroke.width = 2.0;
    pixmap.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}

impl Model for ShowcaseModel {
    type Message = ShowcaseMsg;

    fn on_resize(&mut self, width: f32, height: f32, _context: &mut xerune::Context) {
        self.portrait = height > width;
    }

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
        match msg {
            ShowcaseMsg::SelectTab(idx) => self.active_tab = idx.clamp(1, 4),
            ShowcaseMsg::IncrementProgress => self.cpu = (self.cpu + 15.0).min(97.0),
            ShowcaseMsg::IncrementUserCounter => self.user_counter += 1,
            ShowcaseMsg::DecrementUserCounter => self.user_counter -= 1,
            ShowcaseMsg::ResetCounter => self.user_counter = 0,
            ShowcaseMsg::IncrementSpeed => {
                if self.engine_running {
                    self.target_speed = (self.target_speed + 25.0).min(MAX_SPEED - 5.0);
                }
            }
            ShowcaseMsg::DecrementSpeed => self.target_speed = (self.target_speed - 30.0).max(0.0),
            ShowcaseMsg::ToggleEngine => self.engine_running = !self.engine_running,
            ShowcaseMsg::ToggleAutoScale => self.auto_scale = !self.auto_scale,
            ShowcaseMsg::ToggleMaintenance => self.maintenance_mode = !self.maintenance_mode,
            ShowcaseMsg::ToggleWifi => self.wifi = !self.wifi,
            ShowcaseMsg::ToggleBluetooth => self.bluetooth = !self.bluetooth,
            ShowcaseMsg::VolumeUp => self.volume = (self.volume + 1).min(10),
            ShowcaseMsg::VolumeDown => self.volume = self.volume.saturating_sub(1),
            ShowcaseMsg::Tick => {
                self.tick_count = self.tick_count.wrapping_add(1);
                let mut rng = LcgRng::new(self.tick_count.wrapping_mul(2654435761) ^ 42);
                self.step_system(&mut rng);
                self.step_vehicle(&mut rng);
                if self.active_tab == 3 {
                    self.step_monitor(&mut rng, context);
                }
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<ShowcaseModel, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = ShowcaseModel::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };

    let mut runtime = Runtime::new(model, measurer);

    runtime.set_interval("tick".to_string(), TICK_MS);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Showcase", 800, 480, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Showcase", 800, 480, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Showcase", 800, 480, runtime, render_frame, |_| {})?;
    }

    Ok(())
}
