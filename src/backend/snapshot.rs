//! Headless rendering of an app to a PNG file.
//!
//! [`SnapshotBackend`] runs an app without a window: it applies the size,
//! dispatches a list of messages to reach the model state of interest,
//! advances a simulated clock (for timers and animations), waits for pending
//! image loads, renders one frame through the app's own render function and
//! writes it out. That makes the UI inspectable by tests, CI and AI agents
//! without opening and screenshotting a window.
//!
//! Every built-in backend checks the environment first, so any app can be
//! snapshotted without code changes:
//!
//! ```sh
//! XERUNE_SNAPSHOT=todo.png XERUNE_SNAPSHOT_MESSAGES="add;toggle:0" cargo run --example todo
//! ```
//!
//! - `XERUNE_SNAPSHOT`: output PNG path (enables snapshot mode)
//! - `XERUNE_SNAPSHOT_SIZE`: `WIDTHxHEIGHT`, defaults to the app's window size
//! - `XERUNE_SNAPSHOT_MESSAGES`: `;`-separated messages dispatched in order;
//!   an `@MS` item holds the messages after it until `MS` milliseconds of
//!   simulated time, so a recording can be scripted
//! - `XERUNE_SNAPSHOT_TIME_MS`: simulated time to run before rendering (default 0)
//! - `XERUNE_SNAPSHOT_POINTER`: `X,Y` where the pointer hovers, after the
//!   messages (for `:hover` and `data-on-hover`)
//! - `XERUNE_SNAPSHOT_FRAMES`: record this many frames instead of one, written
//!   as `out_0000.png`, `out_0001.png`, ... (the first at `TIME_MS`)
//! - `XERUNE_SNAPSHOT_FPS`: frames per second of a recording (default 30)

use std::path::PathBuf;
use std::time::Duration;

use super::{Backend, BackendError, Messenger};
use crate::{Model, Runtime, TextMeasurer};
use crate::runtime::Clock;

/// Upper bound on waiting for images (local or remote) before rendering anyway.
const IMAGE_TIMEOUT: Duration = Duration::from_secs(10);

/// Renders an app headlessly to a PNG file instead of opening a window.
#[derive(Debug, Clone)]
pub struct SnapshotBackend {
    /// Output PNG path.
    pub path: PathBuf,
    /// Size override; `None` uses the size passed to [`Backend::run`].
    pub size: Option<(u32, u32)>,
    /// Messages dispatched, in order, before rendering.
    pub messages: Vec<String>,
    /// Where the pointer hovers, after the messages.
    pub pointer: Option<(f32, f32)>,
    /// Simulated time to advance before rendering, letting timers fire and
    /// animations progress deterministically.
    pub time: Duration,
    /// Frames to record; more than one writes numbered PNGs.
    pub frames: u32,
    /// Frames per second of a recording.
    pub fps: u32,
}

