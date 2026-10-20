//! The C API of xerune: HTML/CSS templates rendered into the caller's
//! pixel buffer, with the model as JSON and the view's messages as strings.
//!
//! `include/xerune.h` is generated from this file (`tests/header.rs`); its
//! comments are these doc comments, so they are written for C programmers.
//!
//! Every function catches Rust panics: a bug in xerune shows as an error
//! (`xr_last_error`), never as an unwind into the caller.

mod engine;

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::time::Duration;

use xerune::{InputEvent, Pixels, Surface};
use xerune_render::Fonts;

pub use engine::{Engine, JsonModel, Page, Ui};

/// Pixels are 32-bit words `0xXXRRGGBB` (blue in the low byte; the top
/// byte is not written): DRM/fbdev XRGB8888, `GL_BGRA` with
/// `GL_UNSIGNED_INT_8_8_8_8_REV`, SDL_PIXELFORMAT_XRGB8888.
pub const XR_PIXELS_XRGB8888: u32 = 0;
/// Pixels are 32-bit words `0xXXBBGGRR` (red in the low byte; the top byte
/// is not written): in memory on little-endian machines R, G, B, X, as
/// `GL_RGBA` / `GL_UNSIGNED_BYTE` reads them.
pub const XR_PIXELS_XBGR8888: u32 = 1;

/// A UI: a template, the model it shows, and its state between frames
/// (scroll positions, animations, the previous frame). Create one with
/// `xr_new`, free it with `xr_free`.
///
/// A UI is used from one thread at a time.
pub struct XrUi {
    engine: Box<dyn Engine>,
    /// The message `xr_next_message` returned last.
    message: Option<CString>,
    /// The pointer is down (between `xr_pointer_down` and `xr_pointer_up`).
    pressed: bool,
}

/// A font: TrueType or OpenType data for normal text and, optionally, for
/// bold text. xerune copies the data; it needn't outlive the call.
#[repr(C)]
pub struct XrFont {
    /// The font for normal text.
    pub regular: *const u8,
    pub regular_len: usize,
    /// The font for bold text, or NULL to draw bold text with `regular`.
    pub bold: *const u8,
    pub bold_len: usize,
}

/// A rectangle in pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct XrRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_error(message: &str) {
    let message = CString::new(message.replace('\0', "\\0")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = message);
}

/// Runs `f`, turning its error or panic into `xr_last_error` and `failed`.
fn guard<T>(failed: T, f: impl FnOnce() -> Result<T, String>) -> T {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(value)) => value,
        Ok(Err(message)) => {
            set_error(&message);
            failed
        }
        Err(panic) => {
            let message = panic.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| panic.downcast_ref::<String>().cloned());
            set_error(&format!("xerune panicked: {}", message.as_deref().unwrap_or("(no message)")));
            failed
        }
    }
}

fn ui<'a>(ui: *mut XrUi) -> Result<&'a mut XrUi, String> {
    // SAFETY: the caller passes a pointer from `xr_new` that it hasn't freed.
    unsafe { ui.as_mut() }.ok_or_else(|| "the UI is NULL".to_string())
}

fn str_arg<'a>(s: *const c_char, what: &str) -> Result<&'a str, String> {
    if s.is_null() {
        return Err(format!("{what} is NULL"));
    }
    // SAFETY: the caller passes a NUL-terminated string.
    unsafe { CStr::from_ptr(s) }.to_str().map_err(|_| format!("{what} isn't UTF-8"))
}

fn bytes<'a>(data: *const u8, len: usize) -> Option<&'a [u8]> {
    // SAFETY: the caller passes `len` readable bytes at `data`.
    (!data.is_null() && len > 0).then(|| unsafe { std::slice::from_raw_parts(data, len) })
}

/// Roboto, which xerune's examples and tests use.
fn bundled_fonts() -> Fonts {
    Fonts::ttf(
        include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
    )
    .expect("bundled Roboto")
}

fn fonts(fonts: *const XrFont, count: usize) -> Result<Fonts, String> {
    if fonts.is_null() || count == 0 {
        return Ok(bundled_fonts());
    }
    // SAFETY: the caller passes `count` fonts at `fonts`.
    let fonts = unsafe { std::slice::from_raw_parts(fonts, count) };
    let mut set: Option<Fonts> = None;
    for (i, font) in fonts.iter().enumerate() {
        let regular = bytes(font.regular, font.regular_len).ok_or_else(|| format!("font {i} has no regular font data"))?;
        let bold = bytes(font.bold, font.bold_len);
        let next = match set {
            None => Fonts::ttf(regular, bold),
            Some(set) => set.with_fallback(regular, bold),
        };
        set = Some(next.map_err(|e| format!("font {i}: {e:?}"))?);
    }
    set.ok_or_else(|| "no fonts".to_string())
}

