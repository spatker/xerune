//! CSS transitions (CSS Transitions 1), kept by element key and property
//! across rebuilds.
//!
//! After each build, every property an element transitions is compared
//! with its value in the previous view (its before-change value, which is
//! what was on screen, a running transition included). A change starts a
//! transition from there. Ticks set the interpolated values on the current
//! view without rebuilding it.

use std::collections::{HashMap, HashSet};
use crate::css::transition::Transitions;
use crate::css::{AnimatedProperty as Property, AnimatedValue as Value, Easing};
use crate::runtime::time::Instant;
use crate::ui::Ui;
use core::time::Duration;
use taffy::prelude::*;

/// A transition of one property of one element.
#[derive(Clone, Debug)]
pub(crate) struct RunningTransition {
    /// The element's layout node in the current view.
    pub(crate) node_id: NodeId,
    pub(crate) start_value: Value,
    pub(crate) end_value: Value,
    /// Where the transition would have started had it not been reversed
    /// (the spec's reversing-adjusted start value).
    pub(crate) reversing_adjusted_start_value: Value,
    /// How much shorter it is for being a reversal (1: not shortened).
    pub(crate) reversing_shortening_factor: f32,
    /// When it starts moving, after its delay (earlier, for a negative one).
    pub(crate) start_time: Instant,
    pub(crate) duration: Duration,
    pub(crate) easing: Easing,
    /// Whether its delay is over (its start event was sent).
    pub(crate) started: bool,
}

/// What happened to a transition, for the element's
/// `data-on-transition*` messages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransitionEvent {
    Run,
    Start,
    End,
    Cancel,
}

/// Transition events, by the element's node in the current view.
pub(crate) type Events = Vec<(NodeId, TransitionEvent)>;

impl RunningTransition {
    /// How far it is at `now` (0..=1), before easing. It shows its start
    /// value during its delay.
    fn progress(&self, now: Instant) -> f32 {
        if now < self.start_time {
            return 0.0;
        }
        let elapsed = now.saturating_duration_since(self.start_time);
        if self.duration.is_zero() {
            return 1.0;
        }
        (elapsed.as_secs_f32() / self.duration.as_secs_f32()).min(1.0)
    }

    pub(crate) fn value_at(&self, now: Instant) -> Value {
        self.start_value.interpolate(&self.end_value, self.easing.apply(self.progress(now)))
    }

    pub(crate) fn is_finished(&self, now: Instant) -> bool {
        now >= self.start_time + self.duration
    }
}

/// The running transitions of a view, by element key and property.
pub(crate) type TransitionStore = HashMap<(u64, Property), RunningTransition>;

/// A transition from `start_value` to `end_value` starting `now`, unless
/// the values can't be interpolated (and `allow-discrete` isn't set).
fn new_transition(node_id: NodeId, start_value: Value, end_value: Value, timing: &crate::css::transition::TransitionTiming, now: Instant) -> Option<RunningTransition> {
    if !(timing.allow_discrete || start_value.interpolates_with(&end_value)) {
        return None;
    }
    Some(RunningTransition {
        node_id,
        reversing_adjusted_start_value: start_value.clone(),
        start_value,
        end_value,
        reversing_shortening_factor: 1.0,
        start_time: offset(now, timing.delay),
        duration: seconds(timing.duration),
        easing: timing.easing,
        started: false,
    })
}

fn offset(now: Instant, by: f32) -> Instant {
    if by >= 0.0 { now + seconds(by) } else { now - seconds(-by) }
}

/// A CSS time in seconds, to the microsecond: `0.2` in `f32` is a hair over
/// 200 ms, which would miss the frame at exactly 200 ms.
fn seconds(s: f32) -> Duration {
    // `+ 0.5` rounds: times are positive here.
    Duration::from_micros((s as f64 * 1e6 + 0.5) as u64)
}

/// The value `property` shows for `node` in `ui`: its style as drawn,
/// animations and transitions included.
fn shown(ui: &Ui, node: NodeId, property: Property) -> Option<Value> {
    property.get(&ui.render_data.get(node)?.style, ui.taffy.style(node).ok()?)
}

