/// Timer abstractions and callback ticking management.
pub mod timer;
/// Transition and keyframe animation tracking state.
pub mod animation;
/// Platform-independent monotonic time wrappers.
pub mod time;
mod diff;
mod stats;
mod transition;

pub use stats::{FrameStats, PhaseTime};
pub use timer::{Poll, Subscriptions, TimerId, Wake};
pub use time::{Clock, SystemClock};
#[cfg(feature = "std")]
pub use time::ManualClock;
pub use animation::ActiveAnimation;

use core::str::FromStr;
use taffy::prelude::*;
use crate::alloc_prelude::*;
use crate::runtime::time::Instant;
use crate::runtime::timer::{Interval, Timer};
use crate::runtime::stats::Phase;
use core::time::Duration;

use crate::graphics::{Context, Damage, DrawCommand, Rect, TextMeasurer, Renderer, RenderResources, Surface};
use crate::images::ImageStore;
use crate::style::{ContainerStyle, RenderKind};
use crate::model::{InputEvent, Model};
use crate::ui::{Interaction, Ui, NodeMap};
use crate::keyboard::{Keyboard, KeyboardMsg, OnText, TextRequest};

/// Placeholder box used for images without an explicit size until they have loaded.
const IMAGE_PLACEHOLDER_SIZE: f32 = 100.0;

/// Current active state of a touch event/gesture tracking.
#[derive(Debug, Clone, Copy)]
pub struct TouchState {
    /// Associated pointer/touch input identifier.
    pub id: u64,
    /// Starting X coordinate of the touch stroke.
    pub start_x: f32,
    /// Starting Y coordinate of the touch stroke.
    pub start_y: f32,
    /// Last recorded/current X coordinate.
    pub last_x: f32,
    /// Last recorded/current Y coordinate.
    pub last_y: f32,
    /// Flags if this touch interaction has evolved into scroll offsets.
    pub has_scrolled: bool,
    /// Target scroll container NodeId resolved for this touch stroke.
    pub scroll_node: Option<NodeId>,
}

/// A range input being dragged: its element's key, and its value when
/// the drag started and now.
#[derive(Debug, Clone, Copy)]
struct RangeDrag {
    key: u64,
    start: f32,
    value: f32,
}

/// The on-screen keyboard while it is open: its state, how to tell the
/// model the result, and the model's scroll positions to go back to.
struct KeyboardSession<Msg> {
    keyboard: Keyboard,
    on_done: OnText<Msg>,
    scroll_offsets: NodeMap<(f32, f32)>,
}

/// The Elm/MVU runtime engine coordinating state updates, layout, input events, and rendering.
pub struct Runtime<M: Model, R> {
    model: M,
    measurer: R,
    /// The current resolved UI structure mapping. Internal.
    #[doc(hidden)]
    pub ui: Ui,
    default_style: ContainerStyle,
    pub(crate) scroll_offsets: NodeMap<(f32, f32)>,
    cached_size: Size<AvailableSpace>,
    viewport_px: Option<(f32, f32)>,
    last_fingerprint: Option<u64>,
    context: Context<M::Message>,
    last_commands: Vec<DrawCommand>,
    /// The next frame must be drawn completely: nothing is drawn yet, the
    /// viewport changed, or images arrived whose commands look unchanged.
    full_repaint: bool,
    /// Focused input ID if any text inputs are currently active.
    pub focused_id: Option<String>,
    /// Where all times come from.
    clock: Box<dyn Clock>,
    /// Frames per second while something animates, as set by
    /// [`Self::set_target_fps`]; `XERUNE_TARGET_FPS` overrides it.
    target_fps: u32,
    target_fps_override: Option<u32>,
    /// Timers started with `Context::after`, not yet fired.
    timers: Vec<Timer<M::Message>>,
    /// What the model last asked for in `Model::subscriptions`, and when
    /// each of its `every` is due next.
    subscriptions: Subscriptions<M::Message>,
    intervals: Vec<Interval>,
    /// While frames run: when the previous one was and the next is due.
    frame_clock: Option<(Instant, Instant)>,
    /// Running keyframe animations, by element key.
    pub(crate) active_animations: HashMap<u64, ActiveAnimation>,
    /// Running transitions, by element key and property.
    transitions: transition::TransitionStore,
    /// Messages of `data-on-transition*` events, for the next poll.
    transition_messages: Vec<Interaction>,
    pub(crate) touch_state: Option<TouchState>,
    /// The range input the pointer pressed, while it is held.
    range_drag: Option<RangeDrag>,
    images: ImageStore,
    /// Where the pointer was last seen, and whether it is pressed.
    pointer: Option<(f32, f32)>,
    pressed: bool,
    /// The elements under the pointer the current view was built with.
    pointer_state: crate::ui::PointerState,
    /// Counts views built, to tell whether a message rebuilt it.
    builds: u32,
    /// What keys do in the runtime's own UI.
    key_map: crate::keys::KeyMap,
    /// The on-screen keyboard, shown instead of the model's view.
    keyboard: Option<KeyboardSession<M::Message>>,
    /// Tapping a text input opens the keyboard for it, as set by
    /// [`Self::set_input_keyboard`] or `XERUNE_INPUT_KEYBOARD=1`.
    input_keyboard: bool,
    input_keyboard_override: bool,
    /// The next view replaces another tree (the keyboard's or the
    /// model's): none of its elements were there before.
    new_tree: bool,
    /// Phase times, while the model's stats subscription or
    /// `XERUNE_PROFILE=1` wants them.
    stats: stats::Profiler,
}

