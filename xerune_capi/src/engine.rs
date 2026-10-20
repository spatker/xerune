//! What an `XrUi` handle holds: a xerune [`Runtime`] for a model that the
//! caller sets as JSON, and its renderer. [`Engine`] hides the model's
//! type, so a crate of compiled templates can hand out the same handles as
//! the interpreter ([`Page`]). The models are those of `XrApp`s too
//! (`crate::app`).

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::RcDom;
use xerune::images::{DefaultImageLoader, ImageError, ImageLoader, is_remote};
use xerune::style::ContainerStyle;
use xerune::ui::{TemplateLayout, UiBuilder};
use xerune::{Canvas, Context, Damage, InputEvent, Model, Runtime, Surface, TextMeasurer};
use xerune_render::{Fonts, SoftwareRenderer};
use xerune_template::{File, Template, Value};

/// A model the C API drives: its data comes as JSON, and the messages of
/// its view queue up for the caller to take.
pub trait JsonModel: Model + TemplateLayout {
    /// Takes the model's data from `json`. On an error the model is as it
    /// was.
    fn set_json(&mut self, json: &str) -> Result<(), String>;

    /// Where `update` puts the messages of the view for the caller. An app
    /// replaces it with one its caller's thread reads.
    fn outbox(&mut self) -> &mut Outbox;

    /// Shows `template` from now on, its images found relative to
    /// `base_dir`. Only an interpreted model can.
    fn set_template(&mut self, _template: Template, _base_dir: &Path) -> Result<(), String> {
        Err("this UI's templates are compiled into it".to_string())
    }

    /// Shows `template` for the model in `json`, both new: each is checked
    /// against the other, not against what the model had. On an error the
    /// model is as it was.
    fn set_template_and_json(&mut self, template: Template, base_dir: &Path, json: &str) -> Result<(), String> {
        self.set_template(template, base_dir)?;
        self.set_json(json)
    }
}

/// Messages of a view, and errors, waiting for the caller. Clones share
/// the queue: an app's model fills it on the app's thread while the caller
/// takes from it on its own.
#[derive(Clone, Default)]
pub struct Outbox(Arc<Mutex<Mail>>);

#[derive(Default)]
struct Mail {
    messages: VecDeque<String>,
    errors: VecDeque<String>,
    /// Readable while anything waits, or the app ended unseen.
    signal: Option<Arc<Signal>>,
    raised: bool,
}

impl Outbox {
    /// Queues a message of the view.
    pub fn push(&self, message: String) {
        self.with(|mail| mail.messages.push_back(message));
    }

    /// The oldest message the caller hasn't taken.
    pub fn pop(&self) -> Option<String> {
        self.with(|mail| mail.messages.pop_front())
    }

    pub(crate) fn push_error(&self, error: String) {
        self.with(|mail| mail.errors.push_back(error));
    }

    pub(crate) fn pop_error(&self) -> Option<String> {
        self.with(|mail| mail.errors.pop_front())
    }

    /// Raises `signal` while anything waits, from now on.
    pub(crate) fn signal_with(&self, signal: Arc<Signal>) {
        self.with(|mail| mail.signal = Some(signal));
    }

    /// Raises the signal although nothing waits (the app ended); the next
    /// `pop` lowers it.
    pub(crate) fn notify(&self) {
        let mut mail = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(signal) = &mail.signal {
            signal.raise();
            mail.raised = true;
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut Mail) -> T) -> T {
        let mut mail = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let result = f(&mut mail);
        let waiting = !mail.messages.is_empty() || !mail.errors.is_empty();
        if let Some(signal) = &mail.signal {
            if waiting != mail.raised {
                if waiting { signal.raise() } else { signal.lower() }
                mail.raised = waiting;
            }
        }
        result
    }
}

/// A file descriptor that is readable while it is raised, for the caller's
/// `poll` or event loop: an eventfd.
#[cfg(target_os = "linux")]
pub(crate) struct Signal(std::os::fd::OwnedFd);

