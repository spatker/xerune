//! Image resources.
//!
//! Images are owned by the [`Runtime`](crate::Runtime), not by renderers:
//!
//! * [`ImageStore`] tracks every `<img src>` referenced by the current view and holds the
//!   decoded pixels. Renderers only read from it (via [`RenderResources`](crate::graphics::RenderResources)).
//! * An [`ImageLoader`] turns a `src` into bytes (and bytes into an [`Image`]). It is plain
//!   blocking code; the store runs it on a small pool of worker threads, so loading never
//!   blocks the UI thread.
//! * When a load finishes, the store invokes the waker installed by the backend so the event
//!   loop wakes up, then [`Runtime::tick`](crate::Runtime::tick) picks up the result, rebuilds
//!   layout (images without an explicit size take their natural size) and requests a redraw.

use crate::alloc_prelude::*;

/// A decoded image: tightly packed, non-premultiplied RGBA8 pixels in row-major order.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// RGBA8 pixel data (`width * height * 4` bytes).
    pub data: Vec<u8>,
}

impl Image {
    /// Create an image from RGBA8 pixel data. Returns `None` if the buffer size does not match.
    pub fn from_rgba(width: u32, height: u32, data: Vec<u8>) -> Option<Self> {
        if width == 0 || height == 0 || data.len() != width as usize * height as usize * 4 {
            return None;
        }
        Some(Self { width, height, data })
    }
}

/// Load state of an image source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageStatus {
    /// Queued or being fetched/decoded in the background.
    Loading,
    /// Decoded and available via [`ImageStore::get`].
    Ready,
    /// Fetching or decoding failed. Not retried while the source stays referenced.
    Failed,
}

enum Entry {
    Loading,
    Ready { image: Image, pinned: bool },
    Failed,
}

/// Callback used to wake the backend's event loop when a background load completes.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Holds decoded images for the current view and drives their (background) loading.
pub struct ImageStore {
    entries: HashMap<String, Entry>,
    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    pool: pool::Pool,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageStore {
    /// Create a store. With `std` (on non-wasm targets) it uses [`DefaultImageLoader`];
    /// otherwise images must be supplied with [`ImageStore::insert`].
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
            pool: pool::Pool::new(Arc::new(DefaultImageLoader::default())),
        }
    }

    /// Decoded image for `src`, if it has finished loading.
    pub fn get(&self, src: &str) -> Option<&Image> {
        match self.entries.get(src) {
            Some(Entry::Ready { image, .. }) => Some(image),
            _ => None,
        }
    }

    /// Load state of `src`, or `None` if it is unknown to the store.
    pub fn status(&self, src: &str) -> Option<ImageStatus> {
        self.entries.get(src).map(|e| match e {
            Entry::Loading => ImageStatus::Loading,
            Entry::Ready { .. } => ImageStatus::Ready,
            Entry::Failed => ImageStatus::Failed,
        })
    }

    /// True while any image is still loading.
    pub fn has_pending(&self) -> bool {
        self.entries.values().any(|e| matches!(e, Entry::Loading))
    }

    /// Provide an image directly (e.g. bundled assets or `no_std` targets).
    ///
    /// Inserted images are pinned: they are kept even while no `<img>` references them.
    /// Call [`Runtime::sync_view`](crate::Runtime::sync_view) afterwards to relayout.
    pub fn insert(&mut self, src: impl Into<String>, image: Image) {
        self.entries.insert(src.into(), Entry::Ready { image, pinned: true });
    }

    /// Remove an image (pinned or not). It is fetched again if still referenced on the next rebuild.
    pub fn remove(&mut self, src: &str) -> Option<Image> {
        match self.entries.remove(src) {
            Some(Entry::Ready { image, .. }) => Some(image),
            _ => None,
        }
    }

    /// Request every referenced source not yet known and evict unreferenced, unpinned images.
    pub(crate) fn sync<'a>(&mut self, referenced: impl IntoIterator<Item = &'a str>) {
        let referenced: HashSet<&str> = referenced.into_iter().filter(|s| !s.is_empty()).collect();

        self.entries.retain(|src, entry| match entry {
            Entry::Ready { pinned: true, .. } | Entry::Loading => true,
            _ => referenced.contains(src.as_str()),
        });

        for src in referenced {
            if !self.entries.contains_key(src) {
                if self.request(src) {
                    self.entries.insert(src.to_string(), Entry::Loading);
                }
            }
        }
    }

    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    fn request(&mut self, src: &str) -> bool {
        self.pool.request(src)
    }

    #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
    fn request(&mut self, _src: &str) -> bool {
        false
    }

    /// Apply finished background loads. Returns true if anything changed.
    pub(crate) fn poll(&mut self) -> bool {
        #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
        {
            let mut changed = false;
            while let Some((src, result)) = self.pool.try_recv() {
                changed |= self.apply(src, result);
            }
            changed
        }
        #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
        {
            false
        }
    }

    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    fn apply(&mut self, src: String, result: Result<Image, ImageError>) -> bool {
        // Ignore results for sources dropped from the store in the meantime.
        if !matches!(self.entries.get(&src), Some(Entry::Loading)) {
            return false;
        }
        let entry = match result {
            Ok(image) => Entry::Ready { image, pinned: false },
            Err(e) => {
                log::warn!("Failed to load image {}: {}", src, e);
                Entry::Failed
            }
        };
        self.entries.insert(src, entry);
        true
    }
}