impl<M: Model + crate::ui::TemplateLayout, R: TextMeasurer> Runtime<M, R> {
    /// Create a new UI MVU runtime with the specified initial Model and a TextMeasurer.
    pub fn new(model: M, measurer: R) -> Self {
         let default_style = ContainerStyle::default();
         let validator = |s: &str| M::Message::from_str(s).is_ok();
         let ui = Ui::new_compiled(&model, &measurer, default_style.clone(), &validator).unwrap();
         let mut ui = ui;
         let images = ImageStore::new();
         // Images are requested on the first layout (`set_size` / `sync_view`), so a custom
         // loader can still be installed right after construction.
         Runtime::<M, R>::apply_image_sizes(&mut ui, &images);
         
         let mut context = Context::new();
         Runtime::<M, R>::sync_canvases(&ui, &mut context);
         let initial_fingerprint = model.view_fingerprint();
         #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
         let target_fps_override = std::env::var("XERUNE_TARGET_FPS").ok().and_then(|v| v.trim().parse().ok()).filter(|&fps: &u32| fps > 0);
         #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
         let target_fps_override = None;
         #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
         let input_keyboard_override = std::env::var("XERUNE_INPUT_KEYBOARD").is_ok_and(|v| v.trim() == "1");
         #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
         let input_keyboard_override = false;
         #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
         let profile = std::env::var("XERUNE_PROFILE").is_ok_and(|v| v.trim() == "1");
         #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
         let profile = false;

         let mut runtime = Self {
             model,
             measurer,
             ui,
             default_style,
             scroll_offsets: NodeMap::new(),
             cached_size: Size::MAX_CONTENT,
             viewport_px: None,
             last_fingerprint: initial_fingerprint,
             context,
             last_commands: Vec::new(),
             full_repaint: true,
             focused_id: None,
             clock: Box::new(SystemClock),
             target_fps: 60,
             target_fps_override,
             timers: Vec::new(),
             subscriptions: Subscriptions::none(),
             intervals: Vec::new(),
             frame_clock: None,
             active_animations: HashMap::new(),
             transitions: HashMap::new(),
             transition_messages: Vec::new(),
             touch_state: None,
             range_drag: None,
             images,
             pointer: None,
             pressed: false,
             pointer_state: crate::ui::PointerState::default(),
             builds: 0,
             key_map: crate::keys::KeyMap::default(),
             keyboard: None,
             input_keyboard: false,
             input_keyboard_override,
             new_tree: true,
             stats: stats::Profiler::new(profile.then(|| SystemClock.now())),
         };
         runtime.update_subscriptions();
         runtime
    }

    /// Reads the time from `clock` from now on, e.g. a `no_std` board's
    /// millisecond counter (`|| Instant::from_millis(millis())`) or a
    /// [`ManualClock`] in tests. Meant to be set before the app runs:
    /// subscriptions start over from the new clock's current time, and
    /// timers and animations carry on from it.
    pub fn set_clock(&mut self, clock: impl Clock) {
        let (old, new) = (self.now(), clock.now());
        self.clock = Box::new(clock);
        for timer in &mut self.timers {
            timer.deadline = new + timer.deadline.saturating_duration_since(old);
        }
        self.intervals.clear();
        self.stats.restart(new);
        self.update_subscriptions();
        for active in self.active_animations.values_mut() {
            active.updated = new;
        }
        self.frame_clock = None;
    }

    /// The current time on the runtime's clock.
    pub fn now(&self) -> Instant {
        self.clock.now()
    }

    /// How often frames come while something animates (CSS animations and
    /// transitions, a model's frame subscription), like raylib's
    /// `SetTargetFPS`. Lower it to leave the CPU to more important work;
    /// animations take the same time in fewer steps. 60 by default; the
    /// `XERUNE_TARGET_FPS` environment variable overrides it, to tune a
    /// device without rebuilding.
    pub fn set_target_fps(&mut self, fps: u32) {
        self.target_fps = fps.max(1);
    }

    /// Which keys move, press and cancel in the runtime's own UI (the
    /// on-screen keyboard). [`crate::KeyMap::default`] knows the built-in
    /// backends' names; a device with other buttons binds its own.
    pub fn set_key_map(&mut self, key_map: crate::keys::KeyMap) {
        self.key_map = key_map;
    }

    /// Makes tapping a text input (`<input type="text" id="..">`) open the
    /// on-screen keyboard with its value, for devices without a keyboard.
    /// On OK the model gets `<id>:value:<text>`, the input's new value;
    /// Cancel sends nothing. Off by default, where inputs take typed text
    /// (`<id>:text:<typed>`); `XERUNE_INPUT_KEYBOARD=1` turns it on.
    pub fn set_input_keyboard(&mut self, on: bool) {
        self.input_keyboard = on;
    }

    /// The frame rate in effect (see [`Self::set_target_fps`]).
    pub fn target_fps(&self) -> u32 {
        self.target_fps_override.unwrap_or(self.target_fps)
    }

    fn frame_interval(&self) -> Duration {
        Duration::from_nanos(1_000_000_000 / self.target_fps() as u64)
    }

    /// Get a reference to the inner model.
    pub fn model(&self) -> &M {
        &self.model
    }

    /// Get a mutable reference to the inner model.
    pub fn model_mut(&mut self) -> &mut M {
        &mut self.model
    }

    /// Get a reference to the inner context.
    pub fn context(&self) -> &Context<M::Message> {
        &self.context
    }

    /// Get a mutable reference to the inner context.
    pub fn context_mut(&mut self) -> &mut Context<M::Message> {
        &mut self.context
    }

    /// Images referenced by the current view and their load state.
    pub fn images(&self) -> &ImageStore {
        &self.images
    }

    /// Mutable access to the image store, e.g. to [`ImageStore::insert`] bundled images.
    pub fn images_mut(&mut self) -> &mut ImageStore {
        &mut self.images
    }

    /// Replace the loader used to fetch and decode `<img src>` (default: [`crate::DefaultImageLoader`]).
    /// Install it before the first layout so initial images use it too.
    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    pub fn set_image_loader(&mut self, loader: impl crate::images::ImageLoader) {
        self.images.set_loader(loader);
    }

    /// Install a callback that wakes the event loop when a background image load completes.
    /// Built-in backends install this automatically; the next [`Self::poll`] applies the result.
    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    pub fn set_image_waker(&mut self, waker: impl Fn() + Send + Sync + 'static) {
        self.images.set_waker(Arc::new(waker));
    }

    /// Block until pending images have loaded (or failed), up to `timeout`, then relayout.
    /// Intended for headless rendering and tests. Returns true if a redraw is needed.
    #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
    pub fn wait_for_images(&mut self, timeout: core::time::Duration) -> bool {
        if self.images.wait(timeout) {
            self.full_repaint = true;
            self.sync_view_inner(false)
        } else {
            false
        }
    }

