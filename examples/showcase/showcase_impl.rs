#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use serde::{Serialize, Deserialize};
use tiny_skia::{PixmapMut, Paint, Color, Transform, Rect};
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;

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
pub struct RowData {
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(XeruneTemplate, Serialize, Deserialize)]
#[template(path = "showcase.html")]
pub struct ShowcaseModel {
    pub active_tab: usize, // 1 = System, 2 = Gauges, 3 = Medical, 4 = Controls

    // System overview stats
    pub system_load_value: f32,
    pub user_counter: i32,
    pub table_data: Vec<RowData>,

    // HMI / Gauges stats
    pub speed_kmh: f32,
    pub engine_rpm: f32,
    pub battery_pct: f32,
    pub target_temp: f32,
    pub engine_running: bool,

    // Medical stats
    pub heart_rate_bpm: u32,
    pub spo2_pct: u32,
    pub sys_bp: u32,
    pub dia_bp: u32,

    // Config options
    pub auto_scale: bool,
    pub maintenance_mode: bool,

    #[serde(default)]
    pub tick_count: u64,

    #[serde(skip)]
    pub ecg_samples: Vec<f32>,
}

impl Default for ShowcaseModel {
    fn default() -> Self {
        let mut initial_ecg = Vec::with_capacity(120);
        for _ in 0..120 {
            initial_ecg.push(0.0);
        }

        Self {
            active_tab: 1,
            system_load_value: 38.0,
            user_counter: 1234,
            table_data: vec![
                RowData { id: "001".into(), name: "System Core".into(), status: "Online".into() },
                RowData { id: "002".into(), name: "Render Engine".into(), status: "Active".into() },
                RowData { id: "003".into(), name: "Network".into(), status: "Idle".into() },
                RowData { id: "004".into(), name: "Storage".into(), status: "Checking".into() },
                RowData { id: "005".into(), name: "Audio".into(), status: "Muted".into() },
            ],

            // HMI stats
            speed_kmh: 75.0,
            engine_rpm: 2450.0,
            battery_pct: 88.0,
            target_temp: 21.5,
            engine_running: true,

            // Medical stats
            heart_rate_bpm: 74,
            spo2_pct: 98,
            sys_bp: 120,
            dia_bp: 80,

            // Config stats
            auto_scale: true,
            maintenance_mode: false,

            tick_count: 0,
            ecg_samples: initial_ecg,
        }
    }
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum ShowcaseMsg {
    SelectTab(usize),
    IncrementProgress,
    IncrementUserCounter,
    IncrementSpeed,
    DecrementSpeed,
    ToggleEngine,
    ToggleAutoScale,
    ToggleMaintenance,
    Tick,
}

impl Model for ShowcaseModel {
    type Message = ShowcaseMsg;

