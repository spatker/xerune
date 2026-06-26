use std::str::FromStr;
use taffy::prelude::*;
use std::collections::HashMap;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Context, DrawCommand, Rect, TextMeasurer, Renderer};
use crate::style::{ContainerStyle, RenderData, AnimationIterationCount};
use crate::model::{InputEvent, Model};
use crate::ui::Ui;

#[derive(Clone, Debug)]
pub struct Timer {
    pub id: usize,
    pub message: String,
    pub interval: std::time::Duration,
    pub next_trigger: std::time::Instant,
    pub is_recurring: bool,
}

#[derive(Clone, Debug)]
pub struct ActiveAnimation {
    pub node_id: NodeId,
    pub name: std::sync::Arc<str>,
    pub duration: f32, // in seconds
    pub timing_function: std::sync::Arc<str>,
    pub delay: f32, // in seconds
    pub iteration_count: crate::style::AnimationIterationCount,
    pub direction: std::sync::Arc<str>,
    pub fill_mode: std::sync::Arc<str>,
    pub play_state: std::sync::Arc<str>,
    pub elapsed: std::time::Duration,
    pub is_finished: bool,
}

#[derive(Clone, Debug)]
pub struct TickResult {
    pub needs_redraw: bool,
    pub next_tick_in: std::time::Duration,
}

pub struct Runtime<M, R> {
    model: M,
    measurer: R,
    pub ui: Ui,
    default_style: ContainerStyle,
    pub(crate) scroll_offsets: HashMap<NodeId, (f32, f32)>, // Persist scroll offsets
    cached_size: Size<AvailableSpace>,
    context: Context,
    last_commands: Vec<DrawCommand>,
    pub focused_id: Option<String>,
    pub target_fps: u32,
    pub(crate) timers: Vec<Timer>,
    next_timer_id: usize,
    pub(crate) active_animations: HashMap<NodeId, ActiveAnimation>,
    last_tick_time: std::time::Instant,
}

impl<M: Model + crate::ui::TemplateLayout, R: TextMeasurer> Runtime<M, R> {
    pub fn new(model: M, measurer: R) -> Self {
         let default_style = ContainerStyle::default();
         let validator = |s: &str| M::Message::from_str(s).is_ok();
         let ui = Ui::new_compiled(&model, &measurer, default_style.clone(), &validator).unwrap();
         
         let mut context = Context::new();
         // Initial sync of canvases
         Runtime::<M, R>::sync_canvases(&ui, &mut context);

         Self {
             model,
             measurer,
             ui,
             default_style,
             scroll_offsets: HashMap::new(),
             cached_size: Size::MAX_CONTENT,
             context,
             last_commands: Vec::new(),
             focused_id: None,
             target_fps: 60,
             timers: Vec::new(),
             next_timer_id: 1,
             active_animations: HashMap::new(),
             last_tick_time: std::time::Instant::now(),
         }
    }

    fn sync_canvases(ui: &Ui, context: &mut Context) {
        for (node_id, data) in &ui.render_data {
            if let RenderData::Canvas(id, _style) = data {
                if !context.canvases.contains_key(id) {
                     // Try to get size from Taffy style (set by CSS or attributes)
                     let mut width = 200;
                     let mut height = 200;

                     if let Ok(style) = ui.taffy.style(*node_id) {
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

    pub fn handle_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::Click { x, y } => {
                if let Some((msg_str, clicked_node)) = self.ui.hit_test(x, y) {
                    // Update focus if this node is an input
                    if let Some(RenderData::TextInput(id, _, _)) = self.ui.render_data.get(&clicked_node) {
                        if !id.is_empty() {
                            self.focused_id = Some(id.clone());
                        } else {
                            self.focused_id = None;
                        }
                    } else {
                        self.focused_id = None;
                    }
                    
                    if !msg_str.is_empty() {
                        return self.process_message_str(&msg_str) || self.focused_id.is_some();
                    }
                    return self.focused_id.is_some(); // True if focus changed (needs redraw)
                }
                
                let old_focus = self.focused_id.take();
                return old_focus.is_some(); // True if changed
            }
            InputEvent::Message(msg_str) => {
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
                    // If the event provides an explicit ID, it must match.
                    if event_id.is_empty() || &event_id == focused {
                        let msg_str = format!("{}:text:{}", focused, text);
                        return self.process_message_str(&msg_str);
                    }
                }
                false
            }
            _ => false
        }
    }

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
            self.sync_view()
        } else {
            false
        }
    }

    
    fn process_message_str(&mut self, msg_str: &str) -> bool {
        if let Ok(msg) = M::Message::from_str(msg_str) {
            profile!("update");
            self.model.update(msg, &mut self.context);
            self.sync_view()
        } else {
            log::debug!("Unhandled or failed to parse message: {}", msg_str);
            false
        }
    }

