use taffy::prelude::*;
use crate::css::{AnimatedProperty, AnimatedValue, Easing, KeyframesAnimation};
use crate::style::{ContainerStyle, AnimationIterationCount};
use crate::alloc_prelude::*;

/// Representation of an active CSS animation currently running on an
/// element. The runtime keeps it by the element's key, so it goes on when
/// the view is rebuilt.
#[derive(Clone, Debug)]
pub struct ActiveAnimation {
    /// The layout node of the element in the current view.
    pub node_id: NodeId,
    /// The name of the animation keyframe mapping.
    pub name: Arc<str>,
    /// Duration of the animation sequence in seconds.
    pub duration: f32,
    /// The element's `animation-timing-function`, for keyframes that set
    /// none of their own.
    pub easing: Easing,
    /// Delay before animation startup in seconds.
    pub delay: f32,
    /// Number of times the animation should execute.
    pub iteration_count: AnimationIterationCount,
    /// Progression direction of the keyframes sequence.
    pub direction: &'static str,
    /// Fill mode configuration.
    pub fill_mode: &'static str,
    /// Play state configuration ("running" or "paused").
    pub play_state: &'static str,
    /// Elapsed duration since animation start.
    pub elapsed: core::time::Duration,
    /// The time `elapsed` runs up to.
    pub updated: crate::runtime::time::Instant,
    /// Flags if the animation sequence has completed all iterations.
    pub is_finished: bool,
}

impl ActiveAnimation {
    /// The animation `name` an element with `style` (at `node_id`) declares,
    /// starting `now`.
    pub(crate) fn new(node_id: NodeId, name: Arc<str>, style: &ContainerStyle, now: crate::runtime::time::Instant) -> Self {
        ActiveAnimation {
            node_id,
            name,
            duration: style.animation_duration,
            easing: style.animation_timing_function,
            delay: style.animation_delay,
            iteration_count: style.animation_iteration_count,
            direction: style.animation_direction,
            fill_mode: style.animation_fill_mode,
            play_state: style.animation_play_state,
            elapsed: core::time::Duration::ZERO,
            updated: now,
            is_finished: false,
        }
    }

    /// Whether time moves it on: started, not finished and not paused.
    pub(crate) fn is_running(&self) -> bool {
        !self.is_finished && self.play_state != "paused"
    }

    fn reversed(&self, iteration: u32) -> bool {
        match self.direction {
            "reverse" => true,
            "alternate" => iteration % 2 == 1,
            "alternate-reverse" => iteration % 2 == 0,
            _ => false,
        }
    }

    /// Where in the keyframes the animation is (0 at `0%`, 1 at `100%`),
    /// after its direction; `None` when it has no effect (in its delay or
    /// after it ended, without a fill mode for that). Marks it finished
    /// once its iterations are over.
    pub(crate) fn progress(&mut self) -> Option<f32> {
        let fills = |mode: &str| self.fill_mode == mode || self.fill_mode == "both";
        let elapsed = self.elapsed.as_secs_f32() - self.delay;
        if elapsed < 0.0 {
            return fills("backwards").then(|| if self.reversed(0) { 1.0 } else { 0.0 });
        }
        let iterations = elapsed / self.duration.max(0.001);
        if let AnimationIterationCount::Count(count) = self.iteration_count {
            if iterations >= count {
                self.is_finished = true;
                if !fills("forwards") {
                    return None;
                }
                // The end of the last iteration, which may be partway.
                let last = (count.max(0.001) - 0.0001) as u32;
                let end = match count % 1.0 {
                    0.0 if count > 0.0 => 1.0,
                    part => part,
                };
                return Some(if self.reversed(last) { 1.0 - end } else { end });
            }
        }
        let iteration = iterations as u32;
        let progress = iterations % 1.0;
        Some(if self.reversed(iteration) { 1.0 - progress } else { progress })
    }
}

/// Sets each property `keyframes` animate to its value at `progress`, on
/// the element's own (`base_*`) style written into `style` / `layout`.
/// Each property goes between the nearest keyframes that set it, with
/// implicit `0%` / `100%` keyframes of the element's own value, and eases
/// with the timing function of the keyframe the segment starts at (else the
/// element's `easing`). Returns whether a property that affects layout was
/// set.
pub(crate) fn apply_keyframes(
    keyframes: &KeyframesAnimation,
    progress: f32,
    easing: Easing,
    base_style: &ContainerStyle,
    base_layout: &Style,
    style: &mut ContainerStyle,
    layout: &mut Style,
) -> bool {
    let mut properties: Vec<AnimatedProperty> = Vec::new();
    for (property, _) in keyframes.keyframes.iter().flat_map(|k| &k.declarations) {
        if !properties.contains(property) {
            properties.push(*property);
        }
    }
    let mut layout_affected = false;
    for property in properties {
        let Some(own) = property.get(base_style, base_layout) else { continue };
        let frames = keyframes.keyframes.iter().filter_map(|k| {
            let value = k.declarations.iter().find(|(p, _)| *p == property).map(|(_, v)| v)?;
            Some((k.percentage, value, k.easing))
        });
        let mut from: (f32, &AnimatedValue, Option<Easing>) = (0.0, &own, None);
        let mut to: (f32, &AnimatedValue) = (1.0, &own);
        let mut found_to = false;
        for (percentage, value, keyframe_easing) in frames {
            if percentage <= progress {
                from = (percentage, value, keyframe_easing);
            }
            if percentage >= progress && !found_to {
                to = (percentage, value);
                found_to = true;
            }
        }
        let t = if to.0 > from.0 { (progress - from.0) / (to.0 - from.0) } else { 0.0 };
        let eased = from.2.unwrap_or(easing).apply(t);
        property.set(&from.1.interpolate(to.1, eased), style, layout);
        layout_affected |= property.affects_layout();
    }
    layout_affected
}
