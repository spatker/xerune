//! `opacity`: it fades everything an element paints, nests by multiplying,
//! makes the element a stacking context, and transitions.

use std::collections::HashMap;
use std::time::Duration;
use xerune::runtime::ManualClock;
use xerune::*;

struct Measurer;
impl TextMeasurer for Measurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Toggle,
}

fn commands<M: Model + ui::TemplateLayout>(model: M) -> Vec<DrawCommand> {
    let mut runtime = Runtime::new(model, Measurer);
    runtime.set_size(400.0, 300.0);
    runtime.ui.build_commands(&HashMap::new(), None)
}

/// The alpha of each 10px-wide rect's fill, by its color's red channel.
fn rect_alphas(commands: &[DrawCommand]) -> Vec<(u8, u8)> {
    commands
        .iter()
        .filter_map(|c| match c {
            DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some((c.r, c.a)),
            _ => None,
        })
        .collect()
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { width: 10px; height: 10px; }
  .a { background-color: #010000; opacity: 0.5; }
  .b { background-color: #020000; opacity: 50%; }
  .c { background-color: #030000; opacity: 2; }
  .d { background-color: #040000; opacity: -1; }
  .e { background-color: #050000; opacity: 0.5; }
  .e > div { background-color: #060000; opacity: 0.5; }
</style>
<main><div class="a"></div><div class="b"></div><div class="c"></div><div class="d"></div><div class="e"><div></div></div></main>"#)]
struct Values;

impl Model for Values {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

/// A number or a percentage, clamped to 0..1; nested opacities multiply,
/// and a fully transparent element draws nothing. (The opaque one paints
/// first: translucent ones are stacking contexts.)
#[test]
fn opacity_fades_and_nests() {
    assert_eq!(rect_alphas(&commands(Values)), [(3, 255), (1, 128), (2, 128), (5, 128), (6, 64)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  .card { opacity: 0.25; background-color: #ff0000; border: 2px solid #00ff00; width: 40px; height: 40px; }
  p { color: #0000ff; margin: 0; }
  img { width: 10px; height: 10px; }
</style>
<main><div class="card"><p>Hi</p><img src="cover.png"></div></main>"#)]
struct Parts;

impl Model for Parts {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

/// Backgrounds, borders, text and images all fade.
#[test]
fn every_part_fades() {
    let commands = commands(Parts);
    let rect = commands.iter().find_map(|c| match c {
        DrawCommand::DrawRect { color: Some(color), border, rect, .. } if rect.width == 44.0 => Some((color.a, border.top.color.map(|c| c.a))),
        _ => None,
    });
    assert_eq!(rect, Some((64, Some(64))));
    let text = commands.iter().find_map(|c| match c {
        DrawCommand::DrawText { color, .. } => Some(color.a),
        _ => None,
    });
    assert_eq!(text, Some(64));
    let image = commands.iter().find_map(|c| match c {
        DrawCommand::DrawImage { opacity, .. } => Some(*opacity),
        _ => None,
    });
    assert_eq!(image, Some(0.25));
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { width: 10px; height: 10px; }
  .faded { opacity: 0.5; background-color: #010000; }
  .later { background-color: #020000; margin-top: -10px; }
  .pinned { position: absolute; top: 50px; background-color: #030000; }
</style>
<main><div class="faded"><div class="pinned"></div></div><div class="later"></div></main>"#)]
struct Stacking;

impl Model for Stacking {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

/// A translucent element is a stacking context painted like a positioned
/// one: after the in-flow content that follows it, together with its
/// positioned descendants, which fade with it.
#[test]
fn a_translucent_element_is_a_stacking_context() {
    assert_eq!(rect_alphas(&commands(Stacking)), [(2, 255), (1, 128), (3, 128)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { width: 10px; height: 10px; background-color: #ff0000; opacity: 0; transition: opacity 1s linear; }
  .on { opacity: 1; }
</style>
<main><div class="{% if on %}on{% endif %}"></div></main>"#)]
struct Fade {
    on: bool,
}

impl Model for Fade {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context<Self::Message>) {
        if msg == Msg::Toggle {
            self.on = !self.on;
        }
    }
}

#[test]
fn opacity_transitions() {
    let mut runtime = Runtime::new(Fade { on: false }, Measurer);
    let clock = ManualClock::new();
    runtime.set_clock(clock.clone());
    runtime.set_target_fps(1000);
    runtime.set_size(400.0, 300.0);
    runtime.poll();
    let alpha = |runtime: &Runtime<Fade, Measurer>| rect_alphas(&runtime.ui.build_commands(&HashMap::new(), None)).first().map(|&(_, a)| a);
    assert_eq!(alpha(&runtime), None, "fully transparent: nothing drawn");
    runtime.handle_messages(["toggle".to_string()]);
    clock.advance(Duration::from_millis(500));
    runtime.poll();
    let half = alpha(&runtime).expect("fading in");
    assert!((127..=128).contains(&half), "half way: {half}");
    clock.advance(Duration::from_millis(600));
    runtime.poll();
    assert_eq!(alpha(&runtime), Some(255));
}

/// The renderer draws a faded image's pixels that much less opaque.
#[test]
fn faded_images_blend_with_what_is_under_them() {
    let mut buffer = vec![0xFF00_0000u32; 4];
    let white = [255u8; 16];
    xerune_render::blit_image_rgba(&mut buffer, 2, 2, 2, &Rect::new(0.0, 0.0, 2.0, 2.0), 0.0, 2, 2, &white, 0.5, false, None, 0);
    let grey = buffer[0] & 0xff;
    assert!((127..=128).contains(&grey), "half white over black: {:08x}", buffer[0]);
}
