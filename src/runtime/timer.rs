use crate::runtime::time::Instant;
use core::time::Duration;

/// Names a timer started with [`crate::Context::after`], to cancel it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimerId(pub(crate) u64);

/// A message the runtime sends to the model once its deadline passes.
#[derive(Clone, Debug)]
pub(crate) struct Timer<M> {
    pub(crate) id: TimerId,
    pub(crate) deadline: Instant,
    pub(crate) message: M,
}

/// A timer requested in `update`, waiting for the runtime to set its
/// deadline once `update` returns.
pub(crate) struct PendingTimer<M> {
    pub(crate) id: TimerId,
    pub(crate) delay: Duration,
    pub(crate) message: M,
}

/// The messages a model gets as time passes, from
/// [`crate::Model::subscriptions`]. The runtime asks for them again after
/// each `update`, so they follow the model's state: there is nothing to
/// start or stop.
///
/// ```ignore
/// fn subscriptions(&self) -> Subscriptions<Msg> {
///     let subscriptions = Subscriptions::none().every(Duration::from_secs(1), Msg::Clock);
///     if self.playing { subscriptions.frames(Msg::Frame) } else { subscriptions }
/// }
/// ```
pub struct Subscriptions<M> {
    pub(crate) every: Vec<(Duration, M)>,
    pub(crate) frames: Option<Box<dyn Fn(f32) -> M + Send>>,
    pub(crate) stats: Option<(Duration, Box<dyn Fn(super::FrameStats) -> M + Send>)>,
}

impl<M> Subscriptions<M> {
    /// No messages.
    pub fn none() -> Self {
        Self { every: Vec::new(), frames: None, stats: None }
    }

    /// `message` every `interval`, the first one `interval` after the model
    /// asks for it. It keeps its phase as long as the model keeps asking for
    /// the same interval (the n-th `every` of that interval is the same one).
    pub fn every(mut self, interval: Duration, message: M) -> Self {
        self.every.push((interval.max(Duration::from_millis(1)), message));
        self
    }

    /// A message on every frame, made by `message` from the seconds since
    /// the previous frame (0 on the first), like raylib's `GetFrameTime()`.
    /// Frames come at the runtime's target frame rate
    /// ([`crate::Runtime::set_target_fps`]).
    pub fn frames(mut self, message: impl Fn(f32) -> M + Send + 'static) -> Self {
        self.frames = Some(Box::new(message));
        self
    }

    /// Every `interval`, a message made by `message` from what the frames
    /// rendered in it cost ([`FrameStats`](super::FrameStats): frame rate
    /// and time per phase), e.g. for an on-screen FPS counter. The runtime
    /// times frames only while something asks for them.
    pub fn stats(mut self, interval: Duration, message: impl Fn(super::FrameStats) -> M + Send + 'static) -> Self {
        self.stats = Some((interval.max(Duration::from_millis(1)), Box::new(message)));
        self
    }
}

impl<M> Default for Subscriptions<M> {
    fn default() -> Self {
        Self::none()
    }
}

/// A running `every` subscription.
pub(crate) struct Interval {
    pub(crate) interval: Duration,
    pub(crate) next: Instant,
}

/// What [`crate::Runtime::poll`] did, and when to call it again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Poll {
    /// The view changed: render a frame.
    pub redraw: bool,
    /// When the runtime has work again.
    pub wake: Wake,
}

/// When the runtime next has work: when to call [`crate::Runtime::poll`]
/// again. Input wakes it earlier whatever this says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// Something animates (CSS animations or transitions, or a model's
    /// frame subscription): the next frame is due at this time.
    Frame(Instant),
    /// A timer is due at this time.
    At(Instant),
    /// Nothing until input arrives.
    Idle,
}

impl Wake {
    /// The time to poll again at, if any.
    pub fn deadline(self) -> Option<Instant> {
        match self {
            Wake::Frame(at) | Wake::At(at) => Some(at),
            Wake::Idle => None,
        }
    }
}