impl SnapshotBackend {
    /// Snapshot to `path` at the app's own size, with no messages.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), size: None, messages: Vec::new(), pointer: None, time: Duration::ZERO, frames: 1, fps: 30 }
    }

    /// Read the snapshot configuration from the `XERUNE_SNAPSHOT*` environment
    /// variables (see the module docs). `None` unless `XERUNE_SNAPSHOT` is set.
    pub fn from_env() -> Option<Result<Self, BackendError>> {
        let path = std::env::var_os("XERUNE_SNAPSHOT")?;
        Some(Self::from_env_with_path(path.into()))
    }

    fn from_env_with_path(path: PathBuf) -> Result<Self, BackendError> {
        let bad = |var: &str, msg: &str| BackendError::Init(format!("{var}: {msg}"));
        let size = match std::env::var("XERUNE_SNAPSHOT_SIZE") {
            Ok(s) => {
                let (w, h) = s.split_once('x').ok_or_else(|| bad("XERUNE_SNAPSHOT_SIZE", "expected WIDTHxHEIGHT"))?;
                let parse = |v: &str| v.trim().parse::<u32>().ok().filter(|&n| n > 0);
                match (parse(w), parse(h)) {
                    (Some(w), Some(h)) => Some((w, h)),
                    _ => return Err(bad("XERUNE_SNAPSHOT_SIZE", "expected positive integers, e.g. 800x480")),
                }
            }
            Err(_) => None,
        };
        let messages = std::env::var("XERUNE_SNAPSHOT_MESSAGES")
            .map(|s| s.split(';').map(str::trim).filter(|m| !m.is_empty()).map(String::from).collect())
            .unwrap_or_default();
        let time = match std::env::var("XERUNE_SNAPSHOT_TIME_MS") {
            Ok(s) => Duration::from_millis(s.trim().parse().map_err(|_| bad("XERUNE_SNAPSHOT_TIME_MS", "expected milliseconds"))?),
            Err(_) => Duration::ZERO,
        };
        let count = |var: &str, default: u32| match std::env::var(var) {
            Ok(s) => s.trim().parse::<u32>().ok().filter(|&n| n > 0).ok_or_else(|| bad(var, "expected a positive integer")),
            Err(_) => Ok(default),
        };
        let frames = count("XERUNE_SNAPSHOT_FRAMES", 1)?;
        let fps = count("XERUNE_SNAPSHOT_FPS", 30)?;
        let pointer = match std::env::var("XERUNE_SNAPSHOT_POINTER") {
            Ok(s) => {
                let parsed = s.split_once(',').and_then(|(x, y)| Some((x.trim().parse().ok()?, y.trim().parse().ok()?)));
                Some(parsed.ok_or_else(|| bad("XERUNE_SNAPSHOT_POINTER", "expected X,Y, e.g. 120,80"))?)
            }
            Err(_) => None,
        };
        Ok(Self { path, size, messages, pointer, time, frames, fps })
    }

    /// Bring `runtime` into the configured state and render one frame,
    /// returning the `0x00RRGGBB` pixels and the size used.
    pub fn render<M, TM, R>(&self, runtime: &mut Runtime<M, TM>, renderer: &mut R, width: u32, height: u32) -> Result<(Vec<u32>, u32, u32), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        R: crate::Renderer,
    {
        let mut frame = None;
        Self { frames: 1, ..self.clone() }.record(runtime, renderer, width, height, |_, pixels, w, h| {
            frame = Some((pixels.to_vec(), w, h));
            Ok(())
        })?;
        Ok(frame.expect("one frame was rendered"))
    }

    /// Like [`render`](Self::render), but render [`frames`](Self::frames)
    /// frames at [`fps`](Self::fps), handing each to `on_frame` with its
    /// index. `@MS` items in the messages wait for that simulated time.
    pub fn record<M, TM, R>(
        &self,
        runtime: &mut Runtime<M, TM>,
        renderer: &mut R,
        width: u32,
        height: u32,
        mut on_frame: impl FnMut(u32, &[u32], u32, u32) -> Result<(), BackendError>,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        R: crate::Renderer,
    {
        let (width, height) = self.size.unwrap_or((width, height));
        let clock = crate::runtime::ManualClock::new();
        runtime.set_clock(clock.clone());
        runtime.set_size(width as f32, height as f32);

        // Each message with the time it is due at.
        let mut script = Vec::new();
        let mut at = Duration::ZERO;
        for message in &self.messages {
            if let Some(ms) = message.strip_prefix('@') {
                let ms: u64 = ms.trim().parse().map_err(|_| BackendError::Run(format!("expected @MILLISECONDS, got {message:?}")))?;
                at = Duration::from_millis(ms);
            } else if <M::Message as core::str::FromStr>::from_str(message).is_err() && !crate::keyboard::is_keyboard_message(message) {
                // Report typos instead of silently snapshotting the wrong state.
                return Err(BackendError::Run(format!("unknown message {message:?}")));
            } else {
                script.push((at, message.clone()));
            }
        }
        let mut script = script.into_iter().peekable();

        let start = clock.now();
        // Poll whenever the runtime has work, on a clock that moves only
        // here, so timers fire and frames come as they would live.
        let advance = |runtime: &mut Runtime<M, TM>, to: Duration| {
            let end = start + to;
            loop {
                match runtime.poll().wake.deadline() {
                    Some(at) if at <= end => clock.advance_to(at),
                    _ => break,
                }
            }
            clock.advance_to(end);
            runtime.poll();
        };

        let due_now: Vec<String> = core::iter::from_fn(|| script.next_if(|(at, _)| at.is_zero()).map(|(_, m)| m)).collect();
        runtime.handle_messages(due_now);
        if let Some((x, y)) = self.pointer {
            runtime.handle_event(crate::InputEvent::Hover { x, y });
        }

        let mut buffer = vec![0u32; width as usize * height as usize];
        for frame in 0..self.frames.max(1) {
            let time = self.time + Duration::from_secs(frame as u64) / self.fps.max(1);
            while let Some((at, message)) = script.next_if(|(at, _)| *at <= time) {
                advance(runtime, at);
                runtime.handle_messages([message]);
            }
            advance(runtime, time);
            runtime.wait_for_images(IMAGE_TIMEOUT);
            // The buffer is kept, so later frames repaint only what changed, as on a screen.
            runtime.render(renderer, &mut crate::Surface::new(crate::Pixels::Xrgb8888(&mut buffer), width, height));
            on_frame(frame, &buffer, width, height)?;
        }
        Ok(())
    }
}