#[cfg(target_os = "linux")]
impl Signal {
    pub(crate) fn new() -> Result<Self, String> {
        use std::os::fd::FromRawFd;
        // SAFETY: eventfd takes no pointers; a valid fd is ours to own.
        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        if fd < 0 {
            return Err(format!("eventfd: {}", std::io::Error::last_os_error()));
        }
        // SAFETY: `fd` is a new descriptor nothing else owns.
        Ok(Self(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) }))
    }

    pub(crate) fn fd(&self) -> i32 {
        use std::os::fd::AsRawFd;
        self.0.as_raw_fd()
    }

    fn raise(&self) {
        use std::os::fd::AsRawFd;
        let one = 1u64;
        // SAFETY: writes the 8 bytes of `one` to our eventfd. It can't
        // overflow: raises alternate with lowers.
        unsafe { libc::write(self.0.as_raw_fd(), (&one as *const u64).cast(), 8) };
    }

    fn lower(&self) {
        use std::os::fd::AsRawFd;
        let mut count = 0u64;
        // SAFETY: reads 8 bytes into `count`; resets the counter, or fails
        // with EAGAIN when it is 0 already.
        unsafe { libc::read(self.0.as_raw_fd(), (&mut count as *mut u64).cast(), 8) };
    }
}

/// No signal outside Linux: `xr_app_message_fd` is -1 there.
#[cfg(not(target_os = "linux"))]
pub(crate) enum Signal {}

#[cfg(not(target_os = "linux"))]
impl Signal {
    pub(crate) fn new() -> Result<Self, String> {
        Err("there is no message fd on this system".to_string())
    }

    pub(crate) fn fd(&self) -> i32 {
        match *self {}
    }

    fn raise(&self) {
        match *self {}
    }

    fn lower(&self) {
        match *self {}
    }
}

/// An `XrUi`'s runtime, whatever its model.
pub trait Engine {
    fn set_size(&mut self, width: u32, height: u32);
    fn size(&self) -> (u32, u32);
    fn set_template(&mut self, template: Template, base_dir: &Path) -> Result<(), String>;
    fn set_json(&mut self, json: &str) -> Result<(), String>;
    fn handle(&mut self, event: InputEvent) -> bool;
    fn tick(&mut self, now_ms: u64) -> bool;
    fn render(&mut self, target: &mut Surface<'_>) -> Damage;
    fn repaint_all(&mut self);
    fn next_message(&mut self) -> Option<String>;
    fn wait_for_images(&mut self, timeout: Duration) -> bool;
    /// Whether (`x`, `y`) is on an element the view shows or that takes
    /// input, rather than on the bare page.
    fn hit(&self, x: f32, y: f32) -> bool;
    /// The `<canvas id>` of the view, marked changed: the caller draws
    /// into it.
    fn canvas(&mut self, id: &str) -> Option<&mut Canvas>;
}

/// The caller's clock: [`Engine::tick`] sets the time, in milliseconds
/// from its first tick.
#[derive(Clone)]
struct CallerClock {
    start: Instant,
    elapsed_ms: Arc<AtomicU64>,
}

impl xerune::runtime::Clock for CallerClock {
    fn now(&self) -> Instant {
        self.start + Duration::from_millis(self.elapsed_ms.load(Ordering::Relaxed))
    }
}

/// Finds images relative to the template's directory, as a browser finds
/// them relative to the page; URLs and absolute paths as they are.
pub(crate) struct TemplateDirLoader {
    dir: Arc<RwLock<PathBuf>>,
    default: DefaultImageLoader,
}

impl TemplateDirLoader {
    /// A loader, and the directory it reads, for setting when the template
    /// changes.
    pub(crate) fn new() -> (Self, Arc<RwLock<PathBuf>>) {
        let dir = Arc::new(RwLock::new(PathBuf::new()));
        (Self { dir: dir.clone(), default: DefaultImageLoader::default() }, dir)
    }
}

impl ImageLoader for TemplateDirLoader {
    fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
        if is_remote(src) || src.starts_with("file://") || Path::new(src).is_absolute() {
            return self.default.fetch(src);
        }
        let dir = self.dir.read().map(|dir| dir.clone()).unwrap_or_default();
        Ok(std::fs::read(dir.join(src))?)
    }
}

