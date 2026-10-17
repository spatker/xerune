//! `transition` and its longhands (CSS Transitions 1).

use super::animation::{parse_time, Easing, Property, Side};
use crate::alloc_prelude::*;

/// An element's `transition-*` values. Each longhand is a list; the
/// `transition-property` list decides how many transitions there are, and
/// the other lists repeat to its length.
#[derive(Clone, Debug, PartialEq)]
pub struct Transitions {
    pub properties: Vec<TransitionProperty>,
    pub durations: Vec<f32>,
    pub delays: Vec<f32>,
    pub easings: Vec<Easing>,
    /// `transition-behavior: allow-discrete`: values that can't be
    /// interpolated transition too, flipping halfway.
    pub allow_discrete: Vec<bool>,
}

/// An entry of `transition-property`.
#[derive(Clone, Debug, PartialEq)]
pub enum TransitionProperty {
    All,
    None,
    /// The properties a name stands for: one, the sides of `margin`, ...;
    /// none for a name that isn't supported, which still takes its place in
    /// the list.
    Only(Vec<Property>),
}

/// How one property transitions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TransitionTiming {
    /// Seconds.
    pub duration: f32,
    /// Seconds; negative starts partway.
    pub delay: f32,
    pub easing: Easing,
    pub allow_discrete: bool,
}

const EASE: Easing = Easing::CubicBezier(0.25, 0.1, 0.25, 1.0);
const SIDES: [Side; 4] = [Side::Top, Side::Right, Side::Bottom, Side::Left];

/// What `all` transitions. `background` covers `background-color`.
const ALL: [Property; 21] = [
    Property::Color,
    Property::Background,
    Property::BorderColor,
    Property::BorderWidth,
    Property::BorderRadius,
    Property::FontSize,
    Property::Opacity,
    Property::Width,
    Property::Height,
    Property::Inset(Side::Top),
    Property::Inset(Side::Right),
    Property::Inset(Side::Bottom),
    Property::Inset(Side::Left),
    Property::Margin(Side::Top),
    Property::Margin(Side::Right),
    Property::Margin(Side::Bottom),
    Property::Margin(Side::Left),
    Property::Padding(Side::Top),
    Property::Padding(Side::Right),
    Property::Padding(Side::Bottom),
    Property::Padding(Side::Left),
];

impl Default for Transitions {
    /// The initial values: `all 0s ease 0s`, which transitions nothing.
    fn default() -> Self {
        Self { properties: vec![TransitionProperty::All], durations: vec![0.0], delays: vec![0.0], easings: vec![EASE], allow_discrete: vec![false] }
    }
}

impl TransitionProperty {
    fn parse(name: &str) -> Self {
        let name = name.trim().to_ascii_lowercase();
        let sides = |property: fn(Side) -> Property| TransitionProperty::Only(SIDES.map(property).to_vec());
        match name.as_str() {
            "all" => TransitionProperty::All,
            "none" => TransitionProperty::None,
            "margin" => sides(Property::Margin),
            "padding" => sides(Property::Padding),
            "inset" => sides(Property::Inset),
            "border" => TransitionProperty::Only(vec![Property::BorderWidth, Property::BorderColor]),
            _ => TransitionProperty::Only(Property::from_name(&name).into_iter().collect()),
        }
    }

    fn matches(&self, property: Property) -> bool {
        match self {
            TransitionProperty::All => ALL.contains(&property),
            TransitionProperty::None => false,
            TransitionProperty::Only(properties) => properties.contains(&property),
        }
    }
}

impl Transitions {
    /// How `property` transitions, if it does: the last entry of
    /// `transition-property` naming it wins.
    pub fn timing(&self, property: Property) -> Option<TransitionTiming> {
        let i = self.properties.iter().rposition(|p| p.matches(property))?;
        let pick = |list: &[f32], default| if list.is_empty() { default } else { list[i % list.len()] };
        Some(TransitionTiming {
            duration: pick(&self.durations, 0.0).max(0.0),
            delay: pick(&self.delays, 0.0),
            easing: if self.easings.is_empty() { EASE } else { self.easings[i % self.easings.len()] },
            allow_discrete: !self.allow_discrete.is_empty() && self.allow_discrete[i % self.allow_discrete.len()],
        })
        // A combined duration of zero or less doesn't transition.
        .filter(|t| t.duration + t.delay > 0.0)
    }

    /// The properties that may transition (some may have no duration).
    pub fn properties(&self) -> Vec<Property> {
        if self.properties.contains(&TransitionProperty::All) {
            return ALL.to_vec();
        }
        let mut properties = Vec::new();
        for property in self.properties.iter().filter_map(|p| match p {
            TransitionProperty::Only(properties) => Some(properties),
            _ => None,
        }).flatten() {
            if !properties.contains(property) {
                properties.push(*property);
            }
        }
        properties
    }
}

fn list(val: &str) -> Vec<&str> {
    xerune_css::text::split_top_level(val, b',').into_iter().map(str::trim).collect()
}

