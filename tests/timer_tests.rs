//! Time reaches a model only as its own messages: one-shot timers from
//! `Context::after`, and the intervals and frames of `Model::subscriptions`.
//! The runtime reads a clock it is given; here one that moves only when the
//! test says so.

use std::time::Duration;
use xerune::runtime::{Clock, ManualClock, Wake};
use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Arm,
    Disarm,
    Rearm,
    Fired,
    Start,
    Stop,
    Second,
    Frame(f32),
}

#[derive(XeruneTemplate)]
#[template(source = "<div>{{ fired }}</div>")]
struct TimerModel {
    fired: u32,
    seconds: u32,
    frames: Vec<f32>,
    running: bool,
    timer: Option<TimerId>,
}

impl TimerModel {
    fn new() -> Self {
        Self { fired: 0, seconds: 0, frames: Vec::new(), running: false, timer: None }
    }
}

impl Model for TimerModel {
    type Message = Msg;
    fn update(&mut self, msg: Msg, context: &mut Context<Msg>) {
        match msg {
            Msg::Arm => self.timer = Some(context.after(Duration::from_millis(1000), Msg::Fired)),
            Msg::Disarm => context.cancel(self.timer.take().unwrap()),
            Msg::Rearm => {
                context.cancel(self.timer.take().unwrap());
                self.timer = Some(context.after(Duration::from_millis(500), Msg::Fired));
            }
            Msg::Fired => self.fired += 1,
            Msg::Start => self.running = true,
            Msg::Stop => self.running = false,
            Msg::Second => self.seconds += 1,
            Msg::Frame(dt) => self.frames.push(dt),
        }
    }

