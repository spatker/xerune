//! End-to-end responsive-feature tests: `@media` in compiled templates,
//! `vw`/`vh` units, `Model::on_resize` notification, breakpoints, and the
//! dynamic (manual `TemplateLayout`) stylesheet expansion path.
//!
//! Everything viewport-dependent lives in ONE test function: the viewport is a
//! process-global and cargo runs tests within a binary on parallel threads,
//! which would otherwise race mid-rebuild.

use xerune::*;

#[derive(Debug, PartialEq, XeruneMessage)]
enum RespMsg {
    Noop,
    ProbeVw,
    ProbeMedia,
    ProbeLand,
    DynMedia,
    DynVw,
}

// ---------------------------------------------------------------------------
// Compiled-template path (XeruneTemplate derive)
// ---------------------------------------------------------------------------

#[derive(XeruneTemplate)]
#[template(path = "tests/responsive_template.html")]
struct ResponsiveModel {
    resizes: Vec<(f32, f32)>,
}

impl Default for ResponsiveModel {
    fn default() -> Self {
        Self { resizes: Vec::new() }
    }
}

impl Model for ResponsiveModel {
    type Message = RespMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
    fn on_resize(&mut self, width: f32, height: f32, _context: &mut Context) {
        self.resizes.push((width, height));
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

fn find_node(ui: &Ui, probe: &str) -> taffy::NodeId {
    for (id, msg) in ui.interactions.iter() {
        if msg == probe {
            return id;
        }
    }
    panic!("probe '{}' not found in interactions", probe);
}

fn bg_color(ui: &Ui, node: taffy::NodeId) -> Option<Color> {
    match ui.render_data.get(&node) {
        Some(RenderData::Container(style)) => style.background_color,
        other => panic!("expected container render data, got {:?}", other.map(|_| ())),
    }
}

fn layout_width(ui: &Ui, node: taffy::NodeId) -> f32 {
    ui.taffy.layout(node).unwrap().size.width
}

const RED: Color = Color { r: 255, g: 0, b: 0, a: 255 };
const GREEN: Color = Color { r: 0, g: 255, b: 0, a: 255 };
const BLUE: Color = Color { r: 0, g: 0, b: 255, a: 255 };

// ---------------------------------------------------------------------------
// Dynamic (manual TemplateLayout with a runtime stylesheet string) path
// ---------------------------------------------------------------------------

struct DynResponsiveModel;

impl Model for DynResponsiveModel {
    type Message = RespMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

impl TemplateLayout for DynResponsiveModel {
    fn stylesheet(&self) -> &'static str {
        r#"
        .dyn { background-color: #ff0000; height: 30px; }
        @media (min-width: 500px) { .dyn { background-color: #00ff00; } }
        .vw-dyn { width: 25vw; height: 10px; }
        "#
    }
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _measurer: &impl TextMeasurer,
        _default_style: &ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        let root = builder.create_element("div", &[("style", "width: 100%; height: 100%")]);
        let a = builder.create_element("div", &[("class", "dyn"), ("data-on-click", "dyn_media")]);
        let b = builder.create_element("div", &[("class", "vw-dyn"), ("data-on-click", "dyn_vw")]);
        builder.append_child(root, a);
        builder.append_child(root, b);
        root
    }
}

// ---------------------------------------------------------------------------
// One sequential flow covering both paths
// ---------------------------------------------------------------------------

#[test]
fn responsive_flow() {
    // ----- compiled template path -----
    let mut runtime = Runtime::new(ResponsiveModel::default(), MockMeasurer);

    // Small portrait viewport: media rules must not apply, 50vw = 150px.
    assert!(runtime.set_size(300.0, 800.0));
    assert_eq!(runtime.model().resizes, vec![(300.0, 800.0)]);
    assert_eq!(screen::breakpoint(), screen::Breakpoint::Mobile);

    let vw_node = find_node(&runtime.ui, "probe_vw");
    let media_node = find_node(&runtime.ui, "probe_media");
    let land_node = find_node(&runtime.ui, "probe_land");

    assert!((layout_width(&runtime.ui, vw_node) - 150.0).abs() < 0.01,
        "50vw of 300px should be 150px");
    assert_eq!(bg_color(&runtime.ui, media_node), Some(RED),
        "@media (min-width: 400px) must not apply at 300px");
    assert_eq!(bg_color(&runtime.ui, land_node), None,
        "@media (orientation: landscape) must not apply in portrait");

    // Large landscape viewport: media rules apply, 50vw = 400px.
    assert!(runtime.set_size(800.0, 600.0));
    assert_eq!(runtime.model().resizes, vec![(300.0, 800.0), (800.0, 600.0)]);
    assert_eq!(screen::breakpoint(), screen::Breakpoint::Tablet);

    let vw_node = find_node(&runtime.ui, "probe_vw");
    let media_node = find_node(&runtime.ui, "probe_media");
    let land_node = find_node(&runtime.ui, "probe_land");

    assert!((layout_width(&runtime.ui, vw_node) - 400.0).abs() < 0.01,
        "50vw of 800px should be 400px");
    assert_eq!(bg_color(&runtime.ui, media_node), Some(GREEN),
        "@media (min-width: 400px) must apply at 800px");
    assert_eq!(bg_color(&runtime.ui, land_node), Some(BLUE),
        "@media (orientation: landscape) must apply when width > height");

    // Same size again: no resize notification, no rebuild.
    assert!(!runtime.set_size(800.0, 600.0));
    assert_eq!(runtime.model().resizes.len(), 2);

    // ----- dynamic stylesheet path -----
    let mut dyn_runtime = Runtime::new(DynResponsiveModel, MockMeasurer);

    dyn_runtime.set_size(300.0, 300.0);
    let media_node = find_node(&dyn_runtime.ui, "dyn_media");
    let vw_node = find_node(&dyn_runtime.ui, "dyn_vw");
    assert_eq!(bg_color(&dyn_runtime.ui, media_node), Some(RED));
    assert!((layout_width(&dyn_runtime.ui, vw_node) - 75.0).abs() < 0.01);

    assert!(dyn_runtime.set_size(600.0, 400.0));
    let media_node = find_node(&dyn_runtime.ui, "dyn_media");
    let vw_node = find_node(&dyn_runtime.ui, "dyn_vw");
    assert_eq!(bg_color(&dyn_runtime.ui, media_node), Some(GREEN),
        "runtime stylesheet @media must re-resolve on resize");
    assert!((layout_width(&dyn_runtime.ui, vw_node) - 150.0).abs() < 0.01);

    // ----- successive runtimes with identical initial sizes do not leak global viewport -----
    let mut rt_first = Runtime::new(ResponsiveModel::default(), MockMeasurer);
    assert!(rt_first.set_size(1024.0, 768.0));
    assert_eq!(rt_first.model().resizes, vec![(1024.0, 768.0)]);

    let mut rt_second = Runtime::new(ResponsiveModel::default(), MockMeasurer);
    assert!(rt_second.set_size(1024.0, 768.0), "Second runtime must not skip on_resize when matching previous global viewport");
    assert_eq!(rt_second.model().resizes, vec![(1024.0, 768.0)]);
}
