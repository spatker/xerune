//! Timers must follow the clock that drives the runtime. With `tick_at_ms`
//! the host's timestamps can have any origin (browser `requestAnimationFrame`
//! time is relative to page load; `no_std` hosts count from boot).

use xerune::*;

#[derive(Debug, PartialEq, XeruneMessage)]
enum Msg {
    Arm,
    Fired,
}

#[derive(XeruneTemplate)]
#[template(source = "<div>{{ fired }}</div>")]
struct TimerModel {
    fired: u32,
}

impl Model for TimerModel {
    type Message = Msg;
    fn update(&mut self, msg: Msg, context: &mut Context) {
        match msg {
            Msg::Arm => context.set_timeout("fired".to_string(), 1000),
            Msg::Fired => self.fired += 1,
        }
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

#[test]
fn timeout_waits_its_full_delay_on_a_host_millisecond_clock() {
    let mut runtime = Runtime::new(TimerModel { fired: 0 }, MockMeasurer);
    // The host clock starts far from zero, e.g. a runtime created 5 minutes after page load.
    let t0 = 300_000;
    runtime.tick_at_ms(t0);
    runtime.handle_messages(["arm".to_string()]);

    runtime.tick_at_ms(t0 + 16);
    runtime.tick_at_ms(t0 + 900);
    assert_eq!(runtime.model().fired, 0, "fired before its 1000 ms delay");

    runtime.tick_at_ms(t0 + 1100);
    assert_eq!(runtime.model().fired, 1);

    runtime.tick_at_ms(t0 + 5000);
    assert_eq!(runtime.model().fired, 1, "a timeout fires once");
}

#[test]
fn interval_set_through_runtime_uses_tick_clock() {
    let mut runtime = Runtime::new(TimerModel { fired: 0 }, MockMeasurer);
    runtime.tick_at_ms(42);
    runtime.set_interval("fired".to_string(), 100); // first deadline at 142
    runtime.tick_at_ms(130);
    assert_eq!(runtime.model().fired, 0);
    runtime.tick_at_ms(150);
    runtime.tick_at_ms(250);
    assert_eq!(runtime.model().fired, 2);
}