/// Wraps an engine in a handle for C. Crates of compiled templates export
/// their own constructor with this; every other `xr_` function works on
/// what it returns.
///
/// # Safety
/// `fonts` must be NULL or point to `count` valid `XrFont`s.
pub unsafe fn new_ui<M: JsonModel + 'static>(model: M, fonts: *const XrFont, count: usize) -> *mut XrUi {
    guard(std::ptr::null_mut(), || {
        let fonts = self::fonts(fonts, count)?;
        let engine: Box<dyn Engine> = Box::new(Ui::new(model, fonts));
        Ok(Box::into_raw(Box::new(XrUi { engine, message: None, pressed: false })))
    })
}

/// The version of xerune, e.g. "0.1.0".
#[unsafe(no_mangle)]
pub extern "C" fn xr_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr().cast()
}

/// The error of the last call on this thread that failed, as text. It
/// stays until the next call on this thread fails.
#[unsafe(no_mangle)]
pub extern "C" fn xr_last_error() -> *const c_char {
    // The string lives in the thread-local, which outlives the call.
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// A new UI drawing text with `fonts`: the first is the text font, the
/// others fallbacks for characters it lacks (e.g. a CJK font). With NULL
/// (or `count` 0) text is drawn with the bundled Roboto. NULL on an error.
///
/// The UI shows nothing until it has a template (`xr_load_template`) and a
/// size (`xr_set_size`).
///
/// # Safety
/// `fonts` must be NULL or point to `count` valid `XrFont`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_new(fonts: *const XrFont, count: usize) -> *mut XrUi {
    // SAFETY: as the caller promised.
    unsafe { new_ui(Page::default(), fonts, count) }
}

/// Frees `ui` (NULL is fine). Strings it returned are gone with it.
///
/// # Safety
/// `ui` must be NULL or from `xr_new`, and not freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_free(ui: *mut XrUi) {
    if !ui.is_null() {
        guard((), || {
            // SAFETY: as the caller promised.
            drop(unsafe { Box::from_raw(ui) });
            Ok(())
        })
    }
}

/// Shows the template in the file at `path` from now on (UTF-8 HTML with
/// `{{ }}`, `{% if %}`, `{% for %}`, `{% include %}`; plain HTML is a
/// template too). Includes and images are found relative to its directory.
/// Loading a template again (an edited file) keeps the UI's state.
///
/// 0 on success; -1 on an error (`xr_last_error`), and the UI shows what
/// it showed before.
///
/// # Safety
/// `ui` must be from `xr_new`; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_load_template(ui: *mut XrUi, path: *const c_char) -> i32 {
    guard(-1, || {
        let ui = self::ui(ui)?;
        let path = Path::new(str_arg(path, "the path")?);
        let content = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let template = engine::parse_template(&path.display().to_string(), content, &dir)?;
        ui.engine.set_template(template, &dir)?;
        Ok(0)
    })
}

/// Like `xr_load_template`, with the template's text in `source`; includes
/// and images are found relative to `base_dir` (NULL: the working
/// directory).
///
/// # Safety
/// `ui` must be from `xr_new`; `source` and `base_dir` NUL-terminated
/// strings (`base_dir` may be NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_load_template_source(ui: *mut XrUi, source: *const c_char, base_dir: *const c_char) -> i32 {
    guard(-1, || {
        let ui = self::ui(ui)?;
        let source = str_arg(source, "the source")?;
        let dir = if base_dir.is_null() { PathBuf::new() } else { PathBuf::from(str_arg(base_dir, "the base directory")?) };
        let template = engine::parse_template("<source>", source.to_string(), &dir)?;
        ui.engine.set_template(template, &dir)?;
        Ok(0)
    })
}

/// Sets the model the template shows: a JSON object whose fields the
/// template reads (`{{ score }}`, `{% for item in items %}`). Integers are
/// 64-bit, `null` is `None` (`{% if let Some(name) = player %}`), and
/// conditions must be `true`/`false`. Set the whole model each time; the
/// view is rebuilt from it.
///
/// 0 on success; -1 on an error (not JSON, or the template reads a field
/// it lacks: `xr_last_error`), and the UI shows what it showed before.
///
/// # Safety
/// `ui` must be from `xr_new`; `json` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_set_model(ui: *mut XrUi, json: *const c_char) -> i32 {
    guard(-1, || {
        let ui = self::ui(ui)?;
        ui.engine.set_json(str_arg(json, "the model")?)?;
        Ok(0)
    })
}

/// Sets the size of the UI in pixels: the size of the buffer `xr_render`
/// draws into, and the viewport of the template's CSS (`@media`, `vw`,
/// `vh`).
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_set_size(ui: *mut XrUi, width: u32, height: u32) {
    guard((), || {
        self::ui(ui)?.engine.set_size(width, height);
        Ok(())
    })
}

