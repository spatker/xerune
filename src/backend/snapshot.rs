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
//! - `XERUNE_SNAPSHOT_MESSAGES`: `;`-separated messages dispatched in order
//! - `XERUNE_SNAPSHOT_TIME_MS`: simulated time to run before rendering (default 0)

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{Backend, BackendError, EventProxy, SendError};
use crate::{graphics::Rect, Model, Runtime, TextMeasurer};

/// Frame step used when advancing the simulated clock.
const FRAME: Duration = Duration::from_millis(16);
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
    /// Simulated time to advance before rendering, letting timers fire and
    /// animations progress deterministically.
    pub time: Duration,
}

impl SnapshotBackend {
    /// Snapshot to `path` at the app's own size, with no messages.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into(), size: None, messages: Vec::new(), time: Duration::ZERO }
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
        Ok(Self { path, size, messages, time })
    }

    /// Bring `runtime` into the configured state and render one frame,
    /// returning the `0x00RRGGBB` pixels and the size used.
    pub fn render<M, TM, F>(&self, runtime: &mut Runtime<M, TM>, render_fn: &mut F, width: u32, height: u32) -> Result<(Vec<u32>, u32, u32), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) -> Option<Rect>,
    {
        let (width, height) = self.size.unwrap_or((width, height));
        runtime.set_size(width as f32, height as f32);

        for message in &self.messages {
            // Report typos instead of silently snapshotting the wrong state.
            if <M::Message as core::str::FromStr>::from_str(message).is_err() {
                return Err(BackendError::Run(format!("unknown message {message:?}")));
            }
        }
        runtime.handle_messages(self.messages.iter().cloned());

        let start = Instant::now();
        let mut elapsed = Duration::ZERO;
        runtime.tick_with_time(start);
        while elapsed < self.time {
            elapsed = (elapsed + FRAME).min(self.time);
            runtime.tick_with_time(start + elapsed);
        }

        runtime.wait_for_images(IMAGE_TIMEOUT);

        let mut buffer = vec![0u32; width as usize * height as usize];
        render_fn(runtime, &mut buffer, width, height);
        Ok((buffer, width, height))
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

/// Proxy handed to nothing: snapshot mode does not run the app's `setup`
/// callback (it typically spawns producers that would make the output
/// nondeterministic). Use `XERUNE_SNAPSHOT_MESSAGES` to reach a state instead.
#[derive(Clone, Debug)]
pub struct SnapshotProxy;

impl EventProxy for SnapshotProxy {
    fn send_message(&self, _message: String) -> Result<(), SendError> {
        Err(SendError("snapshot mode has no event loop".to_string()))
    }
}

impl Backend for SnapshotBackend {
    type Proxy = SnapshotProxy;

    fn run<M, TM, F>(
        self,
        _title: &str,
        width: u32,
        height: u32,
        mut runtime: Runtime<M, TM>,
        mut render_fn: F,
        _setup: impl FnOnce(Self::Proxy) + 'static,
    ) -> Result<(), BackendError>
    where
        M: Model + crate::ui::TemplateLayout + 'static,
        TM: TextMeasurer + 'static,
        F: FnMut(&mut Runtime<M, TM>, &mut [u32], u32, u32) -> Option<Rect> + 'static,
    {
        let (pixels, w, h) = self.render(&mut runtime, &mut render_fn, width, height)?;
        write_png(&self.path, &pixels, w, h)?;
        log::info!("xerune: wrote {}x{} snapshot to {}", w, h, self.path.display());
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
