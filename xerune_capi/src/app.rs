//! `XrApp`: a UI that shows itself. Its model runs on a thread of its own
//! in a xerune [`App`], which draws to the display (DRM/KMS, fbdev, or a
//! window on a desktop) and reads the input devices, as the Rust examples
//! do. The program only sets the model and takes the messages, from its own
//! thread and event loop.

use std::ffi::{CString, c_char};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{TrySendError, sync_channel};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::thread::JoinHandle;

use xerune::backend::Messenger;
use xerune::{App, Runtime};
use xerune_render::{Fonts, SoftwareRenderer};
use xerune_template::Template;

use crate::engine::{self, JsonModel, Outbox, Page, Signal, TemplateDirLoader};
use crate::{XrFont, guard, str_arg};

/// A UI that shows itself on the display and reads the input devices, on a
/// thread of its own. The program sets its model and takes its messages,
/// from one thread at a time. Create one with `xr_app_new`, free it with
/// `xr_app_free`.
pub struct XrApp {
    app: Box<dyn AppEngine>,
    /// The message `xr_app_next_message` returned last.
    message: Option<CString>,
    /// The error `xr_app_next_error` returned last.
    error: Option<CString>,
}

/// An `XrApp`, whatever its model.
trait AppEngine {
    fn set_template(&mut self, template: Template, dir: PathBuf) -> Result<(), String>;
    fn set_json(&mut self, json: String) -> Result<(), String>;
    fn start(&mut self, title: String, width: u32, height: u32) -> Result<(), String>;
    fn running(&self) -> bool;
    fn outbox(&self) -> &Outbox;
    fn fd(&self) -> i32;
    fn scroll_into_view(&mut self, id: String) -> Result<(), String>;
}

/// What the program set that the model hasn't taken yet. Only the latest
/// counts: a model set twice before the app got to it is shown once.
#[derive(Default)]
struct Pending {
    template: Option<(Template, PathBuf)>,
    json: Option<String>,
    /// An update that takes these is queued in the app.
    queued: bool,
}

/// Gives `model` what is pending.
fn apply_pending<M: JsonModel>(pending: &Mutex<Pending>, model: &mut M, image_dir: &RwLock<PathBuf>) -> Result<(), String> {
    let (template, json) = {
        let mut pending = pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.queued = false;
        (pending.template.take(), pending.json.take())
    };
    let dir = match (template, json) {
        (Some((template, dir)), Some(json)) => {
            model.set_template_and_json(template, &dir, &json)?;
            dir
        }
        (Some((template, dir)), None) => {
            model.set_template(template, &dir)?;
            dir
        }
        (None, Some(json)) => return model.set_json(&json),
        (None, None) => return Ok(()),
    };
    *image_dir.write().unwrap_or_else(PoisonError::into_inner) = dir;
    Ok(())
}

struct AppOf<M: JsonModel> {
    /// Makes the model, on the app's thread; taken by `start`.
    make: Option<Box<dyn FnOnce() -> M + Send>>,
    fonts: Fonts,
    pending: Arc<Mutex<Pending>>,
    outbox: Outbox,
    signal: Option<Arc<Signal>>,
    /// The directory the template's images are in.
    image_dir: Arc<RwLock<PathBuf>>,
    loader: Option<TemplateDirLoader>,
    /// The app's loop runs.
    alive: Arc<AtomicBool>,
    messenger: Option<Messenger<M>>,
    thread: Option<JoinHandle<()>>,
}

impl<M: JsonModel + 'static> AppOf<M> {
    fn new(make: Box<dyn FnOnce() -> M + Send>, fonts: Fonts) -> Self {
        let outbox = Outbox::default();
        let signal = Signal::new().ok().map(Arc::new);
        if let Some(signal) = &signal {
            outbox.signal_with(signal.clone());
        }
        let (loader, image_dir) = TemplateDirLoader::new();
        Self {
            make: Some(make),
            fonts,
            pending: Arc::default(),
            outbox,
            signal,
            image_dir,
            loader: Some(loader),
            alive: Arc::default(),
            messenger: None,
            thread: None,
        }
    }

    /// Hands what is pending to the running app, unless an update that
    /// takes it is queued already. Before the app starts, `start` takes it.
    fn send_pending(&mut self) -> Result<(), String> {
        if self.make.is_some() {
            return Ok(());
        }
        let Some(messenger) = self.messenger.as_ref().filter(|_| self.alive.load(Ordering::Acquire)) else {
            return Err("the app isn't running".to_string());
        };
        {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            if pending.queued {
                return Ok(());
            }
            pending.queued = true;
        }
        let (pending, image_dir, outbox) = (self.pending.clone(), self.image_dir.clone(), self.outbox.clone());
        messenger
            .update(move |model, _| {
                if let Err(error) = apply_pending(&pending, model, &image_dir) {
                    outbox.push_error(error);
                }
            })
            .map_err(|e| e.to_string())
    }
}