/// Moves the UI's time on to `now_ms`, a monotonic time in milliseconds
/// (any origin; the first call is time 0), and does what is due: CSS
/// animations and transitions step, loaded images appear. Call it once a
/// frame, before `xr_render`.
///
/// Returns true if the UI changed and should be rendered.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_tick(ui: *mut XrUi, now_ms: u64) -> bool {
    guard(false, || Ok(self::ui(ui)?.engine.tick(now_ms)))
}

/// Draws what changed since the last call into `pixels`, a buffer of the
/// UI's size (`xr_set_size`) whose rows are `stride` pixels apart, in
/// `format` (`XR_PIXELS_...`). The buffer must hold the frame the last call
/// drew (call `xr_repaint_all` when it doesn't). The first call, and the
/// first after a resize, draw everything.
///
/// Returns true if it drew anything; then `damage` (if not NULL) is the
/// area it drew, e.g. the part of a texture to upload.
///
/// # Safety
/// `ui` must be from `xr_new`; `pixels` must hold `stride * height`
/// 32-bit pixels; `damage` must be NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_render(ui: *mut XrUi, pixels: *mut u32, stride: usize, format: u32, damage: *mut XrRect) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        let (width, height) = ui.engine.size();
        if width == 0 || height == 0 {
            return Err("the UI has no size: call xr_set_size first".to_string());
        }
        if pixels.is_null() {
            return Err("the pixel buffer is NULL".to_string());
        }
        if stride < width as usize {
            return Err(format!("the stride {stride} is less than the width {width}"));
        }
        // SAFETY: the caller passes a buffer of `stride * height` pixels.
        let buffer = unsafe { std::slice::from_raw_parts_mut(pixels, stride * height as usize) };
        let pixels = match format {
            XR_PIXELS_XRGB8888 => Pixels::Xrgb8888(buffer),
            XR_PIXELS_XBGR8888 => Pixels::Xbgr8888(buffer),
            other => return Err(format!("unknown pixel format {other}")),
        };
        let mut target = Surface::new(pixels, width, height).with_stride(stride);
        let drawn = ui.engine.render(&mut target);
        let Some(bounds) = drawn.bounds() else { return Ok(false) };
        if !damage.is_null() {
            // Whole pixels covering the area, on the buffer.
            let x0 = (bounds.x.floor().max(0.0) as i32).min(width as i32);
            let y0 = (bounds.y.floor().max(0.0) as i32).min(height as i32);
            let x1 = (bounds.x + bounds.width).ceil().clamp(x0 as f32, width as f32) as i32;
            let y1 = (bounds.y + bounds.height).ceil().clamp(y0 as f32, height as f32) as i32;
            // SAFETY: the caller passes a writable rect.
            unsafe { *damage = XrRect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 } };
        }
        Ok(true)
    })
}

/// Makes the next `xr_render` draw everything: for a buffer that doesn't
/// hold the last frame (a new one, or one shared with other drawing).
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_repaint_all(ui: *mut XrUi) {
    guard((), || {
        self::ui(ui)?.engine.repaint_all();
        Ok(())
    })
}

/// The pixels of the view's `<canvas id="...">`, for the program to draw
/// into: `*width` x `*height` pixels of 4 bytes, R, G, B, A in memory
/// (straight alpha; transparent pixels show what is behind the canvas),
/// rows `*width * 4` bytes apart. The next `xr_render` draws the canvas,
/// scaled to the element's box. NULL if the view has no such canvas
/// (`xr_last_error`).
///
/// The canvas's size is its `width` and `height` attributes (or its CSS
/// size in px; 200 x 200 without either) when the view first showed it.
/// The canvas keeps its pixels between frames; the pointer is valid until
/// the next call on `ui`, so call this each frame the canvas changes.
///
/// # Safety
/// `ui` must be from `xr_new`; `id` a NUL-terminated string; `width` and
/// `height` NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_canvas(ui: *mut XrUi, id: *const c_char, width: *mut u32, height: *mut u32) -> *mut u8 {
    guard(std::ptr::null_mut(), || {
        let ui = self::ui(ui)?;
        let id = str_arg(id, "the canvas id")?;
        let canvas = ui.engine.canvas(id).ok_or_else(|| format!("the view has no <canvas id=\"{id}\">"))?;
        // SAFETY: the caller passes NULL or writable sizes.
        unsafe {
            if let Some(width) = width.as_mut() {
                *width = canvas.width;
            }
            if let Some(height) = height.as_mut() {
                *height = canvas.height;
            }
        }
        Ok(canvas.data.as_mut_ptr())
    })
}

