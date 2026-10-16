//! Pseudo-elements xerune draws: `::before` / `::after` and the parts of
//! form controls.

/// A pseudo-element: `::before` / `::after`, or a part of a form control,
/// styled with the selectors pages use for Chromium and Firefox
/// (`::-webkit-slider-thumb`, `::-moz-range-thumb`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlPart {
    /// `<progress>`'s `::-webkit-progress-bar`: the track.
    ProgressBar,
    /// `<progress>`'s `::-webkit-progress-value` / `::-moz-progress-bar`.
    ProgressValue,
    /// A range's `::-webkit-slider-runnable-track` / `::-moz-range-track`.
    SliderTrack,
    /// A range's `::-webkit-slider-thumb` / `::-moz-range-thumb`.
    SliderThumb,
    /// A range's `::-moz-range-progress`: the track up to the thumb.
    SliderFill,
    /// A text input's `::placeholder`.
    Placeholder,
    /// `::before`: a box generated as the element's first child.
    Before,
    /// `::after`: a box generated as the element's last child.
    After,
}

impl ControlPart {
    const PARTS: [(ControlPart, &'static [&'static str]); 8] = [
        (ControlPart::ProgressBar, &["-webkit-progress-bar"]),
        (ControlPart::ProgressValue, &["-webkit-progress-value", "-moz-progress-bar"]),
        (ControlPart::SliderTrack, &["-webkit-slider-runnable-track", "-moz-range-track"]),
        (ControlPart::SliderThumb, &["-webkit-slider-thumb", "-moz-range-thumb"]),
        (ControlPart::SliderFill, &["-moz-range-progress"]),
        (ControlPart::Placeholder, &["placeholder", "-webkit-input-placeholder", "-moz-placeholder"]),
        (ControlPart::Before, &["before"]),
        (ControlPart::After, &["after"]),
    ];

    /// The part a pseudo-element name (without `::`) selects.
    pub fn from_pseudo_element(name: &str) -> Option<Self> {
        Self::PARTS.iter().find(|(_, names)| names.iter().any(|n| n.eq_ignore_ascii_case(name))).map(|(part, _)| *part)
    }

    /// Whether this is a part of a form control, not a generated box.
    pub fn of_form_control(self) -> bool {
        !matches!(self, ControlPart::Before | ControlPart::After)
    }
}