    /// Sizes `<img>` nodes: once loaded, an image without explicit width/height takes its
    /// natural size, and its aspect ratio is used when only one dimension is set. Until then,
    /// unsized images get a placeholder box.
    fn apply_image_sizes(ui: &mut Ui, images: &ImageStore) {
        let nodes: Vec<(NodeId, Option<(u32, u32)>)> = ui
            .render_data
            .iter()
            .filter_map(|(node, data)| match &data.kind {
                RenderKind::Image(src) => Some((node, images.get(src).map(|i| (i.width, i.height)))),
                _ => None,
            })
            .collect();

        for (node, natural) in nodes {
            let Ok(mut style) = ui.taffy.style(node).cloned() else { continue };
            let auto_w = style.size.width.is_auto();
            let auto_h = style.size.height.is_auto();
            match natural {
                Some((w, h)) => {
                    if style.aspect_ratio.is_none() {
                        style.aspect_ratio = Some(w as f32 / h as f32);
                    }
                    if auto_w && auto_h {
                        style.size.width = length(w as f32);
                    }
                }
                None => {
                    if auto_w && auto_h {
                        style.size = Size {
                            width: length(IMAGE_PLACEHOLDER_SIZE),
                            height: length(IMAGE_PLACEHOLDER_SIZE),
                        };
                    } else if (auto_w != auto_h) && style.aspect_ratio.is_none() {
                        style.aspect_ratio = Some(1.0);
                    }
                }
            }
            let _ = ui.taffy.set_style(node, style.clone());
            // Keep the animation base in sync so restoring an animated node keeps the size.
            if let Some((base_layout, _)) = ui.base_styles.get_mut(node) {
                *base_layout = style;
            }
        }
    }

    fn sync_canvases(ui: &Ui, context: &mut Context<M::Message>) {
        for (node_id, data) in &ui.render_data {
            if let RenderKind::Canvas(id) = &data.kind {
                if !context.canvases.contains_key(id) {
                     let mut width = 200;
                     let mut height = 200;

                     if let Ok(style) = ui.taffy.style(node_id) {
                         let w_dim = style.size.width;
                         if !w_dim.is_auto() {
                             let val = w_dim.value();
                             if w_dim == Dimension::length(val) {
                                  width = val as u32;
                              }
                         }

                         let h_dim = style.size.height;
                         if !h_dim.is_auto() {
                             let val = h_dim.value();
                             if h_dim == Dimension::length(val) {
                                  height = val as u32;
                             }
                         }
                     }

                     context.canvases.insert(id.clone(), crate::graphics::Canvas::new(width, height));
                }
            }
        }
    }

    fn restore_scroll(&mut self) {
        self.ui.scroll_offsets = self.scroll_offsets.clone();
    }