#[cfg(feature = "std")]
pub use loader::{decode_png, is_remote, DefaultImageLoader, ImageError, ImageLoader, MAX_IMAGE_BYTES};

#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
impl ImageStore {
    /// Replace the loader used for sources requested from now on.
    pub fn set_loader(&mut self, loader: impl ImageLoader) {
        self.pool.set_loader(Arc::new(loader));
    }

    /// Number of worker threads used for loading (default 2). Takes effect for new workers.
    pub fn set_worker_count(&mut self, workers: usize) {
        self.pool.set_worker_count(workers);
    }

    /// Install the callback that wakes the event loop when a load completes.
    pub fn set_waker(&mut self, waker: Waker) {
        self.pool.set_waker(waker);
    }

    /// Block until all pending images have settled or `timeout` elapses.
    /// Returns true if anything changed. Intended for headless rendering and tests.
    pub fn wait(&mut self, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut changed = self.poll();
        while self.has_pending() {
            let now = std::time::Instant::now();
            if now >= deadline {
                break;
            }
            match self.pool.recv_timeout(deadline - now) {
                Some((src, result)) => changed |= self.apply(src, result),
                None => break,
            }
        }
        changed
    }
}

#[cfg(feature = "std")]
mod loader {
    use super::Image;
    use std::fmt;
    use std::time::Duration;

    /// Maximum accepted size of an encoded image (16 MiB).
    pub const MAX_IMAGE_BYTES: u64 = 16 * 1024 * 1024;

    /// Errors produced while fetching or decoding an image.
    #[derive(Debug)]
    pub enum ImageError {
        /// Local file could not be read.
        Io(std::io::Error),
        /// Remote fetch failed (network error, bad status, ...).
        Http(String),
        /// A remote URL was requested but the `http` feature is disabled.
        HttpDisabled,
        /// The encoded image exceeded [`MAX_IMAGE_BYTES`].
        TooLarge,
        /// The bytes could not be decoded.
        Decode(String),
    }