impl SnapshotBackend {
    /// Where frame `n` of a recording goes: `out.png` becomes `out_0000.png`.
    fn frame_path(&self, n: u32) -> PathBuf {
        let stem = self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let ext = self.path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "png".into());
        self.path.with_file_name(format!("{stem}_{n:04}.{ext}"))
    }
}

/// Write `0x00RRGGBB` pixels as an RGB PNG.
pub fn write_png(path: &std::path::Path, pixels: &[u32], width: u32, height: u32) -> Result<(), BackendError> {
    let err = |e: &dyn std::fmt::Display| BackendError::Run(format!("writing {}: {e}", path.display()));
    let file = std::fs::File::create(path).map_err(|e| err(&e))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let rgb: Vec<u8> = pixels.iter().flat_map(|&p| [(p >> 16) as u8, (p >> 8) as u8, p as u8]).collect();
    let mut writer = encoder.write_header().map_err(|e| err(&e))?;
    writer.write_image_data(&rgb).map_err(|e| err(&e))
}

impl Backend for SnapshotBackend {
    fn run<M, TM, R>(
        self,
        _title: &str,
        width: u32,
        height: u32,
        mut runtime: Runtime<M, TM>,
        mut renderer: R,
        // Not run: producers it starts would make the output
        // nondeterministic. `XERUNE_SNAPSHOT_MESSAGES` reaches a state instead.
        _setup: impl FnOnce(Messenger<M>) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        R: crate::Renderer + 'static,
    {
        if self.frames <= 1 {
            let (pixels, w, h) = self.render(&mut runtime, &mut renderer, width, height)?;
            write_png(&self.path, &pixels, w, h)?;
            log::info!("xerune: wrote {}x{} snapshot to {}", w, h, self.path.display());
            return Ok(());
        }
        self.record(&mut runtime, &mut renderer, width, height, |frame, pixels, w, h| write_png(&self.frame_path(frame), pixels, w, h))?;
        log::info!("xerune: wrote {} frames to {}", self.frames, self.frame_path(0).display());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_round_trips_pixels() {
        let dir = std::env::temp_dir().join(format!("xerune-snapshot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("px.png");
        write_png(&path, &[0x00ff0000, 0x0000ff00, 0x000000ff, 0x00123456], 2, 2).unwrap();
        let image = crate::images::decode_png(&std::fs::read(&path).unwrap()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!((image.width, image.height), (2, 2));
        assert_eq!(&image.data[..8], &[255, 0, 0, 255, 0, 255, 0, 255]);
        assert_eq!(&image.data[12..16], &[0x12, 0x34, 0x56, 255]);
    }
}