    fn update(&mut self, msg: Self::Message, context: &mut xerune::Context) {
        match msg {
            ShowcaseMsg::SelectTab(idx) => {
                self.active_tab = idx.clamp(1, 4);
            }
            ShowcaseMsg::IncrementProgress => {
                self.system_load_value += 10.0;
                if self.system_load_value > 100.0 {
                    self.system_load_value = 0.0;
                }
            }
            ShowcaseMsg::IncrementUserCounter => {
                self.user_counter += 1;
            }
            ShowcaseMsg::IncrementSpeed => {
                if self.engine_running {
                    self.speed_kmh = (self.speed_kmh + 10.0).min(240.0);
                    self.engine_rpm = (1000.0 + (self.speed_kmh / 240.0) * 5000.0).min(6500.0);
                }
            }
            ShowcaseMsg::DecrementSpeed => {
                self.speed_kmh = (self.speed_kmh - 10.0).max(0.0);
                if self.engine_running {
                    self.engine_rpm = (1000.0 + (self.speed_kmh / 240.0) * 5000.0).min(6500.0);
                } else {
                    self.engine_rpm = 0.0;
                }
            }
            ShowcaseMsg::ToggleEngine => {
                self.engine_running = !self.engine_running;
                if !self.engine_running {
                    self.speed_kmh = 0.0;
                    self.engine_rpm = 0.0;
                } else {
                    self.engine_rpm = 900.0;
                }
            }
            ShowcaseMsg::ToggleAutoScale => {
                self.auto_scale = !self.auto_scale;
            }
            ShowcaseMsg::ToggleMaintenance => {
                self.maintenance_mode = !self.maintenance_mode;
            }
            ShowcaseMsg::Tick => {
                self.tick_count = self.tick_count.wrapping_add(1);

                // Dynamic simulation noise
                let mut rng = LcgRng::new(self.user_counter as u64 + self.tick_count + 42);

                // Slightly fluctuate system load
                let noise = rng.gen_range(-2.0, 2.0);
                self.system_load_value = (self.system_load_value + noise).clamp(15.0, 95.0);

                // --- 1. Draw CPU Load Chart Canvas (Overview tab) ---
                if self.active_tab == 1 {
                    let mut cpu_loads = vec![0.0; 4];
                    for load in cpu_loads.iter_mut() {
                        let n = rng.gen_range(-12.0, 12.0);
                        *load = (self.system_load_value + n).clamp(5.0, 100.0);
                    }

                    if let Some(canvas) = context.canvas_mut("load_chart") {
                        let w = canvas.width;
                        let h = canvas.height;

                        if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, w, h) {
                            pixmap.fill(Color::from_rgba8(0, 0, 0, 0));

                            let num_cores = cpu_loads.len();
                            let padding_top = 10.0;
                            let padding_bottom = 10.0;
                            let padding_left = 60.0;
                            let padding_right = 20.0;

                            let total_h = h as f32 - padding_top - padding_bottom;
                            let bar_gap = 10.0;
                            let bar_height = (total_h - (bar_gap * (num_cores as f32 - 1.0))) / num_cores as f32;

                            for (i, load) in cpu_loads.iter().enumerate() {
                                let y = padding_top + i as f32 * (bar_height + bar_gap);

                                let mut text_paint = Paint::default();
                                text_paint.set_color_rgba8(180, 180, 180, 255);
                                let dot_size = 3.0;
                                for d in 0..(i + 1).min(4) {
                                    let dot_rect = Rect::from_xywh(15.0 + (d as f32 * 6.0), y + bar_height / 2.0 - 1.5, dot_size, dot_size).unwrap();
                                    pixmap.fill_rect(dot_rect, &text_paint, Transform::identity(), None);
                                }

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
                                        let pct = s as f32 / steps as f32;
                                        if pct < 0.4 {
                                            paint.set_color_rgba8(20, 220, 20, 255);
                                        } else if pct < 0.6 {
                                            paint.set_color_rgba8(20, 220, 220, 255);
                                        } else if pct < 0.8 {
                                            paint.set_color_rgba8(220, 160, 20, 255);
                                        } else {
                                            paint.set_color_rgba8(220, 40, 40, 255);
                                        }
                                    } else {
                                        paint.set_color_rgba8(40, 45, 50, 255);
                                    }

                                    pixmap.fill_rect(step_rect, &paint, Transform::identity(), None);
                                }
                            }
                        }

                        canvas.dirty = true;
                    }
                }