/// Applies a `transition-*` longhand or the `transition` shorthand to
/// `transitions`. Returns false if `prop` isn't one; a value that doesn't
/// parse leaves the property as it was.
pub fn apply_transition_property(prop: &str, val: &str, transitions: &mut Transitions) -> bool {
    match prop {
        "transition-property" => transitions.properties = list(val).into_iter().map(TransitionProperty::parse).collect(),
        "transition-duration" => {
            if let Some(times) = list(val).into_iter().map(parse_time).collect() {
                transitions.durations = times;
            }
        }
        "transition-delay" => {
            if let Some(times) = list(val).into_iter().map(parse_time).collect() {
                transitions.delays = times;
            }
        }
        "transition-timing-function" => {
            if let Some(easings) = list(val).into_iter().map(Easing::parse).collect() {
                transitions.easings = easings;
            }
        }
        "transition-behavior" => {
            let behaviors: Option<Vec<bool>> = list(val).into_iter().map(|b| match b.to_ascii_lowercase().as_str() {
                "allow-discrete" => Some(true),
                "normal" => Some(false),
                _ => None,
            }).collect();
            if let Some(behaviors) = behaviors {
                transitions.allow_discrete = behaviors;
            }
        }
        "transition" => {
            if let Some(parsed) = parse_shorthand(val) {
                *transitions = parsed;
            }
        }
        _ => return false,
    }
    true
}

/// `transition: <property> || <duration> || <easing> || <delay> ||
/// <behavior>`, comma-separated.
fn parse_shorthand(val: &str) -> Option<Transitions> {
    let mut transitions = Transitions { properties: Vec::new(), durations: Vec::new(), delays: Vec::new(), easings: Vec::new(), allow_discrete: Vec::new() };
    for single in list(val) {
        let single = single.replace(['\t', '\n', '\r'], " ");
        let (mut property, mut times, mut easing, mut discrete) = (None, Vec::new(), None, false);
        for part in xerune_css::text::split_top_level(&single, b' ').into_iter().filter(|p| !p.is_empty()) {
            if let Some(time) = parse_time(part) {
                times.push(time);
            } else if let Some(e) = Easing::parse(part) {
                easing = Some(e);
            } else if part.eq_ignore_ascii_case("allow-discrete") {
                discrete = true;
            } else if part.eq_ignore_ascii_case("normal") {
                discrete = false;
            } else if property.is_none() {
                property = Some(TransitionProperty::parse(part));
            } else {
                return None;
            }
        }
        if times.len() > 2 {
            return None;
        }
        transitions.properties.push(property.unwrap_or(TransitionProperty::All));
        transitions.durations.push(times.first().copied().unwrap_or(0.0));
        transitions.delays.push(times.get(1).copied().unwrap_or(0.0));
        transitions.easings.push(easing.unwrap_or(EASE));
        transitions.allow_discrete.push(discrete);
    }
    Some(transitions)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(declarations: &[(&str, &str)]) -> Transitions {
        let mut transitions = Transitions::default();
        for (prop, val) in declarations {
            assert!(apply_transition_property(prop, val, &mut transitions));
        }
        transitions
    }

    #[test]
    fn the_shorthand_reads_each_transition() {
        let t = parsed(&[("transition", "left 0.2s ease-out, background-color 150ms linear 50ms")]);
        let left = t.timing(Property::Inset(Side::Left)).unwrap();
        assert_eq!((left.duration, left.delay, left.easing), (0.2, 0.0, Easing::parse("ease-out").unwrap()));
        let bg = t.timing(Property::BackgroundColor).unwrap();
        assert_eq!((bg.duration, bg.delay, bg.easing), (0.15, 0.05, Easing::Linear));
        assert_eq!(t.timing(Property::Color), None);
        assert_eq!(t.properties(), [Property::Inset(Side::Left), Property::BackgroundColor]);
    }

    #[test]
    fn longhand_lists_repeat_to_the_property_list() {
        let t = parsed(&[("transition-property", "color, margin, width"), ("transition-duration", "1s, 2s")]);
        assert_eq!(t.timing(Property::Color).unwrap().duration, 1.0);
        assert_eq!(t.timing(Property::Margin(Side::Bottom)).unwrap().duration, 2.0);
        assert_eq!(t.timing(Property::Width).unwrap().duration, 1.0, "the durations repeat");
    }

    #[test]
    fn all_and_none() {
        let t = parsed(&[("transition", "all 1s")]);
        assert!(t.timing(Property::Padding(Side::Left)).is_some());
        assert!(t.timing(Property::Background).is_some());
        let t = parsed(&[("transition", "none")]);
        assert!(t.properties().is_empty());
        assert_eq!(Transitions::default().timing(Property::Color), None, "0s by default");
    }

    #[test]
    fn the_last_entry_naming_a_property_wins() {
        let t = parsed(&[("transition", "all 1s, color 3s")]);
        assert_eq!(t.timing(Property::Color).unwrap().duration, 3.0);
        assert_eq!(t.timing(Property::Width).unwrap().duration, 1.0);
    }

    #[test]
    fn invalid_values_are_ignored() {
        let t = parsed(&[("transition", "color 1s"), ("transition", "color 1s 2s 3s"), ("transition-duration", "fast")]);
        assert_eq!(t.timing(Property::Color).unwrap().duration, 1.0);
    }
}
