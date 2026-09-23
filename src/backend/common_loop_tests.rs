use super::*;
use crate::{ContainerStyle, Context, TemplateLayout, UiBuilder};
use crate::backend::{EventProxy, MpscProxy};
use std::sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{channel, Sender}};
use std::thread::{self, JoinHandle};
use std::time::Duration;

enum Event {
    Updated,
    Presented(Instant),
}

struct TestModel {
    events: Sender<Event>,
    animate: bool,
}

impl Model for TestModel {
    type Message = String;

    fn update(&mut self, _: String, _: &mut Context) {
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
    proxy: MpscProxy,
    events: Receiver<Event>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestLoop {
    fn start(animate: bool, queued_messages: usize) -> Self {
        let (events_tx, events) = channel();
        let (proxy_tx, proxy_rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = stop.clone();
        let thread = thread::spawn(move || {
            let (sender, messages) = channel();
            let proxy = MpscProxy { sender, event_loop_thread: thread::current() };
            for _ in 0..queued_messages {
                proxy.send_message("update".into()).unwrap();
            }
            proxy_tx.send(proxy).unwrap();
            let model = TestModel { events: events_tx.clone(), animate };
            let mut runtime = Runtime::new(model, Measurer);
            runtime.target_fps = 25;
            let presenter = Presenter { events: events_tx, stop: thread_stop };
            let surface = SurfaceInfo {
                logical_w: 1, logical_h: 1, disp_w: 1, disp_h: 1, rotation: 0,
            };
            let _ = run_embedded_event_loop(
                runtime, |_, _, _, _| None, NoInput, presenter, surface, messages,
            );
        });
        Self { proxy: proxy_rx.recv().unwrap(), events, stop, thread: Some(thread) }
    }

    fn next_event(&self) -> Event {
        self.events.recv_timeout(Duration::from_secs(2)).expect("Event loop stalled")
    }
}

impl Drop for TestLoop {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.proxy.event_loop_thread.unpark();
        let _ = self.thread.take().unwrap().join();
    }
}

#[test]
fn proxy_wakes_idle_loop_without_hardware_input() {
    let event_loop = TestLoop::start(false, 0);
    assert!(matches!(event_loop.next_event(), Event::Presented(_)));
    // Allow the loop to enter its idle wait; the next event must come from the proxy.
    assert!(event_loop.events.recv_timeout(Duration::from_millis(20)).is_err());
    event_loop.proxy.send_message("update".into()).unwrap();
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