impl<M: JsonModel + 'static> AppEngine for AppOf<M> {
    fn set_template(&mut self, template: Template, dir: PathBuf) -> Result<(), String> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner).template = Some((template, dir));
        self.send_pending()
    }

    fn set_json(&mut self, json: String) -> Result<(), String> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner).json = Some(json);
        self.send_pending()
    }

    fn start(&mut self, title: String, width: u32, height: u32) -> Result<(), String> {
        let (Some(make), Some(loader)) = (self.make.take(), self.loader.take()) else {
            return Err("the app was started already".to_string());
        };
        let fonts = self.fonts.clone();
        let (pending, image_dir, outbox, alive) = (self.pending.clone(), self.image_dir.clone(), self.outbox.clone(), self.alive.clone());
        // The app's first word: its messenger once it runs, nothing if it
        // ended at once (a snapshot), or why it couldn't start.
        let (started_tx, started) = sync_channel::<Result<Option<Messenger<M>>, String>>(1);
        let thread = std::thread::Builder::new()
            .name("xerune".to_string())
            .spawn(move || {
                let mut model = make();
                *model.outbox() = outbox.clone();
                let mut runtime = Runtime::new(model, fonts.clone());
                runtime.set_image_loader(loader);
                if let Err(error) = apply_pending(&pending, runtime.model_mut(), &image_dir) {
                    let _ = started_tx.send(Err(error));
                    return;
                }
                runtime.sync_view();
                alive.store(true, Ordering::Release);
                let _ended = Ended(alive, outbox.clone());
                let on_start = started_tx.clone();
                let result = App::new(runtime)
                    .title(title)
                    .size(width, height)
                    .on_start(move |messenger| {
                        let _ = on_start.send(Ok(Some(messenger)));
                    })
                    .run(SoftwareRenderer::new(fonts));
                // `start` waits for one word; later ones go to the outbox.
                let word = result.map(|()| None).map_err(|e| e.to_string());
                if let Err(TrySendError::Full(Err(error)) | TrySendError::Disconnected(Err(error))) = started_tx.try_send(word) {
                    outbox.push_error(error);
                }
            })
            .map_err(|e| format!("starting the app's thread: {e}"))?;
        let first = started.recv().unwrap_or_else(|_| Err("the app's thread panicked".to_string()));
        match first {
            Ok(messenger) => {
                self.messenger = messenger;
                self.thread = Some(thread);
                Ok(())
            }
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    fn running(&self) -> bool {
        self.alive.load(Ordering::Acquire)
    }

    fn outbox(&self) -> &Outbox {
        &self.outbox
    }

    fn fd(&self) -> i32 {
        self.signal.as_ref().map_or(-1, |signal| signal.fd())
    }

    fn scroll_into_view(&mut self, id: String) -> Result<(), String> {
        let Some(messenger) = self.messenger.as_ref().filter(|_| self.alive.load(Ordering::Acquire)) else {
            return Err("the app isn't running".to_string());
        };
        // After the models set before it, which are queued first.
        messenger.update(move |_, context| context.scroll_into_view(&id)).map_err(|e| e.to_string())
    }
}

/// Marks the app ended when its thread is done, even by a panic, and tells
/// the program through the outbox's signal.
struct Ended(Arc<AtomicBool>, Outbox);

impl Drop for Ended {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
        self.1.notify();
    }
}

