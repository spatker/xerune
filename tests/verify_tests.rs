//! `Runtime::set_verify` (`XERUNE_VERIFY=1`): the runtime's shortcuts are
//! checked against the long way round, and planted mistakes are found.

use xerune::*;
use xerune_render::{Fonts, SoftwareRenderer};

const W: u32 = 200;
const H: u32 = 120;

fn fonts() -> Fonts {
    Fonts::ttf(include_bytes!("../resources/fonts/Roboto-Regular.ttf"), None).unwrap()
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Count,
    Rename,
    Toggle,
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
    body { margin: 0; }
    .box { width: 40px; height: 20px; background-color: #2563eb; }
    .wide { width: 120px; }
</style>
<body>
    <p>{{ count }} {{ name }}</p>
    {% if wide %}<div class="box wide"></div>{% else %}<div class="box"></div>{% endif %}
</body>"#)]
struct App {
    count: u32,
    name: String,
    wide: bool,
    /// Leaves `name` out of the fingerprint, as a model might by mistake.
    fingerprint_misses_name: bool,
}

impl App {
    fn new(fingerprint_misses_name: bool) -> Self {
        Self { count: 0, name: "a".into(), wide: false, fingerprint_misses_name }
    }
}

impl Model for App {
    type Message = Msg;

    fn update(&mut self, msg: Msg, _context: &mut Context<Msg>) {
        match msg {
            Msg::Count => self.count += 1,
            Msg::Rename => self.name.push('b'),
            Msg::Toggle => self.wide = !self.wide,
        }
    }

    fn view_fingerprint(&self) -> Option<u64> {
        let name = if self.fingerprint_misses_name { "" } else { self.name.as_str() };
        Some(model::hash_bytes(format!("{} {name} {}", self.count, self.wide).as_bytes()))
    }
}

/// Draws only full repaints: the damaged areas of a frame are left as they
/// were, as a renderer that clipped wrongly would.
struct IgnoresPartialRepaints(SoftwareRenderer);

impl Renderer for IgnoresPartialRepaints {
    fn render(&mut self, target: &mut Surface<'_>, commands: &[DrawCommand], resources: &RenderResources<'_>, area: Rect) {
        if area.width >= W as f32 && area.height >= H as f32 {
            self.0.render(target, commands, resources, area);
        }
    }
}

fn run(model: App, renderer: &mut impl Renderer, messages: &[&str]) -> usize {
    let mut runtime = Runtime::new(model, fonts());
    runtime.set_verify(true);
    runtime.set_size(W as f32, H as f32);
    let mut buffer = vec![0u32; (W * H) as usize];
    for message in std::iter::once(None).chain(messages.iter().map(Some)) {
        if let Some(message) = message {
            runtime.handle_messages([message.to_string()]);
        }
        runtime.render(renderer, &mut Surface::new(Pixels::Xrgb8888(&mut buffer), W, H));
    }
    runtime.verify_failures()
}

const MESSAGES: &[&str] = &["count", "toggle", "rename", "count", "toggle", "rename"];

#[test]
fn a_correct_app_passes() {
    assert_eq!(run(App::new(false), &mut SoftwareRenderer::new(fonts()), MESSAGES), 0);
}

#[test]
fn a_fingerprint_that_misses_state_is_found() {
    // `rename` is skipped as unchanged twice, and shows a stale name.
    assert_eq!(run(App::new(true), &mut SoftwareRenderer::new(fonts()), MESSAGES), 2);
}

#[test]
fn damage_that_is_not_repainted_is_found() {
    let failures = run(App::new(false), &mut IgnoresPartialRepaints(SoftwareRenderer::new(fonts())), MESSAGES);
    assert!(failures > 0);
}
