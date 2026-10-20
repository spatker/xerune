use super::*;
use crate::{ContainerStyle, Context, TemplateLayout, UiBuilder};
use crate::backend::{Messenger, channel_messenger};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{channel, Sender}};
use std::thread::{self, JoinHandle};
use std::time::Duration;

enum Event {
    Updated,
    /// A `Messenger::update` ran.
    Changed,
    Presented(Instant),
}

struct TestModel {
    events: Sender<Event>,
    animate: bool,
}

impl Model for TestModel {
    type Message = String;

    fn update(&mut self, _: String, _: &mut Context<Self::Message>) {
        self.events.send(Event::Updated).unwrap();
    }
}

impl TemplateLayout for TestModel {
    fn stylesheet(&self) -> &'static str {
        if self.animate {
            "@keyframes pulse { from { background-color: red; } to { background-color: blue; } }
             div { animation: pulse 1s linear infinite; }"
        } else {
            ""
        }
    }

    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _: &impl TextMeasurer,
        _: &ContainerStyle,
        _: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
        builder.create_element("div", &[])
    }
}

struct Measurer;

impl TextMeasurer for Measurer {
    fn measure_text(&self, _: &str, _: f32, _: u16) -> (f32, f32) {
        (0.0, 0.0)
    }
}

struct NoRenderer;

impl crate::Renderer for NoRenderer {
    fn render(&mut self, _: &mut crate::Surface<'_>, _: &[crate::DrawCommand], _: &crate::RenderResources<'_>, _: Rect) {}
}

struct NoInput;

impl InputSource for NoInput {
    fn dispatch_events<M: Model + TemplateLayout, TM: TextMeasurer>(
        &mut self,
        _: &mut Runtime<M, TM>,
        _: &SurfaceInfo,
    ) -> Result<bool, BackendError> {
        Ok(false)
    }

    fn wait_for_event(&mut self, _: Option<Duration>) {
        panic!("The loop must wait for messages as well as hardware input");
    }
}

struct Presenter {
    events: Sender<Event>,
    stop: Arc<AtomicBool>,
}

impl FramePresenter for Presenter {
    fn prepare_frame(&mut self) -> Result<(), BackendError> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(BackendError::Run("test finished".into()));
        }
        Ok(())
    }

    fn present(&mut self, _: &[u32], _: &SurfaceInfo, _: Option<Rect>) -> Result<(), BackendError> {
        self.events.send(Event::Presented(Instant::now())).unwrap();
        Ok(())
    }
}

struct TestLoop {
    messenger: Messenger<TestModel>,
    loop_thread: thread::Thread,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
    events_tx: Sender<Event>,
    /// Whether the loop ended without an error.
    thread: Option<JoinHandle<bool>>,
}

impl TestLoop {
    fn start(animate: bool, queued_messages: usize) -> Self {
        let (events_tx, events) = channel();
        let (proxy_tx, proxy_rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let loop_events = events_tx.clone();
        let thread = thread::spawn(move || {
            let (sender, messages) = channel();
            let messenger = channel_messenger(sender, thread::current());
            for _ in 0..queued_messages {
                messenger.send("update").unwrap();
            }
            proxy_tx.send(messenger).unwrap();
            let model = TestModel { events: events_tx.clone(), animate };
            let mut runtime = Runtime::new(model, Measurer);
            runtime.set_target_fps(25);
            let presenter = Presenter { events: events_tx, stop: thread_stop };
            let surface = SurfaceInfo {
                logical_w: 1, logical_h: 1, disp_w: 1, disp_h: 1, rotation: 0,
            };
            run_embedded_event_loop(
                runtime, NoRenderer, NoInput, presenter, surface, messages,
            ).is_ok()
        });
        let loop_thread = thread.thread().clone();
        Self { messenger: proxy_rx.recv().unwrap(), loop_thread, events, events_tx: loop_events, stop, thread: Some(thread) }
    }

    fn next_event(&self) -> Event {
        self.events.recv_timeout(Duration::from_secs(2)).expect("Event loop stalled")
    }
}

impl Drop for TestLoop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.loop_thread.unpark();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[test]
fn proxy_wakes_idle_loop_without_hardware_input() {
    let event_loop = TestLoop::start(false, 0);
    assert!(matches!(event_loop.next_event(), Event::Presented(_)));
    // Allow the loop to enter its idle wait; the next event must come from the proxy.
    assert!(event_loop.events.recv_timeout(Duration::from_millis(20)).is_err());
    event_loop.messenger.send("update").unwrap();
    assert!(matches!(event_loop.next_event(), Event::Updated));
    assert!(matches!(event_loop.next_event(), Event::Presented(_)));
}

#[test]
fn queued_messages_are_drained_across_batch_limits() {
    let event_loop = TestLoop::start(false, 650);
    let mut updates = 0;
    while updates < 650 {
        if matches!(event_loop.next_event(), Event::Updated) {
            updates += 1;
        }
    }
}

#[test]
fn dirty_animation_frames_obey_frame_deadline() {
    let event_loop = TestLoop::start(true, 0);
    let Event::Presented(first) = event_loop.next_event() else { panic!("Expected a frame") };
    let Event::Presented(second) = event_loop.next_event() else { panic!("Expected a frame") };
    // 25 FPS requests 40 ms; leave a little tolerance for work preceding presentation.
    assert!(second.duration_since(first) >= Duration::from_millis(30));
}

#[test]
fn updates_run_between_messages_in_order() {
    let event_loop = TestLoop::start(false, 0);
    assert!(matches!(event_loop.next_event(), Event::Presented(_)));
    event_loop.messenger.send("update").unwrap();
    let events = event_loop.events_tx.clone();
    event_loop.messenger.update(move |_, _| events.send(Event::Changed).unwrap()).unwrap();
    event_loop.messenger.send("update").unwrap();
    let mut seen = Vec::new();
    while seen.len() < 3 {
        match event_loop.next_event() {
            Event::Updated => seen.push("update"),
            Event::Changed => seen.push("change"),
            Event::Presented(_) => {}
        }
    }
    assert_eq!(seen, ["update", "change", "update"]);
}

#[test]
fn quit_ends_the_loop() {
    let mut event_loop = TestLoop::start(false, 0);
    assert!(matches!(event_loop.next_event(), Event::Presented(_)));
    event_loop.messenger.quit().unwrap();
    let thread = event_loop.thread.take().unwrap();
    let start = Instant::now();
    while !thread.is_finished() {
        assert!(start.elapsed() < Duration::from_secs(2), "the loop didn't end");
        thread::yield_now();
    }
    assert!(thread.join().unwrap(), "the loop ended with an error");
}