                // --- 2. Draw Digital Cockpit Gauge Canvas (Gauges tab, active_tab == 2) ---
                if self.active_tab == 2 {
                    if self.engine_running {
                        let vib = rng.gen_range(-1.2, 1.2);
                        self.engine_rpm = (self.engine_rpm + vib).clamp(800.0, 7000.0);
                    }

                    if let Some(canvas) = context.canvas_mut("gauge_chart") {
                        let w = canvas.width;
                        let h = canvas.height;

                        if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, w, h) {
                            pixmap.fill(Color::from_rgba8(12, 18, 28, 255));

                            // Grid background
                            let mut bg_grid = Paint::default();
                            bg_grid.set_color_rgba8(22, 32, 48, 255);
                            for x in (0..w).step_by(30) {
                                if let Some(r) = Rect::from_xywh(x as f32, 0.0, 1.0, h as f32) {
                                    pixmap.fill_rect(r, &bg_grid, Transform::identity(), None);
                                }
                            }
                            for y in (0..h).step_by(30) {
                                if let Some(r) = Rect::from_xywh(0.0, y as f32, w as f32, 1.0) {
                                    pixmap.fill_rect(r, &bg_grid, Transform::identity(), None);
                                }
                            }

                            // Helper function to draw circular gauge with ticks and needle
                            let draw_gauge = |pixmap: &mut PixmapMut, cx: f32, cy: f32, r: f32, val_pct: f32, is_rpm: bool| {
                                let start_angle_deg = 135.0f32;
                                let total_sweep_deg = 270.0f32;
                                let active_angle_deg = start_angle_deg + (val_pct * total_sweep_deg);

                                let num_ticks = 30;

                                // Draw circular tick marks
                                for t in 0..=num_ticks {
                                    let tick_pct = t as f32 / num_ticks as f32;
                                    let angle_deg = start_angle_deg + (tick_pct * total_sweep_deg);
                                    let rad = angle_deg.to_radians();

                                    let is_major = t % 5 == 0;
                                    let tick_len = if is_major { 16.0 } else { 9.0 };
                                    let tick_width = if is_major { 3.5 } else { 1.8 };

                                    let r1 = r - tick_len;
                                    let r2 = r;

                                    let x1 = cx + rad.cos() * r1;
                                    let y1 = cy + rad.sin() * r1;
                                    let x2 = cx + rad.cos() * r2;
                                    let y2 = cy + rad.sin() * r2;

                                    let mut p = Paint::default();
                                    if tick_pct <= val_pct {
                                        if is_rpm && tick_pct > 0.75 {
                                            p.set_color_rgba8(255, 45, 45, 255); // Redline
                                        } else if is_rpm {
                                            p.set_color_rgba8(255, 170, 0, 255); // RPM Amber
                                        } else {
                                            p.set_color_rgba8(0, 220, 255, 255); // Speed Cyan
                                        }
                                    } else {
                                        p.set_color_rgba8(40, 55, 75, 255);
                                    }

                                    let mut pb = tiny_skia::PathBuilder::new();
                                    pb.move_to(x1, y1);
                                    pb.line_to(x2, y2);
                                    if let Some(path) = pb.finish() {
                                        let mut stroke = tiny_skia::Stroke::default();
                                        stroke.width = tick_width;
                                        stroke.line_cap = tiny_skia::LineCap::Round;
                                        pixmap.stroke_path(&path, &p, &stroke, Transform::identity(), None);
                                    }
                                }

                                // Draw Needle
                                let needle_rad = active_angle_deg.to_radians();
                                let needle_len = r - 20.0;
                                let nx = cx + needle_rad.cos() * needle_len;
                                let ny = cy + needle_rad.sin() * needle_len;

                                let mut needle_paint = Paint::default();
                                if is_rpm && val_pct > 0.75 {
                                    needle_paint.set_color_rgba8(255, 40, 40, 255);
                                } else if is_rpm {
                                    needle_paint.set_color_rgba8(255, 200, 30, 255);
                                } else {
                                    needle_paint.set_color_rgba8(0, 240, 255, 255);
                                }

                                let mut pb = tiny_skia::PathBuilder::new();
                                pb.move_to(cx, cy);
                                pb.line_to(nx, ny);
                                if let Some(path) = pb.finish() {
                                    let mut stroke = tiny_skia::Stroke::default();
                                    stroke.width = 4.5;
                                    stroke.line_cap = tiny_skia::LineCap::Round;
                                    pixmap.stroke_path(&path, &needle_paint, &stroke, Transform::identity(), None);
                                }

                                // Center Hub Cap
                                let mut hub_bg = Paint::default();
                                hub_bg.set_color_rgba8(24, 34, 50, 255);
                                if let Some(hub_rect) = Rect::from_xywh(cx - 14.0, cy - 14.0, 28.0, 28.0) {
                                    pixmap.fill_rect(hub_rect, &hub_bg, Transform::identity(), None);
                                }
                                if let Some(hub_inner) = Rect::from_xywh(cx - 6.0, cy - 6.0, 12.0, 12.0) {
                                    pixmap.fill_rect(hub_inner, &needle_paint, Transform::identity(), None);
                                }
                            };

                            // Draw Speedometer (Left: cx=190, cy=130, r=92)
                            let speed_pct = (self.speed_kmh / 240.0).clamp(0.0, 1.0);
                            draw_gauge(&mut pixmap, 190.0, 130.0, 92.0, speed_pct, false);

                            // Draw Tachometer (Right: cx=530, cy=130, r=92)
                            let rpm_pct = (self.engine_rpm / 7000.0).clamp(0.0, 1.0);
                            draw_gauge(&mut pixmap, 530.0, 130.0, 92.0, rpm_pct, true);
                        }

                        canvas.dirty = true;
                    }
                }