    impl fmt::Display for ImageError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                ImageError::Io(e) => write!(f, "io error: {e}"),
                ImageError::Http(e) => write!(f, "http error: {e}"),
                ImageError::HttpDisabled => write!(f, "remote images require the `http` feature"),
                ImageError::TooLarge => write!(f, "image exceeds {MAX_IMAGE_BYTES} bytes"),
                ImageError::Decode(e) => write!(f, "decode error: {e}"),
            }
        }
    }

    impl std::error::Error for ImageError {}

    impl From<std::io::Error> for ImageError {
        fn from(e: std::io::Error) -> Self {
            ImageError::Io(e)
        }
    }

    /// Turns an image `src` into pixels. Implementations are plain blocking code: they run on
    /// the [`ImageStore`](super::ImageStore)'s worker threads, never on the UI thread.
    pub trait ImageLoader: Send + Sync + 'static {
        /// Fetch the encoded bytes for `src` (file read, HTTP request, asset bundle, ...).
        fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError>;

        /// Decode fetched bytes. Defaults to PNG; override to support other formats.
        fn decode(&self, _src: &str, bytes: &[u8]) -> Result<Image, ImageError> {
            decode_png(bytes)
        }
    }

    /// Returns true if `src` is an `http://` or `https://` URL.
    pub fn is_remote(src: &str) -> bool {
        let s = src.trim_start().as_bytes();
        let starts = |p: &str| s.len() >= p.len() && s[..p.len()].eq_ignore_ascii_case(p.as_bytes());
        starts("http://") || starts("https://")
    }

    /// Loader for local paths, `file://` URLs and (with the `http` feature) `http(s)://` URLs.
    #[derive(Debug, Clone)]
    pub struct DefaultImageLoader {
        /// Timeout applied to remote requests.
        pub timeout: Duration,
    }

    impl Default for DefaultImageLoader {
        fn default() -> Self {
            Self { timeout: Duration::from_secs(10) }
        }
    }

    impl DefaultImageLoader {
        /// Set the remote request timeout.
        pub fn with_timeout(mut self, timeout: Duration) -> Self {
            self.timeout = timeout;
            self
        }

        fn fetch_local(&self, src: &str) -> Result<Vec<u8>, ImageError> {
            // A leading '/' is treated as relative to the working directory (asset root).
            let path = match src.strip_prefix("file://") {
                Some(p) => p,
                None => src.strip_prefix('/').unwrap_or(src),
            };
            let bytes = std::fs::read(path)?;
            if bytes.len() as u64 > MAX_IMAGE_BYTES {
                return Err(ImageError::TooLarge);
            }
            Ok(bytes)
        }

        #[cfg(feature = "http")]
        fn fetch_remote(&self, url: &str) -> Result<Vec<u8>, ImageError> {
            use std::io::Read;
            let agent = ureq::AgentBuilder::new().timeout(self.timeout).build();
            let resp = agent.get(url).call().map_err(|e| ImageError::Http(e.to_string()))?;
            let mut buf = Vec::new();
            resp.into_reader().take(MAX_IMAGE_BYTES + 1).read_to_end(&mut buf)?;
            if buf.len() as u64 > MAX_IMAGE_BYTES {
                return Err(ImageError::TooLarge);
            }
            Ok(buf)
        }

        #[cfg(not(feature = "http"))]
        fn fetch_remote(&self, _url: &str) -> Result<Vec<u8>, ImageError> {
            Err(ImageError::HttpDisabled)
        }
    }

    impl ImageLoader for DefaultImageLoader {
        fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
            if is_remote(src) {
                self.fetch_remote(src.trim())
            } else {
                self.fetch_local(src)
            }
        }
    }

    /// Decode PNG bytes into an RGBA8 [`Image`].
    pub fn decode_png(bytes: &[u8]) -> Result<Image, ImageError> {
        let err = |e: png::DecodingError| ImageError::Decode(e.to_string());
        let mut decoder = png::Decoder::new(bytes);
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().map_err(err)?;
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).map_err(err)?;
        let px = &buf[..info.buffer_size()];
        let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
        match info.color_type {
            png::ColorType::Rgba => rgba.extend_from_slice(px),
            png::ColorType::Rgb => px.chunks_exact(3).for_each(|c| rgba.extend_from_slice(&[c[0], c[1], c[2], 255])),
            png::ColorType::GrayscaleAlpha => px.chunks_exact(2).for_each(|c| rgba.extend_from_slice(&[c[0], c[0], c[0], c[1]])),
            png::ColorType::Grayscale => px.iter().for_each(|&g| rgba.extend_from_slice(&[g, g, g, 255])),
            // EXPAND converts palette images to RGB(A), so this is not expected.
            png::ColorType::Indexed => return Err(ImageError::Decode("unexpanded indexed PNG".into())),
        }
        Image::from_rgba(info.width, info.height, rgba).ok_or_else(|| ImageError::Decode("empty image".into()))
    }
}

/// Background worker pool. Workers are spawned lazily on the first request and exit when
/// the pool (i.e. the owning [`ImageStore`]) is dropped.
#[cfg(all(feature = "std", not(target_arch = "wasm32")))]
mod pool {
    use super::{Image, ImageError, ImageLoader, Waker};
    use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
    use std::sync::{Arc, Mutex};

    type Job = (String, Arc<dyn ImageLoader>);
    type Done = (String, Result<Image, ImageError>);

