//! CSS transitions, driven by a simulated clock (`ManualClock`). Expected
//! values follow CSS Transitions 1: a changed property moves from what was
//! on screen to its new value, a transition sent back where it came from
//! takes as long as the way back is, and elements that appear don't
//! transition.

use std::collections::HashMap;
use std::time::Duration;
use xerune::runtime::{ManualClock, Wake};
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
    Touch,
    Show,
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

/// A runtime on a clock that only moves when the test says so.
struct Sim<M: Model> {
    runtime: Runtime<M, Measurer>,
    clock: ManualClock,
}

impl<M: Model + ui::TemplateLayout> Sim<M> {
    fn new(model: M) -> Self {
        let mut runtime = Runtime::new(model, Measurer);
        let clock = ManualClock::new();
        runtime.set_clock(clock.clone());
        // A frame at every millisecond, so each `at` samples one.
        runtime.set_target_fps(1000);
        runtime.set_size(400.0, 300.0);
        let mut sim = Sim { runtime, clock };
        sim.at(0);
        sim
    }

    /// Moves the clock to `ms` after the start and polls.
    fn at(&mut self, ms: u64) {
        self.clock.advance(Duration::from_millis(ms).saturating_sub(self.clock.elapsed()));
        self.runtime.poll();
    }

    fn send(&mut self, message: &str) {
        self.runtime.handle_messages([message.to_string()]);
    }

    fn x(&self, i: usize) -> f32 {
        boxes(&self.runtime)[i].0
    }

    fn red(&self, i: usize) -> u8 {
        boxes(&self.runtime)[i].1 .0
    }
}

/// Within half a pixel: layout places boxes at whole pixels.
fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.6
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { position: relative; left: 0px; width: 10px; height: 10px; background-color: #000000;
        transition: left 1s linear, background-color 1s linear 500ms; }
  .on { left: 100px; background-color: #ff0000; }
</style>
<main><div class="{% if on %}on{% endif %}"></div></main>"#)]
struct Toggle {
    on: bool,
}

impl Model for Toggle {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context<Self::Message>) {
        if msg == Msg::Toggle {
            self.on = !self.on;
        }
    }
}

#[test]
fn a_changed_property_moves_to_its_new_value() {
    let mut sim = Sim::new(Toggle { on: false });
    sim.send("toggle");
    assert!(close(sim.x(0), 0.0), "starts from what was shown: {}", sim.x(0));
    sim.at(250);
    assert!(close(sim.x(0), 25.0), "{}", sim.x(0));
    sim.at(1000);
    assert!(close(sim.x(0), 100.0), "{}", sim.x(0));
}

#[test]
fn a_transition_waits_out_its_delay() {
    let mut sim = Sim::new(Toggle { on: false });
    sim.send("toggle");
    sim.at(400);
    assert_eq!(sim.red(0), 0, "still in its 500 ms delay");
    sim.at(1000);
    assert!((127..=128).contains(&sim.red(0)), "{}", sim.red(0));
    sim.at(1500);
    assert_eq!(sim.red(0), 255);
}

#[test]
fn frames_run_until_the_transitions_end() {
    let mut sim = Sim::new(Toggle { on: false });
    sim.send("toggle");
    assert!(matches!(sim.runtime.poll().wake, Wake::Frame(_)));
    sim.at(1500);
    assert_eq!(sim.runtime.poll().wake, Wake::Idle);
}

/// Sent back halfway, a transition returns from where it is, in the time
/// it took to get there (the reversing shortening factor is 0.5).
#[test]
fn a_reversed_transition_takes_as_long_as_the_way_back() {
    let mut sim = Sim::new(Toggle { on: false });
    sim.send("toggle");
    sim.at(500);
    sim.send("toggle");
    assert!(close(sim.x(0), 50.0), "{}", sim.x(0));
    sim.at(750);
    assert!(close(sim.x(0), 25.0), "half of the half-second way back: {}", sim.x(0));
    sim.at(1000);
    assert!(close(sim.x(0), 0.0), "{}", sim.x(0));
}