    fn subscriptions(&self) -> Subscriptions<Msg> {
        let subscriptions = Subscriptions::none().every(Duration::from_secs(1), Msg::Second);
        if self.running { subscriptions.frames(Msg::Frame) } else { subscriptions }
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

fn start() -> (Runtime<TimerModel, MockMeasurer>, ManualClock) {
    let mut runtime = Runtime::new(TimerModel::new(), MockMeasurer);
    let clock = ManualClock::new();
    runtime.set_clock(clock.clone());
    runtime.set_size(100.0, 100.0);
    (runtime, clock)
}

fn send(runtime: &mut Runtime<TimerModel, MockMeasurer>, message: &str) {
    runtime.handle_messages([message.to_string()]);
}

#[test]
fn a_timer_fires_once_after_its_delay() {
    let (mut runtime, clock) = start();
    clock.advance(Duration::from_millis(300));
    send(&mut runtime, "arm");
    clock.advance(Duration::from_millis(900));
    runtime.poll();
    assert_eq!(runtime.model().fired, 0, "fired before its 1000 ms delay");
    clock.advance(Duration::from_millis(100));
    runtime.poll();
    assert_eq!(runtime.model().fired, 1);
    clock.advance(Duration::from_secs(5));
    runtime.poll();
    assert_eq!(runtime.model().fired, 1, "a timer fires once");
}

#[test]
fn a_cancelled_timer_does_not_fire() {
    let (mut runtime, clock) = start();
    send(&mut runtime, "arm");
    send(&mut runtime, "disarm");
    clock.advance(Duration::from_secs(2));
    runtime.poll();
    assert_eq!(runtime.model().fired, 0);
}

/// Cancelling a timer and starting another in the same `update` keeps the
/// new one (timers used to be cleared by their message text, which removed
/// both).
#[test]
fn restarting_a_timer_in_one_update_keeps_the_new_one() {
    let (mut runtime, clock) = start();
    send(&mut runtime, "arm");
    send(&mut runtime, "rearm");
    clock.advance(Duration::from_millis(500));
    runtime.poll();
    assert_eq!(runtime.model().fired, 1);
}

#[test]
fn the_runtime_wakes_for_the_next_timer_or_interval() {
    let (mut runtime, clock) = start();
    let t0 = clock.now();
    assert_eq!(runtime.poll().wake, Wake::At(t0 + Duration::from_secs(1)), "the `every` second");
    clock.advance(Duration::from_millis(300));
    send(&mut runtime, "arm");
    assert_eq!(runtime.poll().wake, Wake::At(t0 + Duration::from_secs(1)));
    clock.advance(Duration::from_millis(700));
    assert_eq!(runtime.poll().wake, Wake::At(t0 + Duration::from_millis(1300)), "the timer");
    assert_eq!(runtime.model().seconds, 1);
}

#[test]
fn an_interval_keeps_its_phase_across_updates() {
    let (mut runtime, clock) = start();
    for _ in 0..5 {
        clock.advance(Duration::from_millis(400));
        // Updates in between ask for the subscriptions again.
        send(&mut runtime, "fired");
        runtime.poll();
    }
    assert_eq!(runtime.model().seconds, 2, "at 1000 and 2000 ms, not restarted by each update");
}

#[test]
fn frames_come_at_the_target_frame_rate_with_their_delta_time() {
    let (mut runtime, clock) = start();
    runtime.set_target_fps(20);
    send(&mut runtime, "start");
    let Wake::Frame(second) = runtime.poll().wake else { panic!("frames run") };
    assert_eq!(runtime.model().frames, [0.0], "the first frame comes at once");
    assert_eq!(second, clock.now() + Duration::from_millis(50));

    clock.advance(Duration::from_millis(20));
    runtime.poll();
    assert_eq!(runtime.model().frames.len(), 1, "no frame before its time");
    clock.advance(Duration::from_millis(30));
    runtime.poll();
    clock.advance(Duration::from_millis(50));
    runtime.poll();
    assert_eq!(runtime.model().frames, [0.0, 0.05, 0.05]);

    // A frame that comes late is not caught up on.
    clock.advance(Duration::from_millis(170));
    runtime.poll();
    assert_eq!(runtime.model().frames.len(), 4);
    let Wake::Frame(next) = runtime.poll().wake else { panic!("frames run") };
    assert_eq!(next, clock.now() + Duration::from_millis(50));

    send(&mut runtime, "stop");
    assert!(matches!(runtime.poll().wake, Wake::At(_)), "only the `every` second is left");
}

/// While frames run, a timer due between two frames waits for the next one,
/// so the runtime wakes once per frame.
#[test]
fn timers_are_handled_with_the_next_frame() {
    let (mut runtime, clock) = start();
    runtime.set_target_fps(20);
    let t0 = clock.now();
    send(&mut runtime, "start");
    send(&mut runtime, "arm");
    runtime.poll();
    clock.advance(Duration::from_millis(990));
    assert_eq!(runtime.poll().wake, Wake::Frame(t0 + Duration::from_millis(1040)), "not the timer at 1000 ms");
    clock.advance(Duration::from_millis(50));
    runtime.poll();
    assert_eq!(runtime.model().fired, 1);
}

#[derive(Debug, Clone, XeruneMessage)]
enum StatsMsg {
    /// Built by the subscription only: no string form.
    #[xerune(skip)]
    Stats(FrameStats),
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div style="width: 50px; height: 50px; background-color: red">{{ received.len() }}</div>"#)]
struct StatsModel {
    received: Vec<FrameStats>,
}

impl Model for StatsModel {
    type Message = StatsMsg;
    fn update(&mut self, msg: StatsMsg, _context: &mut Context<StatsMsg>) {
        let StatsMsg::Stats(stats) = msg;
        self.received.push(stats);
    }

    fn subscriptions(&self) -> Subscriptions<StatsMsg> {
        Subscriptions::none().stats(Duration::from_secs(1), StatsMsg::Stats)
    }
}

struct NoRenderer;

impl Renderer for NoRenderer {
    fn render(&mut self, _: &mut Surface<'_>, _: &[DrawCommand], _: &RenderResources<'_>, _: Rect) {}
}

/// The stats subscription reports the frames rendered in each interval,
/// and wakes the runtime for it when nothing else would.
#[test]
fn stats_come_every_interval_with_the_frames_rendered() {
    assert!("stats".parse::<StatsMsg>().is_err(), "a skipped variant has no string form");
    let mut runtime = Runtime::new(StatsModel { received: Vec::new() }, MockMeasurer);
    let clock = ManualClock::new();
    runtime.set_clock(clock.clone());
    runtime.set_size(100.0, 100.0);
    let start = clock.now();
    let mut pixels = vec![0u32; 100 * 100];
    for _ in 0..3 {
        runtime.render(&mut NoRenderer, &mut Surface::new(Pixels::Xrgb8888(&mut pixels), 100, 100));
        clock.advance(Duration::from_millis(100));
    }
    assert_eq!(runtime.poll().wake, Wake::At(start + Duration::from_secs(1)));
    assert!(runtime.model().received.is_empty());

    clock.advance(Duration::from_millis(700));
    runtime.poll();
    let [stats] = &runtime.model().received[..] else { panic!("one report: {:?}", runtime.model().received) };
    assert_eq!((stats.frames, stats.period), (3, Duration::from_secs(1)));
    assert_eq!(stats.fps(), 3.0);
    assert!(stats.commands > 0, "the box and its text");
    // The first frame repaints all, the other two nothing.
    assert!((stats.repainted - 1.0 / 3.0).abs() < 1e-6, "{}", stats.repainted);
}