    pub(super) struct Pool {
        loader: Arc<dyn ImageLoader>,
        workers: usize,
        spawned: usize,
        jobs: Option<Sender<Job>>,
        job_rx: Arc<Mutex<Receiver<Job>>>,
        done_tx: Sender<Done>,
        done_rx: Receiver<Done>,
        waker: Arc<Mutex<Option<Waker>>>,
    }

    impl Pool {
        pub(super) fn new(loader: Arc<dyn ImageLoader>) -> Self {
            let (jobs, job_rx) = channel();
            let (done_tx, done_rx) = channel();
            Self {
                loader,
                workers: 2,
                spawned: 0,
                jobs: Some(jobs),
                job_rx: Arc::new(Mutex::new(job_rx)),
                done_tx,
                done_rx,
                waker: Arc::new(Mutex::new(None)),
            }
        }

        pub(super) fn set_loader(&mut self, loader: Arc<dyn ImageLoader>) {
            self.loader = loader;
        }

        pub(super) fn set_worker_count(&mut self, workers: usize) {
            self.workers = workers.max(1);
        }

        pub(super) fn set_waker(&mut self, waker: Waker) {
            *self.waker.lock().unwrap() = Some(waker);
        }

        pub(super) fn request(&mut self, src: &str) -> bool {
            while self.spawned < self.workers {
                if !self.spawn_worker() {
                    break;
                }
            }
            if self.spawned == 0 {
                return false;
            }
            match &self.jobs {
                Some(tx) => tx.send((src.to_string(), self.loader.clone())).is_ok(),
                None => false,
            }
        }

        fn spawn_worker(&mut self) -> bool {
            let job_rx = self.job_rx.clone();
            let done_tx = self.done_tx.clone();
            let waker = self.waker.clone();
            let spawned = std::thread::Builder::new()
                .name("xerune-image-loader".into())
                .spawn(move || loop {
                    let job = job_rx.lock().unwrap().recv();
                    let Ok((src, loader)) = job else { break };
                    let result = loader.fetch(&src).and_then(|bytes| loader.decode(&src, &bytes));
                    if done_tx.send((src, result)).is_err() {
                        break;
                    }
                    let waker = waker.lock().unwrap().clone();
                    if let Some(wake) = waker {
                        wake();
                    }
                });
            match spawned {
                Ok(_) => {
                    self.spawned += 1;
                    true
                }
                Err(e) => {
                    log::warn!("Failed to spawn image loader thread: {}", e);
                    false
                }
            }
        }

        pub(super) fn try_recv(&self) -> Option<Done> {
            self.done_rx.try_recv().ok()
        }

        pub(super) fn recv_timeout(&self, timeout: std::time::Duration) -> Option<Done> {
            match self.done_rx.recv_timeout(timeout) {
                Ok(done) => Some(done),
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => None,
            }
        }
    }

    impl Drop for Pool {
        fn drop(&mut self) {
            // Closing the job channel lets idle workers exit.
            self.jobs.take();
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, w, h);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc.write_header().unwrap();
            writer.write_image_data(&vec![255u8; (w * h * 4) as usize]).unwrap();
        }
        out
    }

    #[test]
    fn detects_remote() {
        assert!(is_remote("https://example.com/a.png"));
        assert!(is_remote("HTTP://example.com/a.png"));
        assert!(!is_remote("/res/a.png"));
        assert!(!is_remote("file:///a.png"));
    }

    #[test]
    fn decodes_png() {
        let img = decode_png(&png_bytes(3, 2)).unwrap();
        assert_eq!((img.width, img.height, img.data.len()), (3, 2, 24));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn loads_in_background_and_evicts() {
        struct Mem;
        impl ImageLoader for Mem {
            fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
                match src {
                    "ok" => Ok(png_bytes(4, 4)),
                    _ => Err(ImageError::Decode("nope".into())),
                }
            }
        }
        let mut store = ImageStore::new();
        store.set_loader(Mem);
        store.sync(["ok", "bad"]);
        assert_eq!(store.status("ok"), Some(ImageStatus::Loading));
        assert!(store.wait(std::time::Duration::from_secs(5)));
        assert_eq!(store.get("ok").map(|i| i.width), Some(4));
        assert_eq!(store.status("bad"), Some(ImageStatus::Failed));

        store.sync(["bad"]);
        assert_eq!(store.status("ok"), None);
    }
}