/// Starts, reverses and cancels the transitions of the newly built `ui`
/// against `old`, the view it replaces (`None`: the first view, whose
/// elements are all new), at `now`. An element new in `ui` starts from its
/// `@starting-style` style, if it has one. `animated` says which
/// properties of an element keyframe animations set; those don't
/// transition.
pub(crate) fn update_transitions(store: &mut TransitionStore, ui: &Ui, old: Option<&Ui>, now: Instant, animated: impl Fn(u64, Property) -> bool, events: &mut Events) {
    let transitioning: Vec<(NodeId, u64, &Transitions)> = ui
        .render_data
        .iter()
        .filter_map(|(node, data)| Some((node, *ui.keys.get(node)?, data.style.transitions.as_deref()?)))
        .collect();
    if transitioning.is_empty() && store.is_empty() {
        return;
    }
    let old_nodes: HashMap<u64, NodeId> = old.map(|old| old.keys.iter().map(|(node, &key)| (key, node)).collect()).unwrap_or_default();
    let mut declared = HashSet::new();

    for (node, key, transitions) in transitioning {
        let Some((base_layout, base_style)) = ui.base_styles.get(node) else { continue };
        for property in transitions.properties() {
            if animated(key, property) {
                continue;
            }
            let Some(after) = property.get(base_style, base_layout) else { continue };
            let timing = transitions.timing(property);
            if timing.is_some() {
                declared.insert((key, property));
            }
            match store.get_mut(&(key, property)) {
                Some(running) if running.end_value == after => running.node_id = node,
                Some(running) => {
                    let current = running.value_at(now);
                    let replacement = timing.as_ref().filter(|_| current != after && current.interpolates_with(&after)).and_then(|timing| {
                        if running.reversing_adjusted_start_value == after {
                            // Going back to where it came from, partway:
                            // take as long as the way back is (spec 3.1, 4.4).
                            let old_factor = running.reversing_shortening_factor;
                            let output = running.easing.apply(running.progress(now));
                            let factor = (output * old_factor + 1.0 - old_factor).abs().clamp(0.0, 1.0);
                            let delay = if timing.delay < 0.0 { timing.delay * factor } else { timing.delay };
                            Some(RunningTransition {
                                node_id: node,
                                reversing_adjusted_start_value: running.end_value.clone(),
                                start_value: current.clone(),
                                end_value: after.clone(),
                                reversing_shortening_factor: factor,
                                start_time: offset(now, delay),
                                duration: seconds(timing.duration * factor),
                                easing: timing.easing,
                                started: false,
                            })
                        } else {
                            new_transition(node, current.clone(), after.clone(), timing, now)
                        }
                    });
                    events.push((node, TransitionEvent::Cancel));
                    match replacement {
                        Some(transition) => {
                            events.push((node, TransitionEvent::Run));
                            *running = transition;
                        }
                        None => {
                            store.remove(&(key, property));
                        }
                    }
                }
                None => {
                    // An element that wasn't in the previous view starts
                    // from its `@starting-style` style, or doesn't transition.
                    let before = match (old, old_nodes.get(&key)) {
                        (Some(old), Some(&old_node)) => shown(old, old_node, property),
                        _ => ui.starting_styles.get(node).and_then(|(layout, style)| property.get(style, layout)),
                    };
                    let Some(before) = before else { continue };
                    if before == after {
                        continue;
                    }
                    if let Some(transition) = timing.as_ref().and_then(|timing| new_transition(node, before, after, timing, now)) {
                        events.push((node, TransitionEvent::Run));
                        store.insert((key, property), transition);
                    }
                }
            }
        }
    }
    // Gone, or no longer transitioning the property: it shows its own
    // value. Elements still there hear of it.
    if store.keys().any(|key| !declared.contains(key)) {
        let nodes: HashMap<u64, NodeId> = ui.keys.iter().map(|(node, &key)| (key, node)).collect();
        store.retain(|key, _| {
            let keep = declared.contains(key);
            if let (false, Some(&node)) = (keep, nodes.get(&key.0)) {
                events.push((node, TransitionEvent::Cancel));
            }
            keep
        });
    }
}

/// Sets each transition's value at `now` on `ui`, and drops the finished
/// ones (their end value is the element's own). Returns whether any
/// property that affects layout was set.
pub(crate) fn apply_transitions(store: &mut TransitionStore, ui: &mut Ui, now: Instant, events: &mut Events) -> bool {
    let mut layout_affected = false;
    for (&(_, property), transition) in store.iter_mut() {
        if !transition.started && now >= transition.start_time {
            transition.started = true;
            events.push((transition.node_id, TransitionEvent::Start));
        }
        if transition.is_finished(now) {
            events.push((transition.node_id, TransitionEvent::End));
        }
        let value = transition.value_at(now);
        let (Some(data), Ok(layout)) = (ui.render_data.get_mut(transition.node_id), ui.taffy.style(transition.node_id)) else { continue };
        let mut layout = layout.clone();
        property.set(&value, &mut data.style, &mut layout);
        if property.affects_layout() && ui.taffy.style(transition.node_id).is_ok_and(|current| *current != layout) {
            let _ = ui.taffy.set_style(transition.node_id, layout);
            layout_affected = true;
        }
    }
    store.retain(|_, transition| !transition.is_finished(now));
    layout_affected
}