pub struct Ui<M: JsonModel> {
    runtime: Runtime<M, Fonts>,
    renderer: SoftwareRenderer,
    size: (u32, u32),
    clock: CallerClock,
    /// The time of the first tick, which is the clock's start.
    first_tick: Option<u64>,
    image_dir: Arc<RwLock<PathBuf>>,
}

impl<M: JsonModel> Ui<M> {
    pub fn new(model: M, fonts: Fonts) -> Self {
        let mut runtime = Runtime::new(model, fonts.clone());
        let clock = CallerClock { start: Instant::now(), elapsed_ms: Arc::default() };
        runtime.set_clock(clock.clone());
        let (loader, image_dir) = TemplateDirLoader::new();
        runtime.set_image_loader(loader);
        Self { runtime, renderer: SoftwareRenderer::new(fonts), size: (0, 0), clock, first_tick: None, image_dir }
    }

    /// FIXME: xerune keeps the viewport in globals (`xerune::screen`), read
    /// while styling for `@media` and `vw`/`vh`. Each call sets this UI's
    /// size first so that several UIs of different sizes style right, as
    /// long as they are used from one thread at a time.
    fn enter(&self) {
        if self.size != (0, 0) {
            xerune::screen::set_viewport(self.size.0 as f32, self.size.1 as f32);
        }
    }
}

impl<M: JsonModel> Engine for Ui<M> {
    fn set_size(&mut self, width: u32, height: u32) {
        self.size = (width, height);
        self.runtime.set_size(width as f32, height as f32);
    }

    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn set_template(&mut self, template: Template, base_dir: &Path) -> Result<(), String> {
        self.enter();
        self.runtime.model_mut().set_template(template, base_dir)?;
        if let Ok(mut dir) = self.image_dir.write() {
            *dir = base_dir.to_path_buf();
        }
        self.runtime.sync_view();
        Ok(())
    }

    fn set_json(&mut self, json: &str) -> Result<(), String> {
        self.enter();
        self.runtime.model_mut().set_json(json)?;
        self.runtime.sync_view();
        Ok(())
    }

    fn handle(&mut self, event: InputEvent) -> bool {
        self.enter();
        self.runtime.handle_event(event)
    }

    fn tick(&mut self, now_ms: u64) -> bool {
        self.enter();
        let first = *self.first_tick.get_or_insert(now_ms);
        self.clock.elapsed_ms.fetch_max(now_ms.saturating_sub(first), Ordering::Relaxed);
        self.runtime.poll().redraw
    }

    fn render(&mut self, target: &mut Surface<'_>) -> Damage {
        self.enter();
        self.runtime.render(&mut self.renderer, target)
    }

    fn repaint_all(&mut self) {
        self.runtime.repaint_all();
    }

    fn next_message(&mut self) -> Option<String> {
        self.runtime.model_mut().outbox().pop()
    }

    fn wait_for_images(&mut self, timeout: Duration) -> bool {
        self.enter();
        self.runtime.wait_for_images(timeout)
    }

    fn hit(&self, x: f32, y: f32) -> bool {
        let ui = &self.runtime.ui;
        match ui.hit_test(x, y) {
            Some((Some(_), _)) => true,
            Some((None, node)) => node != ui.root && Some(node) != ui.body,
            None => false,
        }
    }

    fn canvas(&mut self, id: &str) -> Option<&mut Canvas> {
        let canvas = self.runtime.context_mut().canvas_mut(id)?;
        canvas.dirty = true;
        Some(canvas)
    }
}

