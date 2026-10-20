//! The runtime HTML path (the C API's: a template interpreted into HTML,
//! parsed and built with `ui::html::build_document`) draws what the
//! compiled template draws, pixel for pixel. Both run side by side on one
//! clock through the same messages and frames, so transitions in flight
//! are compared too, and with them the elements' keys. With
//! `XERUNE_DIFF_DIR` set, a failing comparison writes both frames there.

use std::path::Path;

use html5ever::tendril::TendrilSink;
use serde::Serialize;
use xerune::__private::{TemplateLayout, TemplateMethods};
use xerune::runtime::ManualClock;
use xerune::*;

#[path = "../examples/calculator/calculator_impl.rs"]
#[allow(dead_code)]
mod calculator_impl;
#[path = "../examples/music_player/music_player_impl.rs"]
#[allow(dead_code)]
mod music_player_impl;
#[path = "../examples/showcase/showcase_impl.rs"]
#[allow(dead_code)]
mod showcase_impl;
#[path = "../examples/support/mod.rs"]
mod support;
#[path = "../examples/todo/todo_impl.rs"]
#[allow(dead_code)]
mod todo_impl;

/// The interpreter's HTML for the compiled model, as a page.
struct Html {
    dom: markup5ever_rcdom::RcDom,
    html: String,
    css: &'static str,
    generation: u64,
}

impl Model for Html {
    type Message = String;
    fn update(&mut self, _: String, _: &mut Context<String>) {}
    fn view_fingerprint(&self) -> Option<u64> {
        Some(self.generation)
    }
}

impl TemplateLayout for Html {
    fn stylesheet(&self) -> &'static str {
        self.css
    }
    fn build_ui(&self, builder: &mut ui::UiBuilder, _: &impl TextMeasurer, _: &style::ContainerStyle, _: &impl Fn(&str) -> bool) -> taffy::NodeId {
        ui::html::build_document(&self.dom.document, builder).unwrap_or_else(|| builder.create_element("html", &[]))
    }
}

impl Html {
    /// Shows `html`; true if it changed.
    fn set(&mut self, html: String) -> bool {
        if html == self.html {
            return false;
        }
        self.dom = html5ever::parse_document(markup5ever_rcdom::RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()).unwrap();
        self.html = html;
        self.generation += 1;
        true
    }
}

fn parse_template(path: &str) -> xerune_template::Template {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let file = xerune_template::File { name: path.display().to_string(), dir: path.parent().unwrap().to_path_buf(), content: std::fs::read_to_string(&path).unwrap() };
    xerune_template::parse(file, &mut |include: &str, dir: &Path| {
        let content = std::fs::read_to_string(dir.join(include)).map_err(|e| e.to_string())?;
        Ok(xerune_template::File { name: include.to_string(), dir: dir.to_path_buf(), content })
    })
    .unwrap()
}

struct Screen {
    pixels: Vec<u32>,
    width: u32,
    height: u32,
}

impl Screen {
    fn new(width: u32, height: u32) -> Self {
        Self { pixels: vec![0xffff_ffff; (width * height) as usize], width, height }
    }

    fn render<M: Model + TemplateLayout>(&mut self, runtime: &mut Runtime<M, xerune_render::Fonts>, renderer: &mut xerune_render::SoftwareRenderer) {
        let mut target = Surface::new(Pixels::Xrgb8888(&mut self.pixels), self.width, self.height);
        runtime.render(renderer, &mut target);
    }

    fn save(&self, path: &Path) {
        let mut pixmap = tiny_skia::Pixmap::new(self.width, self.height).unwrap();
        for (pixel, bytes) in self.pixels.iter().zip(pixmap.data_mut().chunks_exact_mut(4)) {
            bytes.copy_from_slice(&[(pixel >> 16) as u8, (pixel >> 8) as u8, *pixel as u8, 255]);
        }
        pixmap.save_png(path).unwrap();
    }

    /// The pixels that differ from `other`'s, and where the first is.
    fn diff(&self, other: &Screen) -> (usize, Option<(u32, u32)>) {
        let differ: Vec<usize> = (0..self.pixels.len()).filter(|&i| self.pixels[i] & 0xff_ffff != other.pixels[i] & 0xff_ffff).collect();
        (differ.len(), differ.first().map(|&i| (i as u32 % self.width, i as u32 / self.width)))
    }
}