    pub fn sync_view(&mut self) -> bool {
        self.ui = {
            profile!("ui_new_compiled");
            let validator = |s: &str| M::Message::from_str(s).is_ok();
            Ui::new_compiled(&self.model, &self.measurer, self.default_style.clone(), &validator).unwrap()
        };
        {
            profile!("compute_layout");
            let _ = self.ui.compute_layout(self.cached_size);
        }
        Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
        self.restore_scroll();
        let mut dirty = true;

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
            // Only trigger redraw if a *visible* canvas is dirty
            for cmd in &self.last_commands {
                if let DrawCommand::DrawCanvas { id, .. } = cmd {
                    if let Some(canvas) = self.context.canvases.get(id) {
                        if canvas.dirty {
                            dirty = true;
                            break; // We DO NOT reset canvas.dirty here!
                        }
                    }
                }
            }
        }

        dirty
    }

    
    pub fn render(&mut self, renderer: &mut impl Renderer) -> Option<Rect> {
        profile!("render");
        let commands = self.ui.build_commands(&self.context.canvases, self.focused_id.as_deref());
        
        let mut dirty_region: Option<Rect> = None;

        // Compare with last_commands
        let max_len = commands.len().max(self.last_commands.len());
        for i in 0..max_len {
            let cmd1 = commands.get(i);
            let cmd2 = self.last_commands.get(i);

            if cmd1 != cmd2 {
                if let Some(cmd) = cmd1 {
                    if let Some(b) = cmd.bounds() {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(b)),
                            None => Some(b),
                        };
                    }
                }
                if let Some(cmd) = cmd2 {
                    if let Some(b) = cmd.bounds() {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(b)),
                            None => Some(b),
                        };
                    }
                }
            }
        }

        // Also invalidate for dirty canvases
        for cmd in &commands {
            if let DrawCommand::DrawCanvas { id, rect } = cmd {
                if let Some(canvas) = self.context.canvases.get(id) {
                    if canvas.dirty {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(*rect)),
                            None => Some(*rect),
                        };
                    }
                }
            }
        }

        // After expanding dirty_region bounds to cover the changes,
        // reset the canvas dirty flags.
        for canvas in self.context.canvases.values_mut() {
            canvas.dirty = false;
        }

        renderer.render(&commands, &self.context.canvases, dirty_region);
        self.last_commands = commands;
        dirty_region
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
         let _ = self.ui.compute_layout(Size {
            width: length(width),
             height: length(height),
         });
    }

    pub fn compute_layout(&mut self, size: Size<AvailableSpace>) {
        self.cached_size = size;
        let _ = self.ui.compute_layout(size);
    }
    
    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.ui.scroll_into_view(interaction_id);
        self.scroll_offsets = self.ui.scroll_offsets.clone();
    }

    pub fn set_interval(&mut self, message: String, millis: u32) {
        let duration = std::time::Duration::from_millis(millis as u64);
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            message,
            interval: duration,
            next_trigger: std::time::Instant::now() + duration,
            is_recurring: true,
        });
    }

    pub fn set_timeout(&mut self, message: String, millis: u32) {
        let duration = std::time::Duration::from_millis(millis as u64);
        let id = self.next_timer_id;
        self.next_timer_id += 1;
        self.timers.push(Timer {
            id,
            message,
            interval: duration,
            next_trigger: std::time::Instant::now() + duration,
            is_recurring: false,
        });
    }

    pub fn tick(&mut self) -> TickResult {
        let now = std::time::Instant::now();
        let mut needs_redraw = false;

        // 1. Process Timers
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
        }

        let new_timers = std::mem::take(&mut self.context.pending_timers);
        for mut timer in new_timers {
            timer.id = self.next_timer_id;
            self.next_timer_id += 1;
            self.timers.push(timer);
        }

        // 2. Process Animations
        let dt = now.duration_since(self.last_tick_time);
        self.last_tick_time = now;

        if !self.ui.keyframes.is_empty() || !self.active_animations.is_empty() {
            // Sync declared animations
            let mut declared_animations = HashMap::new();
            for (&node_id, render_data) in &self.ui.render_data {
                let style = render_data.style();
                if let Some(ref name) = style.animation_name {
                    declared_animations.insert(node_id, (name.clone(), style));
                }
            }

            let mut animated_properties_changed = false;

            // Add or update active animations
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
                            elapsed: std::time::Duration::ZERO,
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
                        elapsed: std::time::Duration::ZERO,
                        is_finished: false,
                    });
                    animated_properties_changed = true;
                }
            }

            // Clean up animations that are no longer declared, and restore base styles
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
                            RenderData::Slider(_, style) => *style = base_container.clone(),
                            RenderData::Progress(_, _, style) => *style = base_container.clone(),
                            RenderData::Canvas(_, style) => *style = base_container.clone(),
                            RenderData::TextInput(_, _, style) => *style = base_container.clone(),
                        }
                    }
                }
            }

            let mut layout_affected = false;

            // Execute active animations
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

                // Determine easing progress t
                let mut apply_kf = true;
                let progress = if finished {
                    active.is_finished = true;
                    if &*active.fill_mode == "forwards" || &*active.fill_mode == "both" {
                        // Lock progress to final state
                        let final_raw = match active.iteration_count {
                            AnimationIterationCount::Infinite => 0.0,
                            AnimationIterationCount::Count(c) => c,
                        };
                        let final_iter = (final_raw.max(0.001) - 0.0001).floor() as u32;
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
                    // Delay phase
                    if &*active.fill_mode == "backwards" || &*active.fill_mode == "both" {
                        let is_rev = &*active.direction == "reverse" || &*active.direction == "alternate-reverse";
                        if is_rev { 1.0 } else { 0.0 }
                    } else {
                        apply_kf = false;
                        0.0
                    }
                } else {
                    let progress = raw_progress % 1.0;
                    let iteration = raw_progress.floor() as u32;
                    let is_rev = match &*active.direction {
                        "reverse" => true,
                        "alternate" => iteration % 2 == 1,
                        "alternate-reverse" => iteration % 2 == 0,
                        _ => false,
                    };
                    if is_rev { 1.0 - progress } else { progress }
                };

                let eased = if apply_kf {
                    ease(progress, &active.timing_function)
                } else {
                    0.0
                };

                if let Some((base_layout, base_container)) = self.ui.base_styles.get(node_id) {
                    let mut current_layout = base_layout.clone();
                    let mut current_container = base_container.clone();

                    if apply_kf {
                        if let Some(keyframes_anim) = self.ui.keyframes.get(&*active.name) {
                            let mut animated_properties = std::collections::HashSet::new();
                            for kf in &keyframes_anim.keyframes {
                                for (prop, _) in &kf.declarations {
                                    animated_properties.insert(prop.clone());
                                }
                            }

                            // Find bracketing keyframes once per node instead of inside the prop loop
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
                                let val1 = get_prop_val(kf1, &prop, base_container, base_layout);
                                let val2 = get_prop_val(kf2, &prop, base_container, base_layout);

                                if let (Some(v1), Some(v2)) = (val1, val2) {
                                    // Layout-affecting check
                                    if ["width", "height", "left", "right", "top", "bottom", "margin-left", "margin-right", "margin-top", "margin-bottom", "padding-left", "padding-right", "padding-top", "padding-bottom"].contains(&prop.as_str()) {
                                        layout_affected = true;
                                    }

                                    interpolate_property(&prop, &v1, &v2, segment_t, &mut current_container, &mut current_layout);
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
                            RenderData::Slider(_, style) => *style = current_container,
                            RenderData::Progress(_, _, style) => *style = current_container,
                            RenderData::Canvas(_, style) => *style = current_container,
                            RenderData::TextInput(_, _, style) => *style = current_container,
                        }
                    }
                    needs_redraw = true;
                }
            }

            if layout_affected {
                let _ = self.ui.compute_layout(self.cached_size);
            }
        }

        // 3. Compute Sleep Time
        let target_frame_duration = std::time::Duration::from_nanos((1_000_000_000.0 / self.target_fps as f64) as u64);
        
        let mut min_sleep = if self.active_animations.values().any(|a| !a.is_finished && &*a.play_state != "paused") {
            target_frame_duration
        } else {
            std::time::Duration::from_secs(3600 * 24)
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

fn ease(t: f32, func: &str) -> f32 {
    match func {
        "linear" => t,
        "ease" => solve_cubic_bezier(0.25, 0.1, 0.25, 1.0, t),
        "ease-in" => solve_cubic_bezier(0.42, 0.0, 1.0, 1.0, t),
        "ease-out" => solve_cubic_bezier(0.0, 0.0, 0.58, 1.0, t),
        "ease-in-out" => solve_cubic_bezier(0.42, 0.0, 0.58, 1.0, t),
        _ => {
            if func.starts_with("cubic-bezier(") && func.ends_with(')') {
                let inner = &func["cubic-bezier(".len()..func.len() - 1];
                let parts: Vec<&str> = inner.split(',').collect();
                if parts.len() == 4 {
                    let x1 = parts[0].trim().parse::<f32>().unwrap_or(0.0);
                    let y1 = parts[1].trim().parse::<f32>().unwrap_or(0.0);
                    let x2 = parts[2].trim().parse::<f32>().unwrap_or(1.0);
                    let y2 = parts[3].trim().parse::<f32>().unwrap_or(1.0);
                    return solve_cubic_bezier(x1, y1, x2, y2, t);
                }
            }
            t
        }
    }
}

fn solve_cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    if t <= 0.0 { return 0.0; }
    if t >= 1.0 { return 1.0; }

    let mut u = t;
    for _ in 0..8 {
        let x = sample_curve_x(x1, x2, u);
        let dx = sample_curve_derivative_x(x1, x2, u);
        if dx.abs() < 1e-6 {
            break;
        }
        let next_u = u - (x - t) / dx;
        u = next_u.clamp(0.0, 1.0);
    }
    sample_curve_y(y1, y2, u)
}