/// An interpreted template and the JSON model it shows: the view is the
/// template's HTML for the model, built like any HTML page.
pub struct Page {
    template: Option<Template>,
    /// The template's CSS. The runtime keeps stylesheets for the program's
    /// life (`TemplateLayout::stylesheet` is `'static`), so each template
    /// load leaks one.
    css: &'static str,
    /// The model, once the caller set one; until then the view is empty.
    model: Option<Value>,
    dom: RcDom,
    /// Changes with every new view, for [`Model::view_fingerprint`].
    generation: u64,
    outbox: Outbox,
}

impl Default for Page {
    fn default() -> Self {
        Self { template: None, css: "", model: None, dom: RcDom::default(), generation: 0, outbox: Outbox::default() }
    }
}

impl Page {
    fn parse_model(json: &str) -> Result<Value, String> {
        let model: Value = serde_json::from_str(json).map_err(|e| format!("the model isn't JSON: {e}"))?;
        if !matches!(model, Value::Struct(_)) {
            return Err(format!("the model must be a JSON object, not a {}", model.kind()));
        }
        Ok(model)
    }

    /// Takes `template`, whose view is made.
    fn show(&mut self, template: Template) {
        self.css = Box::leak(template.css.clone().into_boxed_str());
        self.template = Some(template);
        self.generation += 1;
    }

    /// Makes the view of `template` for `model`, or says why it can't.
    fn view(template: &Template, model: &Value) -> Result<RcDom, String> {
        let html = xerune_template::render_html(template, model, &()).map_err(|e| e.to_string())?;
        Ok(html5ever::parse_document(RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()).map_err(|e| e.to_string())?)
    }
}

impl Model for Page {
    type Message = String;

    fn update(&mut self, msg: String, _context: &mut Context<String>) {
        self.outbox.push(msg);
    }

    /// The view changes only when the caller sets the model or the
    /// template: the messages the view sends go to the caller.
    fn view_fingerprint(&self) -> Option<u64> {
        Some(self.generation)
    }
}

impl TemplateLayout for Page {
    fn stylesheet(&self) -> &'static str {
        self.css
    }

    fn build_ui(&self, builder: &mut UiBuilder, _: &impl TextMeasurer, _: &ContainerStyle, _: &impl Fn(&str) -> bool) -> taffy::NodeId {
        xerune::ui::html::build_document(&self.dom.document, builder).unwrap_or_else(|| builder.create_element("html", &[]))
    }
}

impl JsonModel for Page {
    fn set_json(&mut self, json: &str) -> Result<(), String> {
        let model = Self::parse_model(json)?;
        if let Some(template) = &self.template {
            self.dom = Self::view(template, &model)?;
            self.generation += 1;
        }
        self.model = Some(model);
        Ok(())
    }

    fn outbox(&mut self) -> &mut Outbox {
        &mut self.outbox
    }

    fn set_template(&mut self, template: Template, _base_dir: &Path) -> Result<(), String> {
        // A model set before the template is checked against it now.
        if let Some(model) = &self.model {
            self.dom = Self::view(&template, model)?;
        }
        self.show(template);
        Ok(())
    }

    fn set_template_and_json(&mut self, template: Template, _base_dir: &Path, json: &str) -> Result<(), String> {
        let model = Self::parse_model(json)?;
        self.dom = Self::view(&template, &model)?;
        self.model = Some(model);
        self.show(template);
        Ok(())
    }
}

/// Parses the template in `content`; `{% include %}`s are files relative
/// to `dir`.
pub fn parse_template(name: &str, content: String, dir: &Path) -> Result<Template, String> {
    let file = File { name: name.to_string(), dir: dir.to_path_buf(), content };
    xerune_template::parse(file, &mut |include: &str, dir: &Path| {
        let path = dir.join(include);
        let content = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(File { name: include.to_string(), dir: path.parent().unwrap_or(dir).to_path_buf(), content })
    })
    .map_err(|e| e.to_string())
}