impl<M: JsonModel> Drop for AppOf<M> {
    fn drop(&mut self) {
        if let Some(messenger) = &self.messenger {
            let _ = messenger.quit();
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn app<'a>(app: *mut XrApp) -> Result<&'a mut XrApp, String> {
    // SAFETY: the caller passes a pointer from `xr_app_new` that it hasn't
    // freed.
    unsafe { app.as_mut() }.ok_or_else(|| "the app is NULL".to_string())
}

fn c_string(s: String) -> CString {
    CString::new(s.replace('\0', "\\0")).unwrap_or_default()
}

/// Wraps a model in an app handle for C. `make` makes the model on the
/// app's thread. Crates of compiled templates export their own constructor
/// with this; every other `xr_app_` function works on what it returns.
///
/// # Safety
/// `fonts` must be NULL or point to `count` valid `XrFont`s.
pub unsafe fn new_app<M: JsonModel + 'static>(make: impl FnOnce() -> M + Send + 'static, fonts: *const XrFont, count: usize) -> *mut XrApp {
    guard(std::ptr::null_mut(), || {
        let fonts = crate::fonts(fonts, count)?;
        let app: Box<dyn AppEngine> = Box::new(AppOf::new(Box::new(make), fonts));
        Ok(Box::into_raw(Box::new(XrApp { app, message: None, error: None })))
    })
}

/// A new app drawing text with `fonts` (as `xr_new`). NULL on an error.
///
/// It shows nothing until it is started (`xr_app_start`), usually after
/// `xr_app_load_template` and `xr_app_set_model`.
///
/// # Safety
/// `fonts` must be NULL or point to `count` valid `XrFont`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_new(fonts: *const XrFont, count: usize) -> *mut XrApp {
    // SAFETY: as the caller promised.
    unsafe { new_app(Page::default, fonts, count) }
}

/// Ends the app if it runs (the display is left as it was) and frees it
/// (NULL is fine). Strings it returned are gone with it.
///
/// # Safety
/// `app` must be NULL or from `xr_app_new`, and not freed already.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_free(app: *mut XrApp) {
    if !app.is_null() {
        guard((), || {
            // SAFETY: as the caller promised.
            drop(unsafe { Box::from_raw(app) });
            Ok(())
        })
    }
}

/// Shows the template in the file at `path` (as `xr_load_template`),
/// before the app starts or while it runs.
///
/// 0 on success; -1 if the template can't be read or parsed
/// (`xr_last_error`). Whether it fits the model is found on the app's
/// thread: see `xr_app_next_error`.
///
/// # Safety
/// `app` must be from `xr_app_new`; `path` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_load_template(app: *mut XrApp, path: *const c_char) -> i32 {
    guard(-1, || {
        let app = self::app(app)?;
        let path = Path::new(str_arg(path, "the path")?);
        let content = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
        let template = engine::parse_template(&path.display().to_string(), content, &dir)?;
        app.app.set_template(template, dir)?;
        Ok(0)
    })
}

/// Like `xr_app_load_template`, with the template's text in `source`;
/// includes and images are found relative to `base_dir` (NULL: the working
/// directory).
///
/// # Safety
/// `app` must be from `xr_app_new`; `source` and `base_dir` NUL-terminated
/// strings (`base_dir` may be NULL).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_load_template_source(app: *mut XrApp, source: *const c_char, base_dir: *const c_char) -> i32 {
    guard(-1, || {
        let app = self::app(app)?;
        let source = str_arg(source, "the source")?;
        let dir = if base_dir.is_null() { PathBuf::new() } else { PathBuf::from(str_arg(base_dir, "the base directory")?) };
        let template = engine::parse_template("<source>", source.to_string(), &dir)?;
        app.app.set_template(template, dir)?;
        Ok(0)
    })
}

/// Sets the model the template shows (a JSON object, as `xr_set_model`),
/// before the app starts or while it runs. Set the whole model each time.
/// The app takes it on its own thread soon after; when the model is set
/// again before that, it shows only the latest.
///
/// 0 on success; -1 if the app has ended (`xr_last_error`). An error in
/// the model (not JSON, a field the template lacks) is found on the app's
/// thread: see `xr_app_next_error`; the app shows what it showed before.
///
/// # Safety
/// `app` must be from `xr_app_new`; `json` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_set_model(app: *mut XrApp, json: *const c_char) -> i32 {
    guard(-1, || {
        let app = self::app(app)?;
        app.app.set_json(str_arg(json, "the model")?.to_string())?;
        Ok(0)
    })
}

/// Starts the app on a thread of its own: it shows the template and model
/// set so far on the display and reads the input devices. The display is
/// picked when it starts: a window of `width` x `height` titled `title`
/// (NULL: "xerune") in a desktop session, else DRM/KMS at the display's
/// size, else the fbdev framebuffer; the environment variable
/// `XERUNE_BACKEND=winit|drm|fbdev` picks one. Only the displays the
/// library was built with are there (Cargo features `drm`, `linuxfb`,
/// `winit`). With `XERUNE_SNAPSHOT=out.png` it renders a screenshot
/// instead and ends.
///
/// An app starts once. 0 on success; -1 if it can't start (no display,
/// no template, a model that doesn't fit it: `xr_last_error`).
///
/// # Safety
/// `app` must be from `xr_app_new`; `title` NULL or a NUL-terminated
/// string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_start(app: *mut XrApp, title: *const c_char, width: u32, height: u32) -> i32 {
    guard(-1, || {
        let app = self::app(app)?;
        let title = if title.is_null() { "xerune" } else { str_arg(title, "the title")? };
        app.app.start(title.to_string(), width, height)?;
        Ok(0)
    })
}