/// The pointer (mouse or touch, in pixels) moved to (`x`, `y`): hover,
/// dragging a slider, scrolling by dragging while down. Returns true if
/// the UI changed.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_pointer_move(ui: *mut XrUi, x: f32, y: f32) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        let event = if ui.pressed { InputEvent::TouchMove { id: 0, x, y } } else { InputEvent::Hover { x, y } };
        Ok(ui.engine.handle(event))
    })
}

/// The pointer went down at (`x`, `y`). Returns true if the UI changed.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_pointer_down(ui: *mut XrUi, x: f32, y: f32) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        if ui.pressed {
            return Ok(false);
        }
        ui.pressed = true;
        Ok(ui.engine.handle(InputEvent::TouchStart { id: 0, x, y }))
    })
}

/// The pointer went up at (`x`, `y`): a click if it didn't move far since
/// it went down, which sends the element's `data-on-click` message
/// (`xr_next_message`). Returns true if the UI changed.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_pointer_up(ui: *mut XrUi, x: f32, y: f32) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        if !ui.pressed {
            return Ok(false);
        }
        ui.pressed = false;
        Ok(ui.engine.handle(InputEvent::TouchEnd { id: 0, x, y }))
    })
}

/// Scrolls the scrollable element under (`x`, `y`) by (`dx`, `dy`) pixels
/// (a mouse wheel: positive `dy` scrolls down). Returns true if the UI
/// changed.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_scroll(ui: *mut XrUi, x: f32, y: f32, dx: f32, dy: f32) -> bool {
    guard(false, || Ok(self::ui(ui)?.engine.handle(InputEvent::Scroll { x, y, delta_x: dx, delta_y: dy })))
}

/// Whether (`x`, `y`) is on the UI rather than on the bare page: on an
/// element that takes clicks or any element inside `<body>`. A game
/// drawing the UI over its scene sends the pointer to the UI only where
/// this is true.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_hit(ui: *mut XrUi, x: f32, y: f32) -> bool {
    guard(false, || Ok(self::ui(ui)?.engine.hit(x, y)))
}

/// A key went down. `key` is the key's code as browsers name it
/// (`KeyboardEvent.code`: "KeyA", "Digit1", "Enter", "Escape",
/// "ArrowUp", "Space", ...). The UI sends the message "keydown:<key>".
/// Returns true if the UI changed.
///
/// # Safety
/// `ui` must be from `xr_new`; `key` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_key_down(ui: *mut XrUi, key: *const c_char) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        Ok(ui.engine.handle(InputEvent::KeyDown(str_arg(key, "the key")?.to_string())))
    })
}

/// A key went up; the UI sends the message "keyup:<key>". See
/// `xr_key_down`.
///
/// # Safety
/// `ui` must be from `xr_new`; `key` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_key_up(ui: *mut XrUi, key: *const c_char) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        Ok(ui.engine.handle(InputEvent::KeyUp(str_arg(key, "the key")?.to_string())))
    })
}

/// Text was typed (UTF-8). If a text `<input id="name">` has the focus
/// (it was clicked), the UI sends the message "name:text:<text>"; the
/// model sets the input's `value`. Returns true if the UI changed.
///
/// # Safety
/// `ui` must be from `xr_new`; `text` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_text_input(ui: *mut XrUi, text: *const c_char) -> bool {
    guard(false, || {
        let ui = self::ui(ui)?;
        Ok(ui.engine.handle(InputEvent::TextInput { id: String::new(), text: str_arg(text, "the text")?.to_string() }))
    })
}

/// The next message the UI sent, oldest first, or NULL when there is none.
/// Messages are the `data-on-click` (and `data-on-input`, `data-on-change`,
/// `data-on-hover`, `data-on-transitionend`, ...) attributes of the
/// elements, e.g. "buy:3" for `data-on-click="buy:{{ item.id }}"`; ranges
/// append ":<value>". Handle each by updating the model (`xr_set_model`).
///
/// The string is valid until the next call of `xr_next_message` or
/// `xr_free`.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_next_message(ui: *mut XrUi) -> *const c_char {
    guard(std::ptr::null(), || {
        let ui = self::ui(ui)?;
        ui.message = ui.engine.next_message().map(|m| CString::new(m.replace('\0', "\\0")).unwrap_or_default());
        Ok(ui.message.as_ref().map_or(std::ptr::null(), |m| m.as_ptr()))
    })
}

/// Waits up to `timeout_ms` for the images the view shows to load (they
/// load in the background and appear on a later `xr_tick`): for a first
/// frame or a screenshot that should have them. Returns true if all
/// loaded (or failed) in time.
///
/// # Safety
/// `ui` must be from `xr_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_wait_for_images(ui: *mut XrUi, timeout_ms: u32) -> bool {
    guard(false, || Ok(self::ui(ui)?.engine.wait_for_images(Duration::from_millis(timeout_ms.into()))))
}
