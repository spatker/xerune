//! Where frames spend their time: per-phase timings collected by the
//! runtime while something listens ([`Subscriptions::stats`] or
//! `XERUNE_PROFILE=1`), and nothing otherwise.
//!
//! [`Subscriptions::stats`]: crate::Subscriptions::stats

#[cfg(feature = "std")]
use crate::alloc_prelude::*;
use crate::runtime::time::Instant;
use core::time::Duration;

/// A step of producing a frame, timed separately.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// `Model::update`.
    Update,
    /// Building the view: the template, styles and text shaping.
    View,
    /// Taffy layout.
    Layout,
    /// CSS animations and transitions stepping their styles.
    Animate,
    /// The display list and its damage against the previous frame.
    Paint,
    /// The renderer drawing the damaged areas.
    Raster,
}

const PHASES: usize = 6;

/// The time one phase took per rendered frame, over a [`FrameStats`] period.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PhaseTime {
    /// Average per frame.
    pub avg: Duration,
    /// The slowest frame's.
    pub max: Duration,
}

/// What the frames of a period cost, from [`Subscriptions::stats`].
///
/// Each frame's phases count the work since the frame before: updates and
/// rebuilds between two renders belong to the second. Times are measured
/// on the system's monotonic clock and are zero without one (`no_std`,
/// wasm).
///
/// [`Subscriptions::stats`]: crate::Subscriptions::stats
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct FrameStats {
    /// The time the stats cover, on the runtime's clock.
    pub period: Duration,
    /// Frames rendered in it.
    pub frames: u32,
    /// `Model::update` calls.
    pub update: PhaseTime,
    /// Building views: the template, styles and text shaping.
    pub view: PhaseTime,
    /// Layout.
    pub layout: PhaseTime,
    /// CSS animations and transitions.
    pub animate: PhaseTime,
    /// Building the display list and finding what changed.
    pub paint: PhaseTime,
    /// Drawing (the [`Renderer`](crate::Renderer)).
    pub raster: PhaseTime,
    /// Draw commands per frame, on average.
    pub commands: u32,
    /// Share of the screen repainted per frame, on average (0 to 1).
    pub repainted: f32,
}

impl FrameStats {
    /// Frames per second over the period.
    pub fn fps(&self) -> f32 {
        let secs = self.period.as_secs_f32();
        if secs > 0.0 { self.frames as f32 / secs } else { 0.0 }
    }

