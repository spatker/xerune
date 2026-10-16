//! CSS keyframe animations, driven by a simulated clock (`tick_at_ms`).
//! Expected values follow CSS Animations 1: each property interpolates
//! between the nearest keyframes that set it, and the timing function
//! applies per keyframe segment.

use std::collections::HashMap;
use xerune::*;

struct Measurer;
impl TextMeasurer for Measurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Insert,
    Touch,
}

/// The 10px-tall boxes drawn, top to bottom: their x and background color.
fn boxes<M: Model + ui::TemplateLayout>(runtime: &Runtime<M, Measurer>) -> Vec<(f32, (u8, u8, u8))> {
    let mut boxes: Vec<_> = runtime.ui.build_commands(&HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.height == 10.0 => Some((rect.y, rect.x, (c.r, c.g, c.b))),
        _ => None,
    }).collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    boxes.into_iter().map(|(_, x, c)| (x, c)).collect()
}

/// A runtime laid out at 400x300, with its clock started at 0 ms.
fn start<M: Model + ui::TemplateLayout>(model: M) -> Runtime<M, Measurer> {
    let mut runtime = Runtime::new(model, Measurer);
    runtime.set_size(400.0, 300.0);
    runtime.tick_at_ms(0);
    runtime
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.6
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  @keyframes fade { from { background-color: #000000; } to { background-color: #ff0000; } }
  @keyframes slide { 0% { left: 0px; } 50% { color: #ffffff; } 100% { left: 100px; } }
  @keyframes eased { 0% { left: 0px; } 50% { left: 50px; } 100% { left: 100px; } }
  @keyframes stepped { 0% { left: 0px; animation-timing-function: steps(4, end); } 100% { left: 100px; } }
  div { position: relative; width: 10px; height: 10px; background-color: #0000ff; }
  .fade { animation: fade 1s linear; }
  .slide { animation: slide 1s linear; }
  .eased { animation: eased 1s ease-in; }
  .stepped { animation: stepped 1s linear; }
</style>
<div class="fade"></div><div class="slide"></div><div class="eased"></div><div class="stepped"></div>"#)]
struct Keyframes;

impl Model for Keyframes {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context) {}
}

#[test]
fn colors_interpolate_over_the_duration() {
    let mut runtime = start(Keyframes);
    runtime.tick_at_ms(500);
    let (_, (r, g, b)) = boxes(&runtime)[0];
    assert!((127..=128).contains(&r) && g == 0 && b == 0, "{r} {g} {b}");
}

/// `left` is set at 0% and 100% only; the 50% keyframe (color) doesn't
/// stop it halfway.
#[test]
fn each_property_uses_the_keyframes_that_set_it() {
    let mut runtime = start(Keyframes);
    runtime.tick_at_ms(250);
    let (x, _) = boxes(&runtime)[1];
    assert!(close(x, 25.0), "left at 25% is {x}");
}

/// `ease-in` applies to each segment (0..50px, 50..100px), not to the
/// whole animation: a quarter of the way is halfway through the first
/// segment, ease-in(0.5) of it.
#[test]
fn the_timing_function_applies_per_segment() {
    let mut runtime = start(Keyframes);
    runtime.tick_at_ms(250);
    let (x, _) = boxes(&runtime)[2];
    let ease_in_half = 0.3153;
    assert!(close(x, 50.0 * ease_in_half), "left at 25% is {x}");
}

/// A keyframe's own `animation-timing-function` drives the segment after
/// it.
#[test]
fn keyframes_can_set_their_timing_function() {
    let mut runtime = start(Keyframes);
    runtime.tick_at_ms(300);
    let (x, _) = boxes(&runtime)[3];
    assert!(close(x, 25.0), "steps(4) at 30% is {x}");
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  @keyframes fade { from { background-color: #000000; } to { background-color: #ff0000; } }
  div { width: 10px; height: 10px; background-color: #0000ff; animation: fade 1s linear; }
</style>
<main>{% for item in items %}<div data-key="{{ item }}"></div>{% endfor %}</main>"#)]
struct List {
    items: Vec<&'static str>,
}

impl Model for List {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context) {
        if msg == Msg::Insert {
            self.items.insert(0, "new");
        }
    }
}

/// A rebuild in the middle of an animation shows the animated style right
/// away, not the element's own style until the next tick.
#[test]
fn a_rebuild_keeps_running_animations_applied() {
    let mut runtime = start(List { items: vec!["a"] });
    runtime.tick_at_ms(500);
    runtime.handle_event(InputEvent::Message("touch".into()));
    let (_, (r, _, b)) = boxes(&runtime)[0];
    assert!((127..=128).contains(&r) && b == 0, "after the rebuild: {r} {b}");
}

/// Animations belong to elements, not to their position in the tree: an
/// item inserted before a running one starts its own animation, and the
/// running one goes on.
#[test]
fn animations_follow_their_element() {
    let mut runtime = start(List { items: vec!["a"] });
    runtime.tick_at_ms(500);
    runtime.handle_event(InputEvent::Message("insert".into()));
    runtime.tick_at_ms(750);
    let colors: Vec<_> = boxes(&runtime).into_iter().map(|(_, c)| c.0).collect();
    assert_eq!(colors.len(), 2);
    assert!((63..=64).contains(&colors[0]), "the new item is 250 ms in: {colors:?}");
    assert!((191..=192).contains(&colors[1]), "the old item is 750 ms in: {colors:?}");
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  @keyframes fade { from { background-color: #000000; } to { background-color: #ff0000; } }
  main { display: flex; }
  .knob::before { content: ""; display: block; width: 10px; height: 10px; background-color: #0000ff; animation: fade 1s linear; }
</style>
<main><div class="knob"></div></main>"#)]
struct Knob;

impl Model for Knob {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context) {}
}

/// `::before` boxes have keys of their own (from their element's), so
/// their animations go on through rebuilds too.
#[test]
fn generated_boxes_keep_their_animations() {
    let mut runtime = start(Knob);
    runtime.tick_at_ms(500);
    runtime.handle_event(InputEvent::Message("touch".into()));
    runtime.tick_at_ms(750);
    let (_, (r, _, b)) = boxes(&runtime)[0];
    assert!((191..=192).contains(&r) && b == 0, "750 ms in: {r} {b}");
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  @keyframes dots { from { background-color: #000000; } to { background-color: #ff0000; } }
  div { width: 10px; height: 10px; }
  .a { animation: dots 1s cubic-bezier(0, 0, 1, 1); }
  .b { animation: 1s steps(2, start) dots; }
</style>
<main><div class="a"></div><div class="b"></div></main>"#)]
struct Shorthand;

impl Model for Shorthand {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context) {}
}

/// In the `animation` shorthand, a name ending in "s" is a name, and a
/// timing function with spaces between its arguments is one value.
#[test]
fn the_shorthand_reads_names_and_functions() {
    let mut runtime = start(Shorthand);
    runtime.tick_at_ms(250);
    let reds: Vec<u8> = boxes(&runtime).into_iter().map(|(_, c)| c.0).collect();
    assert!((63..=64).contains(&reds[0]) && (127..=128).contains(&reds[1]), "{reds:?}");
}
