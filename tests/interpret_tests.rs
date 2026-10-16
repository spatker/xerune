//! The studio's interpreter (`xerune_template::render_html`) against the
//! compiled templates: for each example in a few states, both show the same
//! text, in the same order. That covers expressions, number formatting,
//! conditions, loops, includes and the model's methods.

use std::path::Path;

use serde::Serialize;
use xerune::__private::{TemplateLayout, TemplateMethods};
use xerune::*;

#[path = "../examples/animation/animation_impl.rs"]
#[allow(dead_code)]
mod animation_impl;
#[path = "../examples/animation_css/animation_css_impl.rs"]
#[allow(dead_code)]
mod animation_css_impl;
#[path = "../examples/breakout/breakout_impl.rs"]
#[allow(dead_code)]
mod breakout_impl;
#[path = "../examples/calculator/calculator_impl.rs"]
#[allow(dead_code)]
mod calculator_impl;
#[path = "../examples/music_player/music_player_impl.rs"]
#[allow(dead_code)]
mod music_player_impl;
#[path = "../examples/showcase/showcase_impl.rs"]
#[allow(dead_code)]
mod showcase_impl;
#[path = "../examples/todo/todo_impl.rs"]
#[allow(dead_code)]
mod todo_impl;

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (text.len() as f32 * 8.0, 16.0)
    }
}

fn normalize(text: &str) -> String {
    let text = text.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The text runs of the compiled view, in document order.
fn native_texts<M: Model + TemplateLayout>(runtime: &Runtime<M, MockMeasurer>) -> Vec<String> {
    fn walk(ui: &xerune::Ui, node: taffy::NodeId, out: &mut Vec<String>) {
        if let Some(RenderKind::Text(text)) = ui.render_data.get(node).map(|d| &d.kind) {
            let text = normalize(text);
            if !text.is_empty() {
                out.push(text);
            }
        }
        for child in ui.taffy.children(node).unwrap_or_default() {
            walk(ui, child, out);
        }
    }
    let mut out = Vec::new();
    walk(&runtime.ui, runtime.ui.root, &mut out);
    out
}

/// The text runs of the interpreter's HTML, in document order.
fn html_texts(html: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut chars = html.chars();
    while let Some(c) = chars.next() {
        if c != '<' {
            text.push(c);
            continue;
        }
        let run = normalize(&text);
        if !run.is_empty() {
            out.push(run);
        }
        text.clear();
        let mut quote = None;
        for c in chars.by_ref() {
            match (quote, c) {
                (None, '>') => break,
                (None, '"') => quote = Some('"'),
                (Some(q), c) if c == q => quote = None,
                _ => {}
            }
        }
    }
    out
}

fn parse_template(path: &str) -> xerune_template::Template {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let file = xerune_template::File {
        name: path.display().to_string(),
        dir: path.parent().unwrap().to_path_buf(),
        content: std::fs::read_to_string(&path).unwrap(),
    };
    xerune_template::parse(file, &mut |include: &str, dir: &Path| {
        let content = std::fs::read_to_string(dir.join(include)).map_err(|e| e.to_string())?;
        Ok(xerune_template::File { name: include.to_string(), dir: dir.to_path_buf(), content })
    })
    .unwrap()
}

/// Runs `model` through `messages`, checking after each that the
/// interpreter shows what the compiled template shows.
fn check<M>(template: &str, model: M, messages: &[&str])
where
    M: Model + TemplateLayout + TemplateMethods + Serialize,
{
    let tpl = parse_template(template);
    let mut runtime = Runtime::new(model, MockMeasurer);
    runtime.set_size(800.0, 480.0);
    for step in std::iter::once("").chain(messages.iter().copied()) {
        if !step.is_empty() {
            runtime.handle_event(InputEvent::Message(step.to_string()));
        }
        let value = xerune_template::to_value(runtime.model()).unwrap();
        let html = xerune_template::render_html(&tpl, &value, runtime.model())
            .unwrap_or_else(|e| panic!("{template} after `{step}`: {e}"));
        assert_eq!(html_texts(&html), native_texts(&runtime), "{template} after `{step}`");
    }
}

#[test]
fn todo() {
    check("examples/todo/templates/todo_list.html", todo_impl::TodoList::default(), &["toggle:0", "toggle:1", "remove:0", "add"]);
}

#[test]
fn calculator() {
    check(
        "examples/calculator/templates/calculator.html",
        calculator_impl::CalculatorModel::default(),
        &["digit:7", "op:*", "digit:3", "equals", "op:-", "digit:9", "equals"],
    );
}

#[test]
fn music_player() {
    let tracks = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/music_player/resources/music.json")).unwrap();
    check(
        "examples/music_player/templates/music_player.html",
        music_player_impl::MusicPlayerModel::new(Some(&tracks)),
        &["select_track:1", "play_pause", "next", "back"],
    );
}

#[test]
fn showcase() {
    check("examples/showcase/templates/showcase.html", showcase_impl::ShowcaseModel::default(), &["select_tab:2", "select_tab:3", "select_tab:4", "toggle_bluetooth", "select_tab:1"]);
}

#[test]
fn breakout() {
    check("examples/breakout/templates/breakout.html", breakout_impl::BreakoutModel::default(), &["frame:0.5"]);
}

#[test]
fn animation() {
    check("examples/animation/templates/animation.html", animation_impl::AnimationModel::new(20), &[]);
}

#[test]
fn animation_css() {
    check("examples/animation_css/templates/animation_css.html", animation_css_impl::AnimationCssModel, &[]);
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum GrammarMsg {
    Nop,
}

#[derive(XeruneTemplate, Serialize)]
#[template(path = "tests/template_grammar.html")]
struct GrammarModel {
    on: bool,
    off: bool,
    title: String,
    cover: Option<String>,
    pairs: Vec<(String, u32)>,
    flags: Vec<bool>,
}

impl Model for GrammarModel {
    type Message = GrammarMsg;
    fn update(&mut self, _msg: GrammarMsg, _context: &mut Context<GrammarMsg>) {}
}

#[test]
fn template_grammar() {
    for (on, cover) in [(true, Some("art.png")), (false, None)] {
        let model = GrammarModel {
            on,
            off: !on,
            title: if on { "Title".into() } else { String::new() },
            cover: cover.map(str::to_string),
            pairs: vec![("a".into(), 1), ("b".into(), 20)],
            flags: vec![true, false, true],
        };
        check("tests/template_grammar.html", model, &[]);
    }
}