    /// The phases with their names, in frame order.
    pub fn phases(&self) -> [(&'static str, PhaseTime); PHASES] {
        [
            ("update", self.update),
            ("view", self.view),
            ("layout", self.layout),
            ("animate", self.animate),
            ("paint", self.paint),
            ("raster", self.raster),
        ]
    }

    /// The average frame's time in all phases together.
    pub fn frame_time(&self) -> Duration {
        self.phases().iter().map(|(_, t)| t.avg).sum()
    }
}

/// A point on the system's monotonic clock, where there is one.
#[derive(Clone, Copy)]
pub(crate) struct Stamp(#[cfg(all(feature = "std", not(target_arch = "wasm32")))] std::time::Instant);

impl Stamp {
    fn now() -> Self {
        Stamp(
            #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
            std::time::Instant::now(),
        )
    }

    fn elapsed(self) -> Duration {
        #[cfg(all(feature = "std", not(target_arch = "wasm32")))]
        return self.0.elapsed();
        #[cfg(not(all(feature = "std", not(target_arch = "wasm32"))))]
        Duration::ZERO
    }
}

/// Frames counted since `start`.
struct Window {
    start: Instant,
    frames: u32,
    sum: [Duration; PHASES],
    max: [Duration; PHASES],
    commands: u64,
    repainted: f32,
}

impl Window {
    fn new(start: Instant) -> Self {
        Window { start, frames: 0, sum: [Duration::ZERO; PHASES], max: [Duration::ZERO; PHASES], commands: 0, repainted: 0.0 }
    }

    fn add(&mut self, frame: &[Duration; PHASES], commands: usize, repainted: f32) {
        self.frames += 1;
        for i in 0..PHASES {
            self.sum[i] += frame[i];
            self.max[i] = self.max[i].max(frame[i]);
        }
        self.commands += commands as u64;
        self.repainted += repainted;
    }

    /// The stats so far, starting a new window at `now`.
    fn take(&mut self, now: Instant) -> FrameStats {
        let window = core::mem::replace(self, Window::new(now));
        let n = window.frames.max(1);
        let phase = |i: usize| PhaseTime { avg: window.sum[i] / n, max: window.max[i] };
        FrameStats {
            period: now.saturating_duration_since(window.start),
            frames: window.frames,
            update: phase(0),
            view: phase(1),
            layout: phase(2),
            animate: phase(3),
            paint: phase(4),
            raster: phase(5),
            commands: (window.commands / n as u64) as u32,
            repainted: window.repainted / n as f32,
        }
    }
}

/// How often `XERUNE_PROFILE=1` prints a report.
#[cfg(feature = "std")]
const REPORT_EVERY: Duration = Duration::from_secs(1);

/// Collects phase times while something listens.
pub(crate) struct Profiler {
    /// Time since the last rendered frame, per phase.
    pending: [Duration; PHASES],
    /// For the model's stats subscription, and its interval.
    subscription: Option<(Window, Duration)>,
    /// For `XERUNE_PROFILE=1` reports.
    report: Option<Window>,
}

impl Profiler {
    pub(crate) fn new(report_from: Option<Instant>) -> Self {
        Profiler { pending: [Duration::ZERO; PHASES], subscription: None, report: report_from.map(Window::new) }
    }

    /// Starts the windows over at `now`, on a new clock.
    pub(crate) fn restart(&mut self, now: Instant) {
        if let Some((window, _)) = &mut self.subscription {
            *window = Window::new(now);
        }
        if let Some(window) = &mut self.report {
            *window = Window::new(now);
        }
    }

    /// Whether anything listens, so phases are worth timing.
    pub(crate) fn enabled(&self) -> bool {
        self.subscription.is_some() || self.report.is_some()
    }

    /// Whether `XERUNE_PROFILE` reports are on.
    pub(crate) fn reporting(&self) -> bool {
        self.report.is_some()
    }

    /// Starts timing a phase, if anything listens.
    pub(crate) fn start(&self) -> Option<Stamp> {
        self.enabled().then(Stamp::now)
    }

    /// Adds the time since `start` to `phase`.
    pub(crate) fn stop(&mut self, phase: Phase, start: Option<Stamp>) {
        if let Some(start) = start {
            self.pending[phase as usize] += start.elapsed();
        }
    }

    /// Follows the model's stats subscription: its interval, or `None`.
    /// A new subscription (or a new interval) starts counting at `now`.
    pub(crate) fn subscribe(&mut self, interval: Option<Duration>, now: Instant) {
        self.subscription = match (self.subscription.take(), interval) {
            (Some((window, old)), Some(interval)) if old == interval => Some((window, interval)),
            (_, Some(interval)) => Some((Window::new(now), interval)),
            (_, None) => None,
        };
    }

    /// When the subscription's stats are due.
    pub(crate) fn due(&self) -> Option<Instant> {
        self.subscription.as_ref().map(|(window, interval)| window.start + *interval)
    }

    /// The subscription's stats, if due at `now`.
    pub(crate) fn take_due(&mut self, now: Instant) -> Option<FrameStats> {
        let (window, interval) = self.subscription.as_mut()?;
        (now >= window.start + *interval).then(|| window.take(now))
    }

    /// A frame was rendered: its phases so far go into the stats.
    pub(crate) fn frame(&mut self, commands: usize, repainted: f32) {
        if !self.enabled() {
            return;
        }
        let frame = core::mem::replace(&mut self.pending, [Duration::ZERO; PHASES]);
        if let Some((window, _)) = &mut self.subscription {
            window.add(&frame, commands, repainted);
        }
        if let Some(window) = &mut self.report {
            window.add(&frame, commands, repainted);
        }
    }

    /// The report's stats, once a report is due at `now`.
    #[cfg(feature = "std")]
    pub(crate) fn take_report(&mut self, now: Instant) -> Option<FrameStats> {
        let window = self.report.as_mut()?;
        (now >= window.start + REPORT_EVERY).then(|| window.take(now))
    }
}

/// `XERUNE_PROFILE=1`'s report: the frame stats, then the renderer's time
/// per kind of draw command.
#[cfg(feature = "std")]
pub(crate) fn report(stats: &FrameStats, commands: &[crate::graphics::CommandProfile]) -> String {
    use core::fmt::Write;
    let us = |d: Duration| d.as_micros();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "xerune: {:.1} fps, {} frames, {} us/frame, {} commands, {:.0}% repainted",
        stats.fps(),
        stats.frames,
        us(stats.frame_time()),
        stats.commands,
        stats.repainted * 100.0
    );
    let _ = writeln!(out, "  {:<8} {:>8} {:>8}", "phase", "avg us", "max us");
    for (name, time) in stats.phases() {
        let _ = writeln!(out, "  {:<8} {:>8} {:>8}", name, us(time.avg), us(time.max));
    }
    if !commands.is_empty() {
        let frames = stats.frames.max(1);
        let _ = writeln!(out, "  {:<8} {:>8} {:>8}", "raster", "us/frame", "count");
        for command in commands {
            let _ = writeln!(out, "  {:<8} {:>8} {:>8}", command.kind, us(command.time / frames), command.count / frames);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::time::Instant;

    /// `ms` milliseconds after a fixed start.
    fn at(ms: u64) -> Instant {
        use std::sync::OnceLock;
        static START: OnceLock<Instant> = OnceLock::new();
        *START.get_or_init(Instant::now) + Duration::from_millis(ms)
    }

    #[test]
    fn windows_average_per_frame_and_keep_the_slowest() {
        let start = at(0);
        let mut window = Window::new(start);
        let ms = Duration::from_millis;
        let mut frame = [Duration::ZERO; PHASES];
        frame[Phase::Raster as usize] = ms(2);
        window.add(&frame, 10, 0.5);
        frame[Phase::Raster as usize] = ms(4);
        window.add(&frame, 20, 0.0);
        let stats = window.take(at(500));
        assert_eq!(stats.frames, 2);
        assert_eq!(stats.fps(), 4.0);
        assert_eq!(stats.raster, PhaseTime { avg: ms(3), max: ms(4) });
        assert_eq!(stats.update, PhaseTime::default());
        assert_eq!((stats.commands, stats.repainted), (15, 0.25));
        assert_eq!(window.frames, 0, "taking starts a new window");
        assert_eq!(window.start, at(500));
    }

    #[test]
    fn a_subscription_keeps_counting_while_its_interval_stays() {
        let mut profiler = Profiler::new(None);
        assert!(!profiler.enabled());
        profiler.subscribe(Some(Duration::from_secs(1)), at(0));
        profiler.frame(5, 1.0);
        profiler.subscribe(Some(Duration::from_secs(1)), at(300));
        assert_eq!(profiler.take_due(at(999)), None);
        let stats = profiler.take_due(at(1000)).unwrap();
        assert_eq!((stats.frames, stats.period), (1, Duration::from_secs(1)));
        profiler.subscribe(None, at(1000));
        assert!(!profiler.enabled());
    }
}