    /// Dispatches an input event to the UI and updates the application model accordingly.
    /// Returns true if the screen needs to be redrawn.
    pub fn handle_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::Hover { x, y } => self.move_pointer(x, y, self.pressed),
            InputEvent::Click { x, y } => {
                // The view is rebuilt below if the message changes it.
                self.pointer = Some((x, y));
                let restyled = self.update_pointer_state();
                if let Some((interaction, clicked_node)) = self.ui.hit_test(x, y) {
                    if let Some(RenderKind::TextInput(id, value, placeholder)) = self.ui.render_data.get(&clicked_node).map(|d| &d.kind) {
                        if (self.input_keyboard || self.input_keyboard_override) && !id.is_empty() {
                            let title = placeholder.as_ref().map_or("", |p| p.0.as_str());
                            let mut request = TextRequest::new(title).text(value.as_deref().unwrap_or(""));
                            if let Some(lang) = self.ui.lang(clicked_node) {
                                request = request.lang(lang);
                            }
                            let id = id.clone();
                            self.open_keyboard(request, Box::new(move |text: Option<String>| {
                                let message = format!("{id}:value:{}", text?);
                                let parsed = M::Message::from_str(&message).ok();
                                if parsed.is_none() {
                                    log::debug!("the model takes no {id}:value: message for the keyboard's text");
                                }
                                parsed
                            }));
                            return self.sync_view_inner(false);
                        }
                        if !id.is_empty() {
                            self.focused_id = Some(id.clone());
                        } else {
                            self.focused_id = None;
                        }
                    } else {
                        self.focused_id = None;
                    }
                    
                    let updated = interaction.is_some_and(|interaction| self.dispatch(interaction));
                    if restyled && !updated {
                        self.rebuild();
                    }
                    return updated || restyled || self.focused_id.is_some();
                }

                if restyled {
                    self.rebuild();
                }
                let old_focus = self.focused_id.take();
                return old_focus.is_some() || restyled;
            }
            InputEvent::Message(msg_str) => self.process_message_str(&msg_str),
            InputEvent::Scroll { x, y, delta_x, delta_y } => {
                if self.ui.handle_scroll(x, y, delta_x, delta_y) {
                    self.scroll_offsets = self.ui.scroll_offsets.clone();
                    return true;
                }
                false
            }
            InputEvent::KeyDown(key) if self.keyboard.is_some() => {
                match self.key_map.action(&key) {
                    Some(action) => self.update_keyboard(|keyboard| keyboard.key(action)),
                    None => false,
                }
            }
            InputEvent::KeyUp(_) if self.keyboard.is_some() => false,
            InputEvent::KeyDown(key) => {
                let msg_str = format!("keydown:{}", key);
                self.process_message_str(&msg_str)
            }
            InputEvent::KeyUp(key) => {
                let msg_str = format!("keyup:{}", key);
                self.process_message_str(&msg_str)
            }
            InputEvent::TextInput { text, .. } if self.keyboard.is_some() => {
                self.update_keyboard(|keyboard| {
                    keyboard.type_text(&text);
                    None
                })
            }
            InputEvent::TextInput { id: event_id, text } => {
                if let Some(ref focused) = self.focused_id {
                    if event_id.is_empty() || &event_id == focused {
                        let msg_str = format!("{}:text:{}", focused, text);
                        return self.process_message_str(&msg_str);
                    }
                }
                false
            }
            InputEvent::TouchStart { id, x, y } => {
                self.touch_state = Some(TouchState {
                    id,
                    start_x: x,
                    start_y: y,
                    last_x: x,
                    last_y: y,
                    has_scrolled: false,
                    scroll_node: None,
                });
                let redraw = self.move_pointer(x, y, true);
                self.start_range_drag(x, y) || redraw
            }
            InputEvent::TouchMove { id, x, y } if self.range_drag.is_some() && self.touch_state.is_some_and(|t| t.id == id) => {
                // A dragged range takes the touch: it doesn't scroll.
                let redraw = self.move_pointer(x, y, self.pressed);
                self.move_range_drag(x) || redraw
            }
            InputEvent::TouchMove { id, x, y } => {
                let mut redraw = false;
                if let Some(ref mut state) = self.touch_state {
                    if state.id == id {
                        let delta_x = x - state.last_x;
                        let delta_y = y - state.last_y;
                        state.last_x = x;
                        state.last_y = y;

                        let moved_x = (x - state.start_x).abs();
                        let moved_y = (y - state.start_y).abs();

                        if state.has_scrolled || moved_x > 8.0 || moved_y > 8.0 {
                            state.has_scrolled = true;
                            let target_node = match state.scroll_node {
                                Some(node) => Some(node),
                                None => {
                                    let node = self.ui.find_scroll_container(state.start_x, state.start_y);
                                    state.scroll_node = node;
                                    node
                                }
                            };
                            if let Some(node) = target_node {
                                if self.ui.scroll_node(node, delta_x, delta_y) {
                                    self.scroll_offsets = self.ui.scroll_offsets.clone();
                                    redraw = true;
                                }
                            }
                        }
                    }
                }
                // A touch that scrolls no longer presses what it started on.
                let pressed = self.pressed && !self.touch_state.is_some_and(|t| t.has_scrolled);
                self.move_pointer(x, y, pressed) || redraw
            }
            InputEvent::TouchEnd { id, x, y } => {
                // Hover stays where the touch ended, as in mobile browsers.
                let mut redraw = self.move_pointer(x, y, false);
                if self.range_drag.is_some() && self.touch_state.is_some_and(|t| t.id == id) {
                    self.touch_state = None;
                    return self.end_range_drag() || redraw;
                }
                if let Some(state) = self.touch_state.take() {
                    if state.id == id {
                        let moved_x = (x - state.start_x).abs();
                        let moved_y = (y - state.start_y).abs();
                        if !state.has_scrolled && moved_x <= 8.0 && moved_y <= 8.0 {
                            redraw |= self.handle_event(InputEvent::Click { x, y });
                        }
                    }
                }
                redraw
            }
            InputEvent::TouchCancel { id, x, y } => {
                if let Some(state) = self.touch_state.as_ref() {
                    if state.id == id {
                        self.touch_state = None;
                        self.range_drag = None;
                    }
                }
                self.move_pointer(x, y, false)
            }
        }
    }

    /// Starts dragging the range input at (`x`, `y`), if there is one that
    /// sends messages, and moves its thumb there. Returns true if the view
    /// changed.
    fn start_range_drag(&mut self, x: f32, y: f32) -> bool {
        let Some(node) = self.ui.range_at(x, y) else { return false };
        let (Some(&key), Some(range)) = (self.ui.keys.get(node), self.ui.range(node)) else { return false };
        let start = range.current();
        self.range_drag = Some(RangeDrag { key, start, value: start });
        self.move_range_drag(x)
    }

    /// Moves the dragged range's thumb to `x`: sends its `data-on-input`
    /// message if that changes the value. Returns true if the view changed.
    fn move_range_drag(&mut self, x: f32) -> bool {
        let Some(mut drag) = self.range_drag else { return false };
        let Some(value) = self.ui.node_with_key(drag.key).and_then(|node| self.ui.range_value_at(node, x)) else { return false };
        if value == drag.value {
            return false;
        }
        drag.value = value;
        self.range_drag = Some(drag);
        self.send_range_value(drag.key, value, |handlers| &handlers.input)
    }

    /// Lets go of the dragged range: sends its `data-on-change` message if
    /// its value changed. Returns true if the view changed.
    fn end_range_drag(&mut self) -> bool {
        match self.range_drag.take() {
            Some(drag) if drag.value != drag.start => self.send_range_value(drag.key, drag.value, |handlers| &handlers.change),
            _ => false,
        }
    }

    /// Sends the message `handler` picks of the range with `key`, with
    /// `value` appended. Returns true if the view changed.
    fn send_range_value(&mut self, key: u64, value: f32, handler: fn(&crate::ui::ValueHandlers) -> &Option<String>) -> bool {
        let Some(node) = self.ui.node_with_key(key) else { return false };
        let (Some(range), Some(handlers)) = (self.ui.range(node), self.ui.value_handlers.get(node)) else { return false };
        let Some(prefix) = handler(handlers) else { return false };
        let message = format!("{prefix}:{}", range.format(value));
        self.process_message_str(&message)
    }

    /// Feeds multiple text messages into the update loop. Returns true if the view changes.
    ///
    /// While the on-screen keyboard is open, its messages (`kb:type:a`,
    /// `kb:ok`, ...) go to it.
    pub fn handle_messages(&mut self, messages: impl IntoIterator<Item = String>) -> bool {
        let mut redraw = false;
        let mut any_update = false;
        for msg_str in messages {
            if self.keyboard.is_some() {
                if let Ok(msg) = KeyboardMsg::from_str(&msg_str) {
                    if any_update {
                        redraw |= self.sync_view_inner(true);
                        any_update = false;
                    }
                    redraw |= self.update_keyboard(|keyboard| keyboard.press(msg));
                    continue;
                }
            }
            if let Ok(msg) = M::Message::from_str(&msg_str) {
                self.update(msg);
                any_update = true;
                // The keyboard opens now, for the messages after this one.
                if self.context.text_request.is_some() {
                    redraw |= self.sync_view_inner(true);
                    any_update = false;
                }
            }
        }
        if any_update {
            redraw |= self.sync_view_inner(true);
        }
        redraw
    }

    fn process_message(&mut self, msg: M::Message) -> bool {
        self.update(msg);
        self.sync_view_inner(true)
    }

    /// Hands the model `msg`, timed.
    fn update(&mut self, msg: M::Message) {
        self.update_model(|model, context| model.update(msg, context));
    }

    /// Runs `f` on the model, timed.
    fn update_model(&mut self, f: impl FnOnce(&mut M, &mut Context<M::Message>)) {
        let start = self.stats.start();
        f(&mut self.model, &mut self.context);
        self.stats.stop(Phase::Update, start);
    }

    /// Changes the model with `f` as an `update` would (`f` gets the
    /// context for timers, canvases, the keyboard), then syncs the view.
    /// Returns true if the view changed.
    pub fn update_with(&mut self, f: impl FnOnce(&mut M, &mut Context<M::Message>)) -> bool {
        self.update_model(f);
        self.sync_view_inner(true)
    }

    fn process_message_str(&mut self, msg_str: &str) -> bool {
        if let Ok(msg) = M::Message::from_str(msg_str) {
            self.process_message(msg)
        } else {
            log::debug!("Unhandled or failed to parse message: {}", msg_str);
            false
        }
    }

    /// Syncs the declarative view to the layout tree and triggers layout recalculation.
    ///
    /// Always performs a full rebuild (callers include backends after a viewport
    /// change). Message-driven syncs use the fingerprint-aware path internally,
    /// see [`Model::view_fingerprint`].
    pub fn sync_view(&mut self) -> bool {
        self.sync_view_inner(false)
    }

    fn sync_view_inner(&mut self, mut allow_skip: bool) -> bool {
        self.update_timers();
        if let Some((request, on_done)) = self.context.text_request.take() {
            self.open_keyboard(request, on_done);
            allow_skip = false;
        }
        let fingerprint = self.model.view_fingerprint();
        // Skipping is only possible when the model explicitly supports
        // fingerprinting and its view-relevant state is unchanged since the
        // last full rebuild. Viewport changes always force a rebuild (they run
        // through `sync_view` / `set_size`).
        let skip = allow_skip
            && fingerprint.is_some()
            && self.last_fingerprint.is_some()
            && fingerprint == self.last_fingerprint;

        let rebuilt = !skip;
        if rebuilt {
            self.rebuild();
            self.last_fingerprint = fingerprint;
        }

        let mut dirty = rebuilt;

        let commands: Vec<_> = self.context.commands.drain(..).collect();
        for cmd in commands {
            match cmd {
                crate::graphics::ContextCommand::ScrollIntoView(id) => {
                    self.scroll_into_view(&id);
                    dirty = true;
                }
            }
        }

        if !dirty {
            for cmd in &self.last_commands {
                if let DrawCommand::DrawCanvas { id, .. } = cmd {
                    if let Some(canvas) = self.context.canvases.get(id) {
                        if canvas.dirty {
                            dirty = true;
                            break;
                        }
                    }
                }
            }
        }

        dirty
    }

    /// Builds the view anew and lays it out. If that moves another element
    /// under the pointer, the view is built once more for its `:hover`.
    fn rebuild(&mut self) {
        self.build_view();
        if self.update_pointer_state() {
            self.build_view();
        }
    }

    fn build_view(&mut self) {
        self.builds = self.builds.wrapping_add(1);
        if let Some((w, h)) = self.viewport_px {
            crate::screen::set_viewport(w, h);
        }
        let start = self.stats.start();
        let ui = {
            // Elements not in the current view are new; in a new tree, all
            // are (also the first: the view built by `new` was never shown).
            let previous_keys = self.ui.uses_starting_style.then(|| match self.new_tree {
                true => HashSet::new(),
                false => self.ui.keys.values().copied().collect(),
            });
            match &self.keyboard {
                Some(session) => {
                    let validator = |s: &str| KeyboardMsg::from_str(s).is_ok();
                    Ui::new_compiled_after(&session.keyboard, &self.measurer, self.default_style.clone(), &validator, self.pointer_state.clone(), previous_keys).unwrap()
                }
                None => {
                    let validator = |s: &str| M::Message::from_str(s).is_ok();
                    Ui::new_compiled_after(&self.model, &self.measurer, self.default_style.clone(), &validator, self.pointer_state.clone(), previous_keys).unwrap()
                }
            }
        };
        // Kept until the transitions have compared the views.
        let mut old = core::mem::replace(&mut self.ui, ui);

        // The keyboard shows no images; the model's stay loaded for its view.
        if self.keyboard.is_none() {
            self.images.sync(self.ui.render_data.values().filter_map(|d| match &d.kind {
                RenderKind::Image(src) => Some(src.as_str()),
                _ => None,
            }));
        }
        Runtime::<M, R>::apply_image_sizes(&mut self.ui, &self.images);
        self.update_transitions(&old);
        self.stats.stop(Phase::View, start);
        let start = self.stats.start();
        self.ui.reuse_layout(&mut old, self.cached_size);
        self.stats.stop(Phase::Layout, start);
        self.layout();
        let start = self.stats.start();
        Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
        self.restore_scroll();
        self.new_tree = false;
        drop(old);
        self.stats.stop(Phase::View, start);
        self.animate(None);
    }

    /// Lays the view out at the viewport size, timed.
    fn layout(&mut self) {
        let start = self.stats.start();
        let _ = self.ui.compute_layout(self.cached_size);
        self.stats.stop(Phase::Layout, start);
    }

    /// Starts and stops transitions where the new view's styles differ from
    /// what `old`, the view it replaces, showed. As with animations, the
    /// studio leaves them to the browser.
    fn update_transitions(&mut self, old: &Ui) {
        if cfg!(target_arch = "wasm32") {
            return;
        }
        let now = self.now();
        let (animations, keyframes) = (&self.active_animations, &self.ui.keyframes);
        let animated = |key: u64, property| {
            animations.get(&key).and_then(|active| keyframes.get(&*active.name)).is_some_and(|k| {
                k.keyframes.iter().flat_map(|frame| &frame.declarations).any(|(p, _)| *p == property)
            })
        };
        let mut events = Vec::new();
        let old = (!self.new_tree).then_some(old);
        transition::update_transitions(&mut self.transitions, &self.ui, old, now, animated, &mut events);
        self.queue_transition_events(events);
    }

    /// Keeps the messages the elements send for `events`, for the next poll.
    fn queue_transition_events(&mut self, events: transition::Events) {
        for (node, event) in events {
            let Some(handlers) = self.ui.transition_handlers.get(node) else { continue };
            let handler = match event {
                transition::TransitionEvent::Run => &handlers.run,
                transition::TransitionEvent::Start => &handlers.start,
                transition::TransitionEvent::End => &handlers.end,
                transition::TransitionEvent::Cancel => &handlers.cancel,
            };
            self.transition_messages.extend(handler.clone());
        }
    }

    /// The model's messages for queued transition events.
    fn take_transition_messages(&mut self) -> Vec<M::Message> {
        core::mem::take(&mut self.transition_messages).into_iter().filter_map(|interaction| match interaction {
            Interaction::Message(message) => message.downcast_ref::<M::Message>().cloned(),
            Interaction::Text(text) => M::Message::from_str(&text).ok(),
        }).collect()
    }

    /// Sends an element's message (`data-on-click`, `data-on-hover`) to the
    /// model. Returns true if the view changed.
    fn dispatch(&mut self, interaction: Interaction) -> bool {
        if self.keyboard.is_some() {
            let msg = match interaction {
                Interaction::Message(message) => message.downcast_ref::<KeyboardMsg>().cloned(),
                Interaction::Text(text) => KeyboardMsg::from_str(&text).ok(),
            };
            return match msg {
                Some(msg) => self.update_keyboard(|keyboard| keyboard.press(msg)),
                None => false,
            };
        }
        match interaction {
            Interaction::Message(message) => match message.downcast_ref::<M::Message>() {
                Some(message) => self.process_message(message.clone()),
                None => {
                    log::error!("an element's message is not of the model's message type");
                    false
                }
            },
            Interaction::Text(text) => self.process_message_str(&text),
        }
    }

    /// Moves the view's keyframe animations and transitions on to `now` and
    /// applies them (transitions over animations, as in the cascade).
    /// Animations are kept by element key: one starts when an element
    /// declares an animation (or another name), at the clock's current
    /// time, and goes on through rebuilds. Called without a time after a
    /// build, so a rebuilt view shows its animations right away. Returns
    /// true if anything was set.
    fn animate(&mut self, now: Option<Instant>) -> bool {
        // In the browser (the studio), the page shows the template as HTML
        // and the browser runs its animations; nothing draws these styles.
        if cfg!(target_arch = "wasm32") || (self.ui.keyframes.is_empty() && self.active_animations.is_empty() && self.transitions.is_empty()) {
            return false;
        }
        let timer = self.stats.start();
        let start = now.unwrap_or_else(|| self.now());
        let mut declared = HashSet::new();
        for (node, data) in &self.ui.render_data {
            let (Some(name), Some(&key)) = (&data.style.animation_name, self.ui.keys.get(node)) else { continue };
            declared.insert(key);
            match self.active_animations.get_mut(&key) {
                Some(active) if active.name == *name => {
                    active.node_id = node;
                    active.play_state = data.style.animation_play_state;
                }
                _ => {
                    self.active_animations.insert(key, ActiveAnimation::new(node, name.clone(), &data.style, start));
                }
            }
        }
        // Elements gone or without the animation now: a rebuild already
        // gave them their own style.
        self.active_animations.retain(|key, _| declared.contains(key));

        let mut applied = false;
        let mut layout_affected = false;
        for active in self.active_animations.values_mut() {
            if let Some(now) = now {
                if active.is_running() {
                    active.elapsed += now.saturating_duration_since(active.updated);
                }
                active.updated = now;
            }
            let (Some((base_layout, base_style)), Some(keyframes)) = (self.ui.base_styles.get(active.node_id), self.ui.keyframes.get(&*active.name)) else {
                continue;
            };
            let mut layout = base_layout.clone();
            let mut style = base_style.clone();
            if let Some(progress) = active.progress() {
                animation::apply_keyframes(keyframes, progress, active.easing, base_style, base_layout, &mut style, &mut layout);
            }
            // Setting a style marks the node for layout: only one that changed.
            if self.ui.taffy.style(active.node_id).is_ok_and(|current| *current != layout) {
                let _ = self.ui.taffy.set_style(active.node_id, layout);
                layout_affected = true;
            }
            if let Some(data) = self.ui.render_data.get_mut(active.node_id) {
                data.style = style;
            }
            applied = true;
        }
        if !self.transitions.is_empty() {
            let mut events = Vec::new();
            layout_affected |= transition::apply_transitions(&mut self.transitions, &mut self.ui, start, &mut events);
            self.queue_transition_events(events);
            applied = true;
        }
        self.stats.stop(Phase::Animate, timer);
        if layout_affected {
            self.layout();
        }
        applied
    }

    /// Shows the keyboard for `request` in place of the model's view (or
    /// replaces the open keyboard's request). The caller rebuilds.
    fn open_keyboard(&mut self, request: TextRequest, on_done: OnText<M::Message>) {
        let scroll_offsets = match self.keyboard.take() {
            Some(session) => session.scroll_offsets,
            None => core::mem::take(&mut self.scroll_offsets),
        };
        self.keyboard = Some(KeyboardSession { keyboard: Keyboard::new(request), on_done, scroll_offsets });
        self.switch_tree();
    }

    /// Changes the keyboard with `f`, which returns its result once the
    /// user is done; then closes it and sends the model its message, or
    /// rebuilds the keyboard. Returns true (the view changed).
    fn update_keyboard(&mut self, f: impl FnOnce(&mut Keyboard) -> Option<Option<String>>) -> bool {
        let Some(session) = self.keyboard.as_mut() else { return false };
        let Some(result) = f(&mut session.keyboard) else {
            return self.sync_view_inner(false);
        };
        let session = self.keyboard.take().expect("the keyboard is open");
        self.scroll_offsets = session.scroll_offsets;
        self.switch_tree();
        if let Some(msg) = (session.on_done)(result) {
            self.update(msg);
        }
        self.sync_view_inner(false)
    }

    /// The next view is another tree: what the pointer was over, the
    /// focus, animations and transitions were the old tree's. Element keys
    /// of different templates can be the same, so nothing carries over.
    fn switch_tree(&mut self) {
        self.new_tree = true;
        self.focused_id = None;
        self.pointer_state = crate::ui::PointerState::default();
        self.active_animations.clear();
        self.transitions.clear();
        self.transition_messages.clear();
    }

    /// The pointer is at (`x`, `y`), pressed or not. Sends the
    /// `data-on-hover-out` messages of the elements it left, then the
    /// `data-on-hover` ones of those it entered, and restyles the view if
    /// what `:hover` / `:active` apply to changed. Returns true if the view
    /// changed.
    fn move_pointer(&mut self, x: f32, y: f32, pressed: bool) -> bool {
        self.pointer = Some((x, y));
        self.pressed = pressed;
        let before = self.pointer_state.hovered.clone();
        let restyle = self.update_pointer_state();
        let handler = |runtime: &Self, key: u64, enter: bool| {
            let (node, _) = runtime.ui.keys.iter().find(|&(_, &k)| k == key)?;
            let handlers = runtime.ui.hover_handlers.get(node)?;
            if enter { handlers.enter.clone() } else { handlers.leave.clone() }
        };
        let hovered = &self.pointer_state.hovered;
        let messages: Vec<Interaction> = before.iter().filter(|key| !hovered.contains(key)).filter_map(|&key| handler(self, key, false))
            .chain(hovered.iter().rev().filter(|key| !before.contains(key)).filter_map(|&key| handler(self, key, true)))
            .collect();
        let builds = self.builds;
        let mut changed = false;
        for message in messages {
            changed |= self.dispatch(message);
        }
        if restyle && self.builds == builds {
            self.rebuild();
            changed = true;
        }
        changed
    }

    /// Finds the elements under the pointer in the current view. Returns
    /// true if that changes what its `:hover` / `:active` rules apply to.
    fn update_pointer_state(&mut self) -> bool {
        let tracks_hover = self.ui.uses_hover || self.ui.hover_handlers.iter().next().is_some();
        let active = self.ui.uses_active && self.pressed;
        let chain = match self.pointer {
            Some((x, y)) if tracks_hover || active => self.ui.pointer_chain(x, y),
            _ => Vec::new(),
        };
        let state = crate::ui::PointerState {
            active: if active { chain.clone() } else { Vec::new() },
            hovered: if tracks_hover { chain } else { Vec::new() },
        };
        let restyle = (self.ui.uses_hover && state.hovered != self.pointer_state.hovered)
            || (self.ui.uses_active && state.active != self.pointer_state.active);
        self.pointer_state = state;
        restyle
    }

    /// Renders the current layout tree state into `target`, drawing only what
    /// changed since the previous frame, and returns the repainted areas.
    ///
    /// `target` must hold the previous frame, as drawn by this method; the
    /// first frame, a viewport change and newly loaded images repaint all.
    pub fn render(&mut self, renderer: &mut impl Renderer, target: &mut Surface<'_>) -> Damage {
        let start = self.stats.start();
        let commands = self.ui.build_commands(&self.context.canvases, self.focused_id.as_deref());

        let damage = if self.full_repaint {
            let screen = self.ui.taffy.layout(self.ui.root).map(|l| Rect::new(0.0, 0.0, l.size.width, l.size.height));
            Damage::full(screen.unwrap_or(Rect::new(0.0, 0.0, f32::MAX, f32::MAX)))
        } else {
            let mut damage = Damage::none();
            diff::damage_between(&self.last_commands, &commands, &mut damage);
            for cmd in &commands {
                if let DrawCommand::DrawCanvas { id, rect, .. } = cmd {
                    if self.context.canvases.get(id).is_some_and(|canvas| canvas.dirty) {
                        damage.add(*rect);
                    }
                }
            }
            damage
        };
        for canvas in self.context.canvases.values_mut() {
            canvas.dirty = false;
        }

        self.stats.stop(Phase::Paint, start);

        let start = self.stats.start();
        let resources = RenderResources::new(&self.context.canvases, &self.images);
        renderer.set_profiling(self.stats.reporting());
        for area in damage.rects() {
            renderer.render(target, &commands, &resources, *area);
        }
        self.stats.stop(Phase::Raster, start);

        let screen = target.width() as f32 * target.height() as f32;
        let repainted = damage.rects().iter().map(|r| r.width * r.height).sum::<f32>() / screen.max(1.0);
        self.stats.frame(commands.len(), repainted.min(1.0));
        #[cfg(feature = "std")]
        if let Some(stats) = self.stats.take_report(self.now()) {
            eprint!("{}", stats::report(&stats, &renderer.take_profile()));
        }

        self.full_repaint = false;
        self.last_commands = commands;
        damage
    }

    /// Makes the next [`render`](Self::render) draw the whole screen: for a
    /// target that no longer holds the previous frame (a new or reused
    /// buffer, a lost GPU texture).
    pub fn repaint_all(&mut self) {
        self.full_repaint = true;
    }

    /// Set the fixed dimensions of the layout viewport.
    ///
    /// When the size actually changes this updates the global viewport state
    /// (used by `@media` matching and `vw`/`vh` units), notifies the model via
    /// [`Model::on_resize`], rebuilds the UI tree so viewport-dependent styles
    /// re-resolve, and recomputes layout. Returns true when a redraw is needed.
    pub fn set_size(&mut self, width: f32, height: f32) -> bool {
        let size = Size {
            width: length(width),
            height: length(height),
        };
        if self.viewport_px == Some((width, height)) {
            // Laid out at this size already; winit calls this on every redraw.
            return false;
        }
        self.viewport_px = Some((width, height));
        self.full_repaint = true;
        crate::screen::set_viewport(width, height);
        self.cached_size = size;
        self.model.on_resize(width, height, &mut self.context);
        // sync_view rebuilds the tree (re-resolving @media/vw) and relayouts
        // with the updated cached_size.
        self.sync_view()
    }

    /// Current viewport size in CSS pixels as reported via [`Self::set_size`].
    pub fn viewport(&self) -> (f32, f32) {
        self.viewport_px.unwrap_or((0.0, 0.0))
    }

    /// Recalculates layout with the provided available space constraints.
    ///
    /// When `size` is fully definite this is equivalent to [`Self::set_size`]
    /// (including resize notification and rebuild); otherwise it only updates
    /// the layout constraints.
    #[doc(hidden)]
    pub fn compute_layout(&mut self, size: Size<AvailableSpace>) {
        let definite = match (size.width, size.height) {
            (AvailableSpace::Definite(w), AvailableSpace::Definite(h)) => Some((w, h)),
            _ => None,
        };
        if let Some((w, h)) = definite {
            self.set_size(w, h);
            return;
        }
        self.cached_size = size;
        let _ = self.ui.compute_layout(size);
    }
    
    /// Scrolls the element with this `id` attribute into view in its nearest scrollable ancestor.
    pub fn scroll_into_view(&mut self, id: &str) {
        self.ui.scroll_into_view(id);
        self.scroll_offsets = self.ui.scroll_offsets.clone();
    }

    /// Starts the timers `update` asked for, from the current time, and
    /// stops those it cancelled; then follows the model's subscriptions.
    fn update_timers(&mut self) {
        let now = self.now();
        let cancelled = core::mem::take(&mut self.context.cancelled_timers);
        self.timers.retain(|t| !cancelled.contains(&t.id));
        for pending in self.context.pending_timers.drain(..) {
            self.timers.push(Timer { id: pending.id, deadline: now + pending.delay, message: pending.message });
        }
        self.update_subscriptions();
    }

    /// Asks the model for its subscriptions. An `every` it asked for before
    /// (the n-th of its interval) keeps its phase; a new one starts now.
    fn update_subscriptions(&mut self) {
        let now = self.now();
        let subscriptions = self.model.subscriptions();
        let mut old = core::mem::take(&mut self.intervals);
        self.intervals = subscriptions.every.iter().map(|&(interval, _)| match old.iter().position(|i| i.interval == interval) {
            Some(i) => old.remove(i),
            None => Interval { interval, next: now + interval },
        }).collect();
        self.stats.subscribe(subscriptions.stats.as_ref().map(|(interval, _)| *interval), now);
        self.subscriptions = subscriptions;
    }

    /// Whether something wants frames: a running CSS animation or
    /// transition, or the model's frame subscription.
    fn wants_frames(&self) -> bool {
        self.subscriptions.frames.is_some() || !self.transitions.is_empty() || self.active_animations.values().any(ActiveAnimation::is_running)
    }

    /// Does the work that is due: applies loaded images, sends the model
    /// the messages of due timers and subscriptions, and on a frame (at the
    /// target frame rate) its frame message and the animations' next
    /// step. Everything due is handled together, with one rebuild. Returns
    /// whether to render, and when to poll again: input wakes the backend
    /// earlier, and it renders and polls again after handling it.
    pub fn poll(&mut self) -> Poll {
        let now = self.now();
        let mut redraw = false;

        // Finished background image loads: rebuild so sizes/layout reflect the new images,
        // and repaint everything, since an image's command can stay the same while its
        // placeholder turns into pixels.
        if self.images.poll() {
            self.full_repaint = true;
            redraw |= self.sync_view_inner(false);
        }

        // Transition events since the last poll (from input, or this
        // poll's image rebuild) come first, as they happened first.
        let mut messages = self.take_transition_messages();
        let mut due: Vec<usize> = (0..self.timers.len()).filter(|&i| self.timers[i].deadline <= now).collect();
        due.sort_by_key(|&i| self.timers[i].deadline);
        let mut fired: Vec<_> = due.iter().map(|&i| self.timers[i].clone()).collect();
        self.timers.retain(|t| t.deadline > now);
        messages.extend(fired.drain(..).map(|t| t.message));
        for (interval, (_, message)) in self.intervals.iter_mut().zip(&self.subscriptions.every) {
            if interval.next <= now {
                messages.push(message.clone());
                interval.next += interval.interval;
                if interval.next <= now {
                    interval.next = now + interval.interval;
                }
            }
        }

        if let (Some(stats), Some((_, message))) = (self.stats.take_due(now), &self.subscriptions.stats) {
            messages.push(message(stats));
        }

        // Frames that just started (from input, or the last poll's updates)
        // come at once.
        let frame = match self.frame_clock.or(Some((now, now))) {
            Some((previous, next)) if now >= next && self.wants_frames() => {
                let mut next = next + self.frame_interval();
                if next <= now {
                    // Too late for that frame too: drop it rather than catch up.
                    next = now + self.frame_interval();
                }
                self.frame_clock = Some((now, next));
                if let Some(message) = &self.subscriptions.frames {
                    messages.push(message(now.saturating_duration_since(previous).as_secs_f32()));
                }
                true
            }
            _ => false,
        };

        if !messages.is_empty() {
            for message in messages {
                self.update(message);
            }
            redraw |= self.sync_view_inner(true);
        }
        if frame {
            redraw |= self.animate(Some(now));
        }
        // Events of this poll's rebuild and frame, answered right away; what
        // those answers set off waits for the next poll.
        let messages = self.take_transition_messages();
        if !messages.is_empty() {
            for message in messages {
                self.update(message);
            }
            redraw |= self.sync_view_inner(true);
        }

        let wake = if !self.transition_messages.is_empty() {
            Wake::At(now)
        } else if self.wants_frames() {
            let interval = self.frame_interval();
            let (_, next) = *self.frame_clock.get_or_insert((now, now + interval));
            Wake::Frame(next)
        } else {
            self.frame_clock = None;
            let next_timer = self.timers.iter().map(|t| t.deadline);
            let next_interval = self.intervals.iter().map(|i| i.next);
            next_timer.chain(next_interval).chain(self.stats.due()).min().map_or(Wake::Idle, Wake::At)
        };
        Poll { redraw, wake }
    }
}