/// Rebuilds for other reasons don't restart a transition, and the rebuilt
/// view shows its value right away.
#[test]
fn a_rebuild_keeps_a_transition_going() {
    let mut sim = Sim::new(Toggle { on: false });
    sim.send("toggle");
    sim.at(300);
    sim.send("touch");
    assert!(close(sim.x(0), 30.0), "{}", sim.x(0));
    sim.at(600);
    assert!(close(sim.x(0), 60.0), "{}", sim.x(0));
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { position: relative; width: 10px; height: 10px; background-color: #000000; transition: background-color 1s linear; }
  .new { background-color: #ff0000; }
</style>
<main><div></div>{% if shown %}<div class="new"></div>{% endif %}</main>"#)]
struct Appear {
    shown: bool,
}

impl Model for Appear {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context<Self::Message>) {
        if msg == Msg::Show {
            self.shown = true;
        }
    }
}

#[test]
fn an_element_that_appears_does_not_transition() {
    let mut sim = Sim::new(Appear { shown: false });
    sim.send("show");
    assert_eq!(sim.red(1), 255);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { width: 10px; height: 10px; background-color: #000000; }
  .plain { transition: background 1s linear; }
  .discrete { transition: background 1s linear allow-discrete; }
  .on { background: linear-gradient(90deg, #ff0000 0%, #ff0000 100%); }
</style>
<main><div class="plain {% if on %}on{% endif %}"></div><div class="discrete {% if on %}on{% endif %}"></div></main>"#)]
struct Discrete {
    on: bool,
}

impl Model for Discrete {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context<Self::Message>) {
        if msg == Msg::Toggle {
            self.on = !self.on;
        }
    }
}

/// A color can't turn into a gradient gradually: it changes at once, or
/// halfway with `allow-discrete`.
#[test]
fn values_that_cannot_interpolate_change_at_once() {
    let mut sim = Sim::new(Discrete { on: false });
    sim.send("toggle");
    let gradient = |sim: &Sim<Discrete>, i: usize| {
        sim.runtime.ui.build_commands(&HashMap::new(), None).iter().filter(|c| matches!(c, DrawCommand::DrawRect { gradient: Some(_), .. })).count() > i
    };
    assert!(gradient(&sim, 0), "the plain one has its gradient right away");
    assert!(!gradient(&sim, 1), "the discrete one keeps its color until halfway");
    sim.at(600);
    assert!(gradient(&sim, 1));
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { width: 10px; height: 10px; background-color: #000000; transition: background-color 1s linear; }
  div:hover { background-color: #ff0000; }
</style>
<main><div></div></main>"#)]
struct Hover;

impl Model for Hover {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

#[test]
fn hover_fades_in_and_out() {
    let mut sim = Sim::new(Hover);
    sim.runtime.handle_event(InputEvent::Hover { x: 5.0, y: 5.0 });
    sim.at(500);
    assert!((127..=128).contains(&sim.red(0)), "{}", sim.red(0));
    sim.runtime.handle_event(InputEvent::Hover { x: 200.0, y: 200.0 });
    sim.at(750);
    assert!((63..=64).contains(&sim.red(0)), "fading back from halfway: {}", sim.red(0));
}


#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { position: relative; width: 10px; height: 10px; background-color: #ff0000; transition: background-color 1s linear; }
  @starting-style {
    .fades { background-color: #000000; }
  }
</style>
<main><div class="fades"></div>{% if shown %}<div class="fades"></div><div></div>{% endif %}</main>"#)]
struct Starting {
    shown: bool,
}

impl Model for Starting {
    type Message = Msg;
    fn update(&mut self, msg: Msg, _context: &mut Context<Self::Message>) {
        if msg == Msg::Show {
            self.shown = true;
        }
    }
}

/// An element that appears starts from its `@starting-style` style, also
/// when the app starts; without one, it doesn't transition.
#[test]
fn starting_style_transitions_elements_that_appear() {
    let mut sim = Sim::new(Starting { shown: false });
    sim.at(500);
    assert!((127..=128).contains(&sim.red(0)), "fading in since the start: {}", sim.red(0));
    sim.send("show");
    sim.at(1000);
    assert_eq!(sim.red(0), 255);
    assert!((127..=128).contains(&sim.red(1)), "the new one fades in: {}", sim.red(1));
    assert_eq!(sim.red(2), 255, "no @starting-style: no transition");
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum EventMsg {
    Toggle,
    Run,
    Start,
    End,
    Cancel,
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  div { position: relative; left: 0px; width: 10px; height: 10px; background-color: #000000; transition: left 1s linear 200ms; }
  .on { left: 100px; }
</style>
<main><div class="{% if on %}on{% endif %}" data-on-transitionrun="run" data-on-transitionstart="start"
  data-on-transitionend="end" data-on-transitioncancel="cancel"></div></main>"#)]
struct Events {
    on: bool,
    log: Vec<EventMsg>,
}

impl Model for Events {
    type Message = EventMsg;
    fn update(&mut self, msg: EventMsg, _context: &mut Context<Self::Message>) {
        match msg {
            EventMsg::Toggle => self.on = !self.on,
            other => self.log.push(other),
        }
    }
}

#[test]
fn transitions_send_run_start_end_and_cancel() {
    let mut sim = Sim::new(Events { on: false, log: Vec::new() });
    sim.send("toggle");
    sim.at(100);
    assert_eq!(sim.runtime.model().log, [EventMsg::Run], "run at once, start after the delay");
    sim.at(200);
    assert_eq!(sim.runtime.model().log, [EventMsg::Run, EventMsg::Start]);
    sim.at(1200);
    assert_eq!(sim.runtime.model().log, [EventMsg::Run, EventMsg::Start, EventMsg::End]);

    // Sent back partway: the running one is cancelled, a new one runs.
    sim.send("toggle");
    sim.at(1700);
    sim.send("toggle");
    sim.at(1701);
    let log = &sim.runtime.model().log[3..];
    assert_eq!(log, [EventMsg::Run, EventMsg::Start, EventMsg::Cancel, EventMsg::Run]);
}
