/// Timer abstractions and callback ticking management.
pub mod timer;
/// Transition and keyframe animation tracking state.
pub mod animation;
/// Platform-independent monotonic time wrappers.
pub mod time;
mod diff;

pub use timer::{Timer, TickResult};
pub use animation::ActiveAnimation;

use core::str::FromStr;
use taffy::prelude::*;
use crate::alloc_prelude::*;
use crate::runtime::time::Instant;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Context, Damage, DrawCommand, Rect, TextMeasurer, Renderer, RenderResources, Surface};
use crate::images::ImageStore;
use crate::style::{ContainerStyle, RenderData, AnimationIterationCount};
use crate::model::{InputEvent, Model};
use crate::ui::{Interaction, Ui, NodeMap};

/// Message a backend's event proxy can send purely to wake the event loop (e.g. when a
/// background image load completes). It is not forwarded to the model.
pub const WAKE_MESSAGE: &str = "\u{0}xerune:wake";

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

/// The Elm/MVU runtime engine coordinating state updates, layout, input events, and rendering.
pub struct Runtime<M, R> {
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
    context: Context,
    last_commands: Vec<DrawCommand>,
    /// The next frame must be drawn completely: nothing is drawn yet, the
    /// viewport changed, or images arrived whose commands look unchanged.
    full_repaint: bool,
    /// Focused input ID if any text inputs are currently active.
    pub focused_id: Option<String>,
    /// Expected frames per second target.
    pub target_fps: u32,
    pub(crate) timers: Vec<Timer>,
    next_timer_id: usize,
    pub(crate) active_animations: HashMap<NodeId, ActiveAnimation>,
    last_tick_time: Instant,
    /// `(now_ms, instant)` from the first `tick_at_ms` call: host millisecond
    /// timestamps are measured relative to it, keeping that clock continuous
    /// with `last_tick_time` whatever the host's time origin is.
    ms_clock_anchor: Option<(u64, Instant)>,
    pub(crate) touch_state: Option<TouchState>,
    images: ImageStore,
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

         Self {
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
             target_fps: 60,
             timers: Vec::new(),
             next_timer_id: 1,
             active_animations: HashMap::new(),
             last_tick_time: Instant::now(),
             ms_clock_anchor: None,
             touch_state: None,
             images,
         }
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
    pub fn context(&self) -> &Context {
        &self.context
    }