fn sample_curve_x(x1: f32, x2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * t * x1 + 3.0 * tm * t * t * x2 + t * t * t
}

fn sample_curve_y(y1: f32, y2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * t * y1 + 3.0 * tm * t * t * y2 + t * t * t
}

fn sample_curve_derivative_x(x1: f32, x2: f32, t: f32) -> f32 {
    let tm = 1.0 - t;
    3.0 * tm * tm * x1 + 6.0 * tm * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2)
}

fn interpolate_color(c1: crate::graphics::Color, c2: crate::graphics::Color, t: f32) -> crate::graphics::Color {
    crate::graphics::Color {
        r: ((1.0 - t) * c1.r as f32 + t * c2.r as f32).round() as u8,
        g: ((1.0 - t) * c1.g as f32 + t * c2.g as f32).round() as u8,
        b: ((1.0 - t) * c1.b as f32 + t * c2.b as f32).round() as u8,
        a: ((1.0 - t) * c1.a as f32 + t * c2.a as f32).round() as u8,
    }
}

fn interpolate_f32(v1: f32, v2: f32, t: f32) -> f32 {
    (1.0 - t) * v1 + t * v2
}

fn interpolate_length_percentage(lp1: LengthPercentage, lp2: LengthPercentage, t: f32) -> Option<LengthPercentage> {
    let r1 = lp1.into_raw();
    let r2 = lp2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(LengthPercentage::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(LengthPercentage::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

fn interpolate_length_percentage_auto(lp1: LengthPercentageAuto, lp2: LengthPercentageAuto, t: f32) -> Option<LengthPercentageAuto> {
    let r1 = lp1.into_raw();
    let r2 = lp2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(LengthPercentageAuto::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(LengthPercentageAuto::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

fn interpolate_dimension(d1: Dimension, d2: Dimension, t: f32) -> Option<Dimension> {
    let r1 = d1.into_raw();
    let r2 = d2.into_raw();
    match (r1.tag(), r2.tag()) {
        (taffy::style::CompactLength::LENGTH_TAG, taffy::style::CompactLength::LENGTH_TAG) => {
            Some(Dimension::length(interpolate_f32(r1.value(), r2.value(), t)))
        }
        (taffy::style::CompactLength::PERCENT_TAG, taffy::style::CompactLength::PERCENT_TAG) => {
            Some(Dimension::percent(interpolate_f32(r1.value(), r2.value(), t)))
        }
        _ => None,
    }
}

fn interpolate_property(
    prop: &str,
    val1: &str,
    val2: &str,
    t: f32,
    style: &mut ContainerStyle,
    taffy_style: &mut Style,
) {
    if ["color", "background-color", "border-color"].contains(&prop) {
        if let (Some(c1), Some(c2)) = (crate::css::parse_hex_color(val1), crate::css::parse_hex_color(val2)) {
            let interpolated = interpolate_color(c1, c2, t);
            match prop {
                "color" => style.color = interpolated,
                "background-color" => style.background_color = Some(interpolated),
                "border-color" => style.border_color = Some(interpolated),
                _ => {}
            }
        }
    }
    else if ["border-radius", "border-width", "font-size"].contains(&prop) {
        if let (Some(v1), Some(v2)) = (crate::css::parse_px(val1), crate::css::parse_px(val2)) {
            let interpolated = (1.0 - t) * v1 + t * v2;
            match prop {
                "border-radius" => style.border_radius = interpolated,
                "border-width" => style.border_width = interpolated,
                "font-size" => style.font_size = interpolated,
                _ => {}
            }
        }
    }
    else if ["width", "height"].contains(&prop) {
        if let (Some(d1), Some(d2)) = (crate::css::parse_dimension(val1), crate::css::parse_dimension(val2)) {
            if let Some(interpolated) = interpolate_dimension(d1, d2, t) {
                match prop {
                    "width" => {
                        taffy_style.size.width = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.width = Some(raw.value());
                        }
                    }
                    "height" => {
                        taffy_style.size.height = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.height = Some(raw.value());
                        }
                    }
                    _ => {}
                }
            }
        }
    }
    else if ["left", "right", "top", "bottom", "margin-left", "margin-right", "margin-top", "margin-bottom"].contains(&prop) {
        if let (Some(lp1), Some(lp2)) = (crate::css::parse_length_percentage_auto(val1), crate::css::parse_length_percentage_auto(val2)) {
            if let Some(interpolated) = interpolate_length_percentage_auto(lp1, lp2, t) {
                match prop {
                    "left" => taffy_style.inset.left = interpolated,
                    "right" => taffy_style.inset.right = interpolated,
                    "top" => taffy_style.inset.top = interpolated,
                    "bottom" => taffy_style.inset.bottom = interpolated,
                    "margin-left" => taffy_style.margin.left = interpolated,
                    "margin-right" => taffy_style.margin.right = interpolated,
                    "margin-top" => taffy_style.margin.top = interpolated,
                    "margin-bottom" => taffy_style.margin.bottom = interpolated,
                    _ => {}
                }
            }
        }
    }
    else if ["padding-left", "padding-right", "padding-top", "padding-bottom"].contains(&prop) {
        if let (Some(lp1), Some(lp2)) = (crate::css::parse_length_percentage(val1), crate::css::parse_length_percentage(val2)) {
            if let Some(interpolated) = interpolate_length_percentage(lp1, lp2, t) {
                match prop {
                    "padding-left" => {
                        taffy_style.padding.left = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_left = raw.value();
                        }
                    }
                    "padding-right" => {
                        taffy_style.padding.right = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_right = raw.value();
                        }
                    }
                    "padding-top" => {
                        taffy_style.padding.top = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_top = raw.value();
                        }
                    }
                    "padding-bottom" => {
                        taffy_style.padding.bottom = interpolated;
                        let raw = interpolated.into_raw();
                        if raw.tag() == taffy::style::CompactLength::LENGTH_TAG {
                            style.padding_bottom = raw.value();
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

fn get_prop_val(
    kf: Option<&crate::css::Keyframe>,
    prop: &str,
    base_container: &ContainerStyle,
    base_layout: &Style,
) -> Option<String> {
    if let Some(k) = kf {
        if let Some((_, val)) = k.declarations.iter().find(|(p, _)| p == prop) {
            return Some(val.clone());
        }
    }
    match prop {
        "color" => Some(format!("rgba({},{},{},{})", base_container.color.r, base_container.color.g, base_container.color.b, base_container.color.a as f32 / 255.0)),
        "background-color" => base_container.background_color.map(|c| format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a as f32 / 255.0)),
        "border-radius" => Some(format!("{}px", base_container.border_radius)),
        "border-width" => Some(format!("{}px", base_container.border_width)),
        "border-color" => base_container.border_color.map(|c| format!("rgba({},{},{},{})", c.r, c.g, c.b, c.a as f32 / 255.0)),
        "font-size" => Some(format!("{}px", base_container.font_size)),
        "width" => format_compact_length(base_layout.size.width.into_raw()),
        "height" => format_compact_length(base_layout.size.height.into_raw()),
        "padding-left" => format_compact_length(base_layout.padding.left.into_raw()),
        "padding-right" => format_compact_length(base_layout.padding.right.into_raw()),
        "padding-top" => format_compact_length(base_layout.padding.top.into_raw()),
        "padding-bottom" => format_compact_length(base_layout.padding.bottom.into_raw()),
        "margin-left" => format_compact_length(base_layout.margin.left.into_raw()),
        "margin-right" => format_compact_length(base_layout.margin.right.into_raw()),
        "margin-top" => format_compact_length(base_layout.margin.top.into_raw()),
        "margin-bottom" => format_compact_length(base_layout.margin.bottom.into_raw()),
        "left" => format_compact_length(base_layout.inset.left.into_raw()),
        "right" => format_compact_length(base_layout.inset.right.into_raw()),
        "top" => format_compact_length(base_layout.inset.top.into_raw()),
        "bottom" => format_compact_length(base_layout.inset.bottom.into_raw()),
        _ => None,
    }
}

fn format_compact_length(cl: taffy::style::CompactLength) -> Option<String> {
    match cl.tag() {
        taffy::style::CompactLength::LENGTH_TAG => Some(format!("{}px", cl.value())),
        taffy::style::CompactLength::PERCENT_TAG => Some(format!("{}%", cl.value() * 100.0)),
        _ => None,
    }
}