/// Scrolls the element with the `id` attribute into view (as
/// `xr_scroll_into_view`), on the app's thread, after the models set
/// before: set the model that selects a row, then scroll to it.
///
/// 0 on success; -1 if the app isn't running (`xr_last_error`).
///
/// # Safety
/// `app` must be from `xr_app_new`; `id` a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_scroll_into_view(app: *mut XrApp, id: *const c_char) -> i32 {
    guard(-1, || {
        let app = self::app(app)?;
        app.app.scroll_into_view(str_arg(id, "the id")?.to_string())?;
        Ok(0)
    })
}

/// Whether the app runs: false before `xr_app_start` and once it ended (its
/// window was closed, or its display failed: `xr_app_next_error`).
///
/// # Safety
/// `app` must be from `xr_app_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_running(app: *mut XrApp) -> bool {
    guard(false, || Ok(self::app(app)?.app.running()))
}

/// A file descriptor that is readable while messages or errors wait
/// (`xr_app_next_message`, `xr_app_next_error`), and once when the app
/// ends: for `poll`, `epoll` or any event loop that watches descriptors.
/// Taking everything that waits makes it unreadable again. The app owns
/// it: don't read or close it. -1 where there is none (only Linux has
/// one).
///
/// # Safety
/// `app` must be from `xr_app_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_message_fd(app: *mut XrApp) -> i32 {
    guard(-1, || Ok(self::app(app)?.app.fd()))
}

/// The next message the UI sent (as `xr_next_message`), oldest first, or
/// NULL when there is none. Handle each by setting the model
/// (`xr_app_set_model`).
///
/// The string is valid until the next call of `xr_app_next_message` or
/// `xr_app_free`.
///
/// # Safety
/// `app` must be from `xr_app_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_next_message(app: *mut XrApp) -> *const c_char {
    guard(std::ptr::null(), || {
        let app = self::app(app)?;
        app.message = app.app.outbox().pop().map(c_string);
        Ok(app.message.as_ref().map_or(std::ptr::null(), |m| m.as_ptr()))
    })
}

/// The next error found on the app's thread, oldest first, or NULL when
/// there is none: a model or template that doesn't fit (the app shows what
/// it showed before), or why the app ended.
///
/// The string is valid until the next call of `xr_app_next_error` or
/// `xr_app_free`.
///
/// # Safety
/// `app` must be from `xr_app_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn xr_app_next_error(app: *mut XrApp) -> *const c_char {
    guard(std::ptr::null(), || {
        let app = self::app(app)?;
        app.error = app.app.outbox().pop_error().map(c_string);
        Ok(app.error.as_ref().map_or(std::ptr::null(), |e| e.as_ptr()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn template(source: &str) -> (Template, PathBuf) {
        (engine::parse_template("t", source.to_string(), Path::new("")).unwrap(), PathBuf::from("ui"))
    }

    #[test]
    fn a_template_and_its_model_change_together() {
        let (pending, image_dir) = (Mutex::new(Pending::default()), RwLock::new(PathBuf::new()));
        let mut page = Page::default();
        page.set_json(r#"{"a": 1}"#).unwrap();
        page.set_template(template("<p>{{ a }}</p>").0, Path::new("")).unwrap();

        // Neither fits what the page has: `b` isn't in the model, the new
        // model lacks `a`.
        *pending.lock().unwrap() = Pending { template: Some(template("<p>{{ b }}</p>")), json: Some(r#"{"b": 2}"#.into()), queued: true };
        apply_pending(&pending, &mut page, &image_dir).unwrap();
        assert_eq!(*image_dir.read().unwrap(), PathBuf::from("ui"));
        assert!(!pending.lock().unwrap().queued);

        *pending.lock().unwrap() = Pending { template: None, json: Some(r#"{"c": 3}"#.into()), queued: true };
        assert!(apply_pending(&pending, &mut page, &image_dir).unwrap_err().contains('b'));
    }
}