    /// Get a mutable reference to the inner context.
    pub fn context_mut(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Get a reference to active timers in the runtime.
    pub fn timers(&self) -> &[Timer] {
        &self.timers
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
    /// Built-in backends install this automatically; the next [`Self::tick`] applies the result.
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
            .filter_map(|(node, data)| match data {
                RenderData::Image(src, _) => Some((node, images.get(src).map(|i| (i.width, i.height)))),
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

    fn sync_canvases(ui: &Ui, context: &mut Context) {
        for (node_id, data) in &ui.render_data {
            if let RenderData::Canvas(id, _style) = data {
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
            InputEvent::Click { x, y } => {
                if let Some((interaction, clicked_node)) = self.ui.hit_test(x, y) {
                    if let Some(RenderData::TextInput(id, _, _, _)) = self.ui.render_data.get(&clicked_node) {
                        if !id.is_empty() {
                            self.focused_id = Some(id.clone());
                        } else {
                            self.focused_id = None;
                        }
                    } else {
                        self.focused_id = None;
                    }
                    
                    let updated = match interaction {
                        Some(Interaction::Message(message)) => match message.downcast_ref::<M::Message>() {
                            Some(message) => self.process_message(message.clone()),
                            None => {
                                log::error!("a data-on-click message is not of the model's message type");
                                false
                            }
                        },
                        Some(Interaction::Text(text)) => self.process_message_str(&text),
                        None => false,
                    };
                    return updated || self.focused_id.is_some();
                }
                
                let old_focus = self.focused_id.take();
                return old_focus.is_some();
            }
            InputEvent::Message(msg_str) => {
                if msg_str == WAKE_MESSAGE {
                    return false;
                }
                self.process_message_str(&msg_str)
            }
            InputEvent::Scroll { x, y, delta_x, delta_y } => {
                if self.ui.handle_scroll(x, y, delta_x, delta_y) {
                    self.scroll_offsets = self.ui.scroll_offsets.clone();
                    return true;
                }
                false
            }
            InputEvent::KeyDown(key) => {
                let msg_str = format!("keydown:{}", key);
                self.process_message_str(&msg_str)
            }
            InputEvent::KeyUp(key) => {
                let msg_str = format!("keyup:{}", key);
                self.process_message_str(&msg_str)
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
                false
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
                redraw
            }
            InputEvent::TouchEnd { id, x, y } => {
                let mut redraw = false;
                if let Some(state) = self.touch_state.take() {
                    if state.id == id {
                        let moved_x = (x - state.start_x).abs();
                        let moved_y = (y - state.start_y).abs();
                        if !state.has_scrolled && moved_x <= 8.0 && moved_y <= 8.0 {
                            redraw = self.handle_event(InputEvent::Click { x, y });
                        }
                    }
                }
                redraw
            }
            InputEvent::TouchCancel { id, .. } => {
                if let Some(state) = self.touch_state.as_ref() {
                    if state.id == id {
                        self.touch_state = None;
                    }
                }
                false
            }
            _ => false
        }
    }

    /// Feeds multiple text messages into the update loop. Returns true if the view changes.
    pub fn handle_messages(&mut self, messages: impl IntoIterator<Item = String>) -> bool {
        let mut any_update = false;
        for msg_str in messages {
            if let Ok(msg) = M::Message::from_str(&msg_str) {
                profile!("update");
                self.model.update(msg, &mut self.context);
                any_update = true;
            }
        }
        if any_update {
            self.sync_view_inner(true)
        } else {
            false
        }
    }

    fn process_message(&mut self, msg: M::Message) -> bool {
        profile!("update");
        self.model.update(msg, &mut self.context);
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

    fn sync_view_inner(&mut self, allow_skip: bool) -> bool {
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
            if let Some((w, h)) = self.viewport_px {
                crate::screen::set_viewport(w, h);
            }
            self.ui = {
                profile!("ui_new_compiled");
                let validator = |s: &str| M::Message::from_str(s).is_ok();
                Ui::new_compiled(&self.model, &self.measurer, self.default_style.clone(), &validator).unwrap()
            };
            self.images.sync(self.ui.render_data.values().filter_map(|d| match d {
                RenderData::Image(src, _) => Some(src.as_str()),
                _ => None,
            }));
            Runtime::<M, R>::apply_image_sizes(&mut self.ui, &self.images);
            {
                profile!("compute_layout");
                let _ = self.ui.compute_layout(self.cached_size);
            }
            Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
            self.restore_scroll();
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
                crate::graphics::ContextCommand::ClearTimer(msg) => {
                    self.clear_timers(&msg);
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

    /// Renders the current layout tree state into `target`, drawing only what
    /// changed since the previous frame, and returns the repainted areas.
    ///
    /// `target` must hold the previous frame, as drawn by this method; the
    /// first frame, a viewport change and newly loaded images repaint all.
    pub fn render(&mut self, renderer: &mut impl Renderer, target: &mut Surface<'_>) -> Damage {
        profile!("render");
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

        let resources = RenderResources::new(&self.context.canvases, &self.images);
        for area in damage.rects() {
            renderer.render(target, &commands, &resources, *area);
        }
        self.full_repaint = false;
        self.last_commands = commands;
        damage
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
            self.cached_size = size;
            let _ = self.ui.compute_layout(size);
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

    /// Clears any active recurring or timeout timers matching the message string.
    pub fn clear_interval(&mut self, message: &str) {
        self.clear_timers(message);
    }

    /// Clears any active recurring or timeout timers matching the message string.
    pub fn clear_timeout(&mut self, message: &str) {
        self.clear_timers(message);
    }

    /// Also drops matching timers requested through [`Context`] that the next
    /// tick hasn't registered yet, so set-then-clear never resurrects one.
    fn clear_timers(&mut self, message: &str) {
        self.timers.retain(|t| t.message != message);
        self.context.pending_timers.retain(|t| t.message != message);
    }

    /// Registers a recurring interval timer.
    pub fn set_interval(&mut self, message: String, millis: u32) {
        self.add_timer(message, millis, true);
    }

    /// Registers a one-shot timeout timer.
    pub fn set_timeout(&mut self, message: String, millis: u32) {
        self.add_timer(message, millis, false);
    }

    fn add_timer(&mut self, message: String, millis: u32, is_recurring: bool) {
        let interval = core::time::Duration::from_millis(millis as u64);
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            message,
            interval,
            next_trigger: self.clock_now() + interval,
            is_recurring,
        });
    }

    /// The current time on the clock that drives `tick*`: the latest tick when
    /// the host supplies timestamps (`tick_at_ms`, or no `std` clock),
    /// otherwise real time.
    fn clock_now(&self) -> Instant {
        #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
        if self.ms_clock_anchor.is_none() {
            return Instant::now();
        }
        self.last_tick_time
    }

    /// Evaluates current animations and timers, updating state if necessary.
    /// Requires std monotonic time features.
    #[cfg(feature = "std")]
    pub fn tick(&mut self) -> TickResult {
        self.tick_with_time(Instant::now())
    }

    /// Evaluates animations and timers using the specified millisecond elapsed time duration.
    pub fn tick_at_ms(&mut self, now_ms: u64) -> TickResult {
        let (anchor_ms, anchor) = *self.ms_clock_anchor.get_or_insert((now_ms, self.last_tick_time));
        let now = anchor + core::time::Duration::from_millis(now_ms.saturating_sub(anchor_ms));
        self.tick_with_time(now)
    }

    /// Registers timers requested since the last tick. Deadlines are computed
    /// here, from the tick's clock, so timers work the same whichever clock
    /// drives the runtime (`tick`, `tick_at_ms`, or `tick_with_time`).
    fn drain_pending_timers(&mut self, now: Instant) {
        let new_timers = core::mem::take(&mut self.context.pending_timers);
        for mut timer in new_timers {
            timer.next_trigger = now + timer.interval;
            if !timer.is_recurring {
                if self.timers.iter().any(|t| !t.is_recurring && t.message == timer.message) {
                    continue;
                }
            }
            timer.id = self.next_timer_id;
            self.next_timer_id += 1;
            self.timers.push(timer);
        }
    }

    /// Evaluates animations and timers using the specified Instant time.
    pub fn tick_with_time(&mut self, now: Instant) -> TickResult {
        let mut needs_redraw = false;

        // Finished background image loads: rebuild so sizes/layout reflect the new images,
        // and repaint everything, since an image's command can stay the same while its
        // placeholder turns into pixels.
        if self.images.poll() {
            self.full_repaint = true;
            needs_redraw |= self.sync_view_inner(false);
        }

        self.drain_pending_timers(now);

        let mut triggered_messages = Vec::new();
        for timer in &mut self.timers {
            if now >= timer.next_trigger {
                triggered_messages.push(timer.message.clone());
                if timer.is_recurring {
                    timer.next_trigger = (timer.next_trigger + timer.interval).max(now);
                }
            }
        }
        
        self.timers.retain(|timer| !(!timer.is_recurring && now >= timer.next_trigger));

        if !triggered_messages.is_empty() {
            needs_redraw |= self.handle_messages(triggered_messages);
            self.drain_pending_timers(now);
        }

        let dt = now.duration_since(self.last_tick_time);
        self.last_tick_time = now;

        #[cfg(not(target_arch = "wasm32"))]
        if !self.ui.keyframes.is_empty() || !self.active_animations.is_empty() {
            let mut declared_animations = HashMap::new();
            for (node_id, render_data) in &self.ui.render_data {
                let style = render_data.style();
                if let Some(ref name) = style.animation_name {
                    declared_animations.insert(node_id, (name.clone(), style.clone()));
                }
            }

            let mut animated_properties_changed = false;

            for (node_id, (name, style)) in &declared_animations {
                let play_state = style.animation_play_state.clone();
                
                if let Some(active) = self.active_animations.get_mut(node_id) {
                    if active.name != *name {
                        *active = ActiveAnimation {
                            node_id: *node_id,
                            name: name.clone(),
                            duration: style.animation_duration,
                            timing_function: style.animation_timing_function.clone(),
                            delay: style.animation_delay,
                            iteration_count: style.animation_iteration_count,
                            direction: style.animation_direction.clone(),
                            fill_mode: style.animation_fill_mode.clone(),
                            play_state: play_state.clone(),
                            elapsed: core::time::Duration::ZERO,
                            is_finished: false,
                        };
                        animated_properties_changed = true;
                    } else {
                        active.play_state = play_state;
                    }
                } else {
                    self.active_animations.insert(*node_id, ActiveAnimation {
                        node_id: *node_id,
                        name: name.clone(),
                        duration: style.animation_duration,
                        timing_function: style.animation_timing_function.clone(),
                        delay: style.animation_delay,
                        iteration_count: style.animation_iteration_count,
                        direction: style.animation_direction.clone(),
                        fill_mode: style.animation_fill_mode.clone(),
                        play_state: play_state.clone(),
                        elapsed: core::time::Duration::ZERO,
                        is_finished: false,
                    });
                    animated_properties_changed = true;
                }
            }

            let mut nodes_to_restore = Vec::new();
            self.active_animations.retain(|node_id, _| {
                let keep = declared_animations.contains_key(node_id);
                if !keep {
                    nodes_to_restore.push(*node_id);
                    animated_properties_changed = true;
                }
                keep
            });

            for node_id in nodes_to_restore {
                if let Some((base_layout, base_container)) = self.ui.base_styles.get(&node_id) {
                    let _ = self.ui.taffy.set_style(node_id, base_layout.clone());
                    if let Some(render_data) = self.ui.render_data.get_mut(&node_id) {
                        match render_data {
                            RenderData::Container(style) => *style = base_container.clone(),
                            RenderData::Text(_, style) => *style = base_container.clone(),
                            RenderData::Image(_, style) => *style = base_container.clone(),
                            RenderData::Checkbox(_, style) => *style = base_container.clone(),
                            RenderData::Slider(_, style, _) => *style = base_container.clone(),
                            RenderData::Progress(_, _, style, _) => *style = base_container.clone(),
                            RenderData::Canvas(_, style) => *style = base_container.clone(),
                            RenderData::TextInput(_, _, style, _) => *style = base_container.clone(),
                        }
                    }
                }
            }

            let mut layout_affected = false;

            for (node_id, active) in &mut self.active_animations {
                if active.is_finished {
                    continue;
                }

                if &*active.play_state != "paused" {
                    active.elapsed += dt;
                }

                let elapsed_sec = active.elapsed.as_secs_f32() - active.delay;
                let duration = active.duration.max(0.001);
                let raw_progress = elapsed_sec / duration;

                let finished = match active.iteration_count {
                    AnimationIterationCount::Infinite => false,
                    AnimationIterationCount::Count(count) => raw_progress >= count,
                };

                let mut apply_kf = true;
                let progress = if finished {
                    active.is_finished = true;
                    if &*active.fill_mode == "forwards" || &*active.fill_mode == "both" {
                        let final_raw = match active.iteration_count {
                            AnimationIterationCount::Infinite => 0.0,
                            AnimationIterationCount::Count(c) => c,
                        };
                        let final_iter = (final_raw.max(0.001) - 0.0001) as u32;
                        let mut final_p = final_raw % 1.0;
                        if final_p == 0.0 && final_raw > 0.0 {
                            final_p = 1.0;
                        }
                        let is_rev = match &*active.direction {
                            "reverse" => true,
                            "alternate" => final_iter % 2 == 1,
                            "alternate-reverse" => final_iter % 2 == 0,
                            _ => false,
                        };
                        if is_rev { 1.0 - final_p } else { final_p }
                    } else {
                        apply_kf = false;
                        0.0
                    }
                } else if elapsed_sec < 0.0 {
                    if &*active.fill_mode == "backwards" || &*active.fill_mode == "both" {
                        let is_rev = &*active.direction == "reverse" || &*active.direction == "alternate-reverse";
                        if is_rev { 1.0 } else { 0.0 }
                    } else {
                        apply_kf = false;
                        0.0
                    }
                } else {
                    let progress = raw_progress % 1.0;
                    let iteration = raw_progress as u32;
                    let is_rev = match &*active.direction {
                        "reverse" => true,
                        "alternate" => iteration % 2 == 1,
                        "alternate-reverse" => iteration % 2 == 0,
                        _ => false,
                    };
                    if is_rev { 1.0 - progress } else { progress }
                };

                let eased = if apply_kf {
                    animation::ease(progress, &active.timing_function)
                } else {
                    0.0
                };

                if let Some((base_layout, base_container)) = self.ui.base_styles.get(node_id) {
                    let mut current_layout = base_layout.clone();
                    let mut current_container = base_container.clone();

                    if apply_kf {
                        if let Some(keyframes_anim) = self.ui.keyframes.get(&*active.name) {
                            let mut animated_properties = HashSet::new();
                            for kf in &keyframes_anim.keyframes {
                                for (prop, _) in &kf.declarations {
                                    animated_properties.insert(prop.clone());
                                }
                            }

                            let mut kf1: Option<&crate::css::Keyframe> = None;
                            let mut kf2: Option<&crate::css::Keyframe> = None;
                            for kf in &keyframes_anim.keyframes {
                                if kf.percentage <= eased {
                                    kf1 = Some(kf);
                                }
                                if kf.percentage >= eased && kf2.is_none() {
                                    kf2 = Some(kf);
                                }
                            }

                            let p1 = kf1.map(|k| k.percentage).unwrap_or(0.0);
                            let p2 = kf2.map(|k| k.percentage).unwrap_or(1.0);
                            let segment_t = if p2 > p1 {
                                (eased - p1) / (p2 - p1)
                            } else {
                                1.0
                            };

                            for prop in animated_properties {
                                let val1 = animation::get_prop_val(kf1, &prop, base_container, base_layout);
                                let val2 = animation::get_prop_val(kf2, &prop, base_container, base_layout);

                                if let (Some(v1), Some(v2)) = (val1, val2) {
                                    if ["width", "height", "left", "right", "top", "bottom", "margin-left", "margin-right", "margin-top", "margin-bottom", "padding-left", "padding-right", "padding-top", "padding-bottom"].contains(&prop.as_str()) {
                                        layout_affected = true;
                                    }

                                    animation::interpolate_property(&prop, &v1, &v2, segment_t, &mut current_container, &mut current_layout);
                                }
                            }
                        }
                    }

                    let _ = self.ui.taffy.set_style(*node_id, current_layout);
                    if let Some(render_data) = self.ui.render_data.get_mut(node_id) {
                        match render_data {
                            RenderData::Container(style) => *style = current_container,
                            RenderData::Text(_, style) => *style = current_container,
                            RenderData::Image(_, style) => *style = current_container,
                            RenderData::Checkbox(_, style) => *style = current_container,
                            RenderData::Slider(_, style, _) => *style = current_container,
                            RenderData::Progress(_, _, style, _) => *style = current_container,
                            RenderData::Canvas(_, style) => *style = current_container,
                            RenderData::TextInput(_, _, style, _) => *style = current_container,
                        }
                    }
                    needs_redraw = true;
                }
            }

            if layout_affected {
                let _ = self.ui.compute_layout(self.cached_size);
            }
        }

        let target_frame_duration = core::time::Duration::from_nanos((1_000_000_000.0 / self.target_fps as f64) as u64);
        
        let mut min_sleep = if self.active_animations.values().any(|a| !a.is_finished && &*a.play_state != "paused") {
            target_frame_duration
        } else {
            core::time::Duration::from_secs(3600 * 24)
        };

        for timer in &self.timers {
            let remaining = timer.next_trigger.saturating_duration_since(now);
            if remaining < min_sleep {
                min_sleep = remaining;
            }
        }

        TickResult {
            needs_redraw,
            next_tick_in: min_sleep,
        }
    }
}
