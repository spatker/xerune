//! Tests for the opt-in `Model::view_fingerprint` rebuild-skipping path.

use xerune::*;

#[derive(Debug, PartialEq, XeruneMessage)]
enum FpMsg {
    IncVisible,
    IncHidden,
}

struct FpModel {
    visible_counter: u32,
    hidden_counter: u32,
}

impl Model for FpModel {
    type Message = FpMsg;

    fn update(&mut self, msg: Self::Message, _context: &mut Context) {
        match msg {
            FpMsg::IncVisible => self.visible_counter += 1,
            FpMsg::IncHidden => self.hidden_counter += 1,
        }
    }

    // The view only renders `visible_counter`, so the fingerprint ignores
    // `hidden_counter` — messages touching it must not trigger a rebuild.
    fn view_fingerprint(&self) -> Option<u64> {
        Some(model::hash_bytes(&self.visible_counter.to_le_bytes()))
    }
}

impl TemplateLayout for FpModel {
    fn stylesheet(&self) -> &'static str {
        ""
    }
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _measurer: &impl TextMeasurer,
        _default_style: &ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        let root = builder.create_element("div", &[]);
        let text = builder.create_text(&self.visible_counter.to_string(), &[]);
        builder.append_child(root, text);
        root
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

fn rendered_text(runtime: &Runtime<FpModel, MockMeasurer>) -> Vec<String> {
    runtime
        .ui
        .render_data
        .values()
        .filter_map(|d| match d {
            RenderData::Text(t, _) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn fingerprint_skips_rebuild_when_view_state_unchanged() {
    let model = FpModel { visible_counter: 0, hidden_counter: 0 };
    let mut runtime = Runtime::new(model, MockMeasurer);
    runtime.set_size(200.0, 200.0);

    // Visible change → full rebuild, redraw needed, text updated.
    assert!(runtime.handle_event(InputEvent::Message("inc_visible".into())));
    assert!(rendered_text(&runtime).iter().any(|t| t == "1"));

    // Hidden-only change → fingerprint unchanged → rebuild skipped, no redraw.
    assert!(!runtime.handle_event(InputEvent::Message("inc_hidden".into())));
    assert_eq!(runtime.model().hidden_counter, 1);
    assert!(rendered_text(&runtime).iter().any(|t| t == "1"));

    // Another visible change → rebuild happens again.
    assert!(runtime.handle_event(InputEvent::Message("inc_visible".into())));
    assert!(rendered_text(&runtime).iter().any(|t| t == "2"));
}

#[test]
fn resize_forces_rebuild_even_with_stable_fingerprint() {
    let model = FpModel { visible_counter: 7, hidden_counter: 0 };
    let mut runtime = Runtime::new(model, MockMeasurer);
    // First call establishes a known viewport for this runtime (its return
    // value is nondeterministic: the process-global viewport may pre-match).
    runtime.set_size(200.0, 200.0);

    // Fingerprint unchanged across the resize (view state identical), but the
    // viewport change must still rebuild and report redraw.
    assert!(runtime.set_size(400.0, 300.0));
    assert_eq!(runtime.model().visible_counter, 7);
    assert!(rendered_text(&runtime).iter().any(|t| t == "7"));
}

// Models that do not implement view_fingerprint keep classic behavior:
// every processed message reports a redraw.
struct NoFpModel {
    hidden: u32,
}

impl Model for NoFpModel {
    type Message = FpMsg;
    fn update(&mut self, msg: Self::Message, _context: &mut Context) {
        match msg {
            FpMsg::IncVisible | FpMsg::IncHidden => self.hidden += 1,
        }
    }
}

impl TemplateLayout for NoFpModel {
    fn stylesheet(&self) -> &'static str {
        ""
    }
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _measurer: &impl TextMeasurer,
        _default_style: &ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        builder.create_element("div", &[])
    }
}

#[test]
fn no_fingerprint_always_rebuilds() {
    let mut runtime = Runtime::new(NoFpModel { hidden: 0 }, MockMeasurer);
    runtime.set_size(100.0, 100.0);
    assert!(runtime.handle_event(InputEvent::Message("inc_hidden".into())));
}