/// Runs `model` through `messages` at both sizes, 50 ms frames between
/// them, comparing every frame of the compiled view with the interpreted.
fn check<M>(template: &str, model: impl Fn() -> M, messages: &[&str])
where
    M: Model + TemplateLayout + TemplateMethods + Serialize,
{
    // FIXME: the viewport is process-wide (`xerune::screen`), so tests at
    // other sizes running at the same time change `@media` and `vh` under
    // these; one at a time until each runtime has its own.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let fonts = support::setup();
    let tpl = parse_template(template);
    let css: &'static str = Box::leak(tpl.css.clone().into_boxed_str());
    for (width, height) in [(800, 480), (480, 800)] {
        let clock = ManualClock::new();
        let mut compiled = Runtime::new(model(), fonts.clone());
        let mut interpreted = Runtime::new(Html { dom: Default::default(), html: String::new(), css, generation: 0 }, fonts.clone());
        compiled.set_clock(clock.clone());
        interpreted.set_clock(clock.clone());
        let (mut a, mut b) = (Screen::new(width, height), Screen::new(width, height));
        let (mut renderer_a, mut renderer_b) = (xerune_render::SoftwareRenderer::new(fonts.clone()), xerune_render::SoftwareRenderer::new(fonts.clone()));
        let sync = |compiled: &Runtime<M, _>, interpreted: &mut Runtime<Html, _>| {
            let value = xerune_template::to_value(compiled.model()).unwrap();
            let html = xerune_template::render_html(&tpl, &value, compiled.model()).unwrap();
            if interpreted.model_mut().set(html) {
                interpreted.sync_view();
            }
        };
        sync(&compiled, &mut interpreted);
        compiled.set_size(width as f32, height as f32);
        interpreted.set_size(width as f32, height as f32);
        compiled.wait_for_images(std::time::Duration::from_secs(5));
        interpreted.wait_for_images(std::time::Duration::from_secs(5));
        for step in std::iter::once("").chain(messages.iter().copied()) {
            if !step.is_empty() {
                compiled.handle_event(InputEvent::Message(step.to_string()));
                sync(&compiled, &mut interpreted);
            }
            for frame in 0..10 {
                compiled.poll();
                sync(&compiled, &mut interpreted);
                interpreted.poll();
                a.render(&mut compiled, &mut renderer_a);
                b.render(&mut interpreted, &mut renderer_b);
                let (count, first) = a.diff(&b);
                if count > 0 {
                    if let Some(dir) = std::env::var_os("XERUNE_DIFF_DIR") {
                        let name = Path::new(template).file_stem().unwrap().to_string_lossy().into_owned();
                        a.save(&Path::new(&dir).join(format!("{name}_{width}x{height}_compiled.png")));
                        b.save(&Path::new(&dir).join(format!("{name}_{width}x{height}_interpreted.png")));
                    }
                }
                assert_eq!(count, 0, "{template} {width}x{height} after `{step}`, frame {frame}: {count} pixels differ, the first at {first:?}");
                clock.advance(std::time::Duration::from_millis(50));
            }
        }
    }
}

#[test]
fn todo() {
    check("examples/todo/templates/todo_list.html", todo_impl::TodoList::default, &["toggle:0", "toggle:1", "remove:0", "add"]);
}

#[test]
fn calculator() {
    check("examples/calculator/templates/calculator.html", calculator_impl::CalculatorModel::default, &["digit:7", "op:*", "digit:3", "equals"]);
}

#[test]
fn music_player() {
    let tracks = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/music_player/resources/music.json")).unwrap();
    check("examples/music_player/templates/music_player.html", || music_player_impl::MusicPlayerModel::new(Some(&tracks)), &["select_track:1", "play_pause", "back"]);
}

/// Not the monitor tab (`select_tab:3`): its charts are canvases the
/// Rust model draws, which interpreted HTML has no model to draw.
#[test]
fn showcase() {
    check("examples/showcase/templates/showcase.html", showcase_impl::ShowcaseModel::default, &["select_tab:1", "select_tab:2", "select_tab:4", "toggle_bluetooth"]);
}