                // --- 3. Draw Medical ECG Canvas (Medical tab, active_tab == 3) ---
                if self.active_tab == 3 {
                    let step = (self.tick_count % 24) as f32;
                    let sample = if step == 4.0 {
                        0.3
                    } else if step == 7.0 {
                        -0.35
                    } else if step == 8.0 {
                        1.5 // R wave peak
                    } else if step == 9.0 {
                        -0.6
                    } else if step == 12.0 {
                        0.4 // T wave
                    } else {
                        let mut ecg_rng = LcgRng::new(self.tick_count * 13 + step as u64);
                        ecg_rng.gen_range(-0.06, 0.06)
                    };

                    if self.ecg_samples.len() >= 120 {
                        self.ecg_samples.remove(0);
                    }
                    self.ecg_samples.push(sample);

                    let hr_delta = rng.gen_range(-1.0, 1.0) as i32;
                    self.heart_rate_bpm = ((self.heart_rate_bpm as i32 + hr_delta).clamp(65, 85)) as u32;

                    if let Some(canvas) = context.canvas_mut("ecg_chart") {
                        let w = canvas.width;
                        let h = canvas.height;

                        if let Some(mut pixmap) = PixmapMut::from_bytes(&mut canvas.data, w, h) {
                            pixmap.fill(Color::from_rgba8(6, 15, 10, 255));

                            // Grid
                            let mut grid_paint = Paint::default();
                            grid_paint.set_color_rgba8(18, 42, 28, 255);
                            for gx in (0..w).step_by(25) {
                                if let Some(r) = Rect::from_xywh(gx as f32, 0.0, 1.0, h as f32) {
                                    pixmap.fill_rect(r, &grid_paint, Transform::identity(), None);
                                }
                            }
                            for gy in (0..h).step_by(25) {
                                if let Some(r) = Rect::from_xywh(0.0, gy as f32, w as f32, 1.0) {
                                    pixmap.fill_rect(r, &grid_paint, Transform::identity(), None);
                                }
                            }

                            // Baseline
                            let baseline_y = h as f32 / 2.0;
                            let mut base_paint = Paint::default();
                            base_paint.set_color_rgba8(30, 85, 50, 255);
                            if let Some(r) = Rect::from_xywh(0.0, baseline_y, w as f32, 1.0) {
                                pixmap.fill_rect(r, &base_paint, Transform::identity(), None);
                            }

                            // ECG trace line
                            let count = self.ecg_samples.len();
                            if count > 1 {
                                let x_step = w as f32 / (count as f32 - 1.0);
                                let mut wave_paint = Paint::default();
                                wave_paint.set_color_rgba8(0, 255, 128, 255);

                                for i in 0..(count - 1) {
                                    let x1 = i as f32 * x_step;
                                    let y1 = baseline_y - (self.ecg_samples[i] * 38.0);
                                    let x2 = (i + 1) as f32 * x_step;
                                    let y2 = baseline_y - (self.ecg_samples[i + 1] * 38.0);

                                    let dx = x2 - x1;
                                    let dy = y2 - y1;
                                    let steps = dx.hypot(dy).ceil() as usize;
                                    for s in 0..=steps.max(1) {
                                        let t = s as f32 / steps.max(1) as f32;
                                        let px = x1 + dx * t;
                                        let py = y1 + dy * t;
                                        if let Some(r) = Rect::from_xywh(px, py, 2.0, 2.0) {
                                            pixmap.fill_rect(r, &wave_paint, Transform::identity(), None);
                                        }
                                    }
                                }
                            }
                        }

                        canvas.dirty = true;
                    }
                }
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<ShowcaseModel, Measurer>, &mut [u32], u32, u32) + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = ShowcaseModel::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };

    let mut runtime = Runtime::new(model, measurer);

    runtime.set_interval("tick".to_string(), 200);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Showcase Dashboard", 820, 820, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Showcase Dashboard", 820, 820, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Showcase Dashboard", 820, 820, runtime, render_frame, |_| {})?;
    }

    Ok(())
}

