use crate::alloc_prelude::*;

/// Representation of an RGBA color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    /// Red component (0 to 255).
    pub r: u8, 
    /// Green component (0 to 255).
    pub g: u8, 
    /// Blue component (0 to 255).
    pub b: u8, 
    /// Alpha component (0 to 255, where 255 is opaque).
    pub a: u8,
}

impl Color {
    /// Opaque black color helper.
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    /// Opaque white color helper.
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    
    /// Create a new RGBA color.
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Create a color from RGBA bytes.
    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

impl Default for Color {
    fn default() -> Self {
        Self { r: 0, g: 0, b: 0, a: 0 }
    }
}

/// Representation of a linear CSS gradient.
#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    /// The angle of the gradient in degrees.
    pub angle: f32,
    /// Color stops consisting of colors and their fractional positions (0.0 to 1.0).
    pub stops: Arc<[(Color, f32)]>,
}

/// Shape of a radial CSS gradient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialShape {
    /// Same radius in both directions.
    Circle,
    /// Radii scaled independently to the box aspect ratio.
    Ellipse,
}

/// How far a radial gradient's ending shape reaches (CSS `<radial-extent>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadialExtent {
    /// Touch the nearest side of the box.
    ClosestSide,
    /// Touch the farthest side of the box.
    FarthestSide,
    /// Pass through the nearest corner of the box.
    ClosestCorner,
    /// Pass through the farthest corner of the box (the CSS default).
    FarthestCorner,
}

/// Representation of a radial CSS gradient.
#[derive(Debug, Clone, PartialEq)]
pub struct RadialGradient {
    /// Ending shape.
    pub shape: RadialShape,
    /// Size of the ending shape.
    pub extent: RadialExtent,
    /// Center as fractions of the box size (0.5, 0.5 is the middle).
    pub center: (f32, f32),
    /// Color stops; positions are fractions of the gradient ray.
    pub stops: Arc<[(Color, f32)]>,
    /// `repeating-radial-gradient`: the stop list tiles outwards.
    pub repeating: bool,
}

/// Representation of a conic CSS gradient.
#[derive(Debug, Clone, PartialEq)]
pub struct ConicGradient {
    /// Start angle in degrees, clockwise from 12 o'clock.
    pub from: f32,
    /// Center as fractions of the box size (0.5, 0.5 is the middle).
    pub center: (f32, f32),
    /// Color stops; positions are fractions of a full turn.
    pub stops: Arc<[(Color, f32)]>,
    /// `repeating-conic-gradient`: the stop list tiles around the circle.
    pub repeating: bool,
}

impl ConicGradient {
    /// What the gradient paints of `rect`: the sectors its non-transparent
    /// stops cover, boxed, with a margin for anti-aliasing. `None` when
    /// nothing is painted.
    fn painted_bounds(&self, rect: &Rect) -> Option<Rect> {
        let (x1, y1) = (rect.x + rect.width, rect.y + rect.height);
        let (cx, cy) = (rect.x + self.center.0 * rect.width, rect.y + self.center.1 * rect.height);
        if self.repeating || !(rect.x < cx && cx < x1 && rect.y < cy && cy < y1) {
            return Some(*rect);
        }

        // Painted ranges of the turn, from `from`. Positions only ever
        // increase; before the first stop and after the last one, their
        // colors continue.
        let mut ranges: Vec<(f32, f32)> = Vec::new();
        let mut add = |lo: f32, hi: f32| match ranges.last_mut() {
            Some(last) if last.1 >= lo => last.1 = last.1.max(hi),
            _ => ranges.push((lo, hi)),
        };
        let mut at = 0.0f32;
        let mut previous: Option<Color> = None;
        for &(color, pos) in self.stops.iter() {
            let pos = pos.clamp(at, 1.0);
            if previous.unwrap_or(color).a != 0 || color.a != 0 {
                add(at, pos);
            }
            (at, previous) = (pos, Some(color));
        }
        if previous.is_some_and(|c| c.a != 0) {
            add(at, 1.0);
        }

        // The box of each sector within `rect`: the center, where its edges
        // leave the rect and the corners between them.
        let corners = [(rect.x, rect.y), (x1, rect.y), (x1, y1), (rect.x, y1)];
        let from = self.from / 360.0;
        let mut bounds: Option<Rect> = None;
        for (lo, hi) in ranges {
            // A pixel blends the colors of up to about a pixel around it.
            const SLACK: f32 = 1.0 / 512.0;
            let (lo, hi) = (lo - SLACK, hi + SLACK);
            let (mut min_x, mut min_y, mut max_x, mut max_y) = (cx, cy, cx, cy);
            let mut include = |(x, y): (f32, f32)| {
                (min_x, min_y, max_x, max_y) = (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y));
            };
            for turn in [lo, hi] {
                // Clockwise from 12 o'clock.
                let angle = (turn + from) * core::f32::consts::TAU;
                let (dx, dy) = (approx_sin(angle), -approx_sin(angle + core::f32::consts::FRAC_PI_2));
                let reach = |d: f32, near: f32, far: f32| if d > 1e-6 { far / d } else if d < -1e-6 { near / d } else { f32::INFINITY };
                let s = reach(dx, rect.x - cx, x1 - cx).min(reach(dy, rect.y - cy, y1 - cy));
                include((cx + dx * s, cy + dy * s));
            }
            for (x, y) in corners {
                let turn = approx_atan2(x - cx, cy - y) / core::f32::consts::TAU - from;
                let turn = turn - libm_floor(turn - lo);
                if turn <= hi {
                    include((x, y));
                }
            }
            let sector = Rect::new(min_x, min_y, max_x - min_x, max_y - min_y);
            bounds = Some(bounds.map_or(sector, |b| b.expand(sector)));
        }
        bounds.map(|b| Rect::new(b.x - 2.0, b.y - 2.0, b.width + 4.0, b.height + 4.0))
    }
}

/// `x.floor()`, which `core` lacks.
fn libm_floor(x: f32) -> f32 {
    let i = x as i32 as f32;
    if i > x { i - 1.0 } else { i }
}

/// sin within about 1e-3, without libm.
fn approx_sin(x: f32) -> f32 {
    use core::f32::consts::{PI, TAU};
    let x = x - TAU * libm_floor((x + PI) / TAU);
    let y = 4.0 / PI * x - 4.0 / (PI * PI) * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

/// atan2 within about 1e-5 rad, without libm.
fn approx_atan2(y: f32, x: f32) -> f32 {
    let (ax, ay) = (x.abs(), y.abs());
    let mx = ax.max(ay);
    if mx == 0.0 {
        return 0.0;
    }
    let a = ax.min(ay) / mx;
    let s = a * a;
    let mut r = ((-0.046_496_475 * s + 0.159_314_22) * s - 0.327_622_76) * s * a + a;
    if ay > ax {
        r = core::f32::consts::FRAC_PI_2 - r;
    }
    if x < 0.0 {
        r = core::f32::consts::PI - r;
    }
    if y < 0.0 { -r } else { r }
}

/// Any CSS gradient image usable as a background.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Gradient {
    /// `linear-gradient(...)`.
    Linear(LinearGradient),
    /// `radial-gradient(...)` / `repeating-radial-gradient(...)`.
    Radial(RadialGradient),
    /// `conic-gradient(...)` / `repeating-conic-gradient(...)`.
    Conic(ConicGradient),
}

/// Representation of a rectangle in 2D space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    /// The x coordinate of the top-left corner.
    pub x: f32,
    /// The y coordinate of the top-left corner.
    pub y: f32,
    /// The width of the rectangle.
    pub width: f32,
    /// The height of the rectangle.
    pub height: f32,
}

impl Rect {
    /// Create a new rectangle.
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// True if the rectangle covers no area.
    pub fn is_empty(&self) -> bool {
        !(self.width > 0.0 && self.height > 0.0)
    }

    /// The smallest rectangle on whole pixels that contains this one.
    pub fn round_out(&self) -> Rect {
        let ceil = |v: f32| -floor(-v);
        let (x, y) = (floor(self.x), floor(self.y));
        Rect {
            x,
            y,
            width: ceil(self.x + self.width) - x,
            height: ceil(self.y + self.height) - y,
        }
    }

    /// Expands this rectangle to encompass another rectangle. Empty rectangles
    /// add nothing, wherever they are.
    pub fn expand(&self, other: Rect) -> Rect {
        if other.is_empty() {
            return *self;
        }
        if self.is_empty() {
            return other;
        }
        let min_x = self.x.min(other.x);
        let min_y = self.y.min(other.y);
        let max_x = (self.x + self.width).max(other.x + other.width);
        let max_y = (self.y + self.height).max(other.y + other.height);
        Rect {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }

    /// Checks if this rectangle intersects another.
    pub fn intersects(&self, other: &Rect) -> bool {
        !(self.x + self.width <= other.x
            || other.x + other.width <= self.x
            || self.y + self.height <= other.y
            || other.y + other.height <= self.y)
    }

    /// Computes the intersection of two rectangles, returning None if they do not overlap.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x1 = self.x.max(other.x);
        let y1 = self.y.max(other.y);
        let x2 = (self.x + self.width).min(other.x + other.width);
        let y2 = (self.y + self.height).min(other.y + other.height);
        if x1 < x2 && y1 < y2 {
            Some(Rect {
                x: x1,
                y: y1,
                width: x2 - x1,
                height: y2 - y1,
            })
        } else {
            None
        }
    }
}

/// `f32::floor`, which needs std, for values that fit an i32 (screen
/// coordinates and font metrics).
pub(crate) fn floor(v: f32) -> f32 {
    let t = v as i32 as f32;
    if t > v { t - 1.0 } else { t }
}

/// The areas of the screen a frame repainted: a few rectangles, merged where
/// they overlap or merging them costs little.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Damage {
    rects: Vec<Rect>,
}

impl Damage {
    /// More rectangles are merged into the existing ones; each is drawn with
    /// a pass over the command list.
    pub const MAX_RECTS: usize = 8;

    /// Nothing was repainted.
    pub fn none() -> Self {
        Self::default()
    }

    /// All of `screen` was repainted.
    pub fn full(screen: Rect) -> Self {
        let mut damage = Self::none();
        damage.add(screen);
        damage
    }

    /// True if nothing was repainted.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// The repainted rectangles. They may overlap.
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// One rectangle around all repainted areas, `None` if nothing was.
    pub fn bounds(&self) -> Option<Rect> {
        self.rects.iter().copied().reduce(|a, b| a.expand(b))
    }

    /// Adds `rect`, rounded out to whole pixels, merging it with rectangles
    /// it overlaps or that it would barely enlarge.
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let mut rect = rect.round_out();
        // Merging may make the result cover others; repeat until it doesn't.
        while let Some(i) = self.rects.iter().position(|r| Self::should_merge(r, &rect)) {
            rect = rect.expand(self.rects.swap_remove(i));
        }
        if self.rects.len() == Self::MAX_RECTS {
            // Merge into the rectangle whose union grows the least.
            let growth = |r: &Rect| area(&r.expand(rect)) - area(r);
            let (i, _) = self
                .rects
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| growth(a).total_cmp(&growth(b)))
                .expect("MAX_RECTS > 0");
            let merged = rect.expand(self.rects.swap_remove(i));
            return self.add(merged);
        }
        self.rects.push(rect);
    }

    /// Overlapping rectangles are merged, and so are neighbours whose union
    /// is hardly bigger than the two (an extra pass over the commands costs
    /// more than a few extra pixels).
    fn should_merge(a: &Rect, b: &Rect) -> bool {
        const SLACK: f32 = 1024.0;
        a.intersects(b) || area(&a.expand(*b)) <= area(a) + area(b) + SLACK
    }
}

fn area(r: &Rect) -> f32 {
    r.width * r.height
}

/// A pixel buffer representing a custom user-drawn canvas element.
pub struct Canvas {
    /// Width of the canvas.
    pub width: u32,
    /// Height of the canvas.
    pub height: u32,
    /// Raw RGBA8 pixel data.
    pub data: Vec<u8>,
    /// Flags if the canvas has been modified and needs redraw.
    pub dirty: bool,
}

impl Canvas {
    /// Create a new canvas with the specified width and height.
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
            dirty: true,
        }
    }
}

/// Commands representing context actions to execute on the runtime.
pub(crate) enum ContextCommand {
    /// Scroll the element with this `id` into view.
    ScrollIntoView(String),
}

/// What [`crate::Model::update`] can ask of the runtime besides changing the
/// model: canvases to draw into, scrolling, and timers that send the model
/// its own messages (`M`) later.
pub struct Context<M> {
    /// Map of canvas IDs to their respective Canvas pixel buffers.
    pub canvases: HashMap<String, Canvas>,
    pub(crate) commands: Vec<ContextCommand>,
    pub(crate) pending_timers: Vec<crate::runtime::timer::PendingTimer<M>>,
    pub(crate) cancelled_timers: Vec<crate::runtime::TimerId>,
    next_timer_id: u64,
    /// Text asked for with [`Self::edit_text`], and how to tell the model.
    pub(crate) text_request: Option<(crate::keyboard::TextRequest, crate::keyboard::OnText<M>)>,
}

impl<M> Context<M> {
    /// Create a new, empty context.
    pub fn new() -> Self {
        Self {
            canvases: HashMap::new(),
            commands: Vec::new(),
            pending_timers: Vec::new(),
            cancelled_timers: Vec::new(),
            next_timer_id: 1,
            text_request: None,
        }
    }
    
    /// Get a mutable reference to the canvas with the specified ID, if it exists.
    pub fn canvas_mut(&mut self, id: &str) -> Option<&mut Canvas> {
        self.canvases.get_mut(id)
    }

    /// Current rendering viewport size in CSS pixels as `(width, height)`.
    ///
    /// Returns `(0.0, 0.0)` before the backend has reported a size.
    pub fn screen_size(&self) -> (f32, f32) {
        crate::screen::size()
    }

    /// Semantic breakpoint for the current viewport width.
    pub fn breakpoint(&self) -> crate::screen::Breakpoint {
        crate::screen::breakpoint()
    }

    /// Scrolls the element with this `id` attribute into view (in its nearest
    /// scrollable ancestor) once the view is rebuilt, like the DOM's
    /// `element.scrollIntoView()`.
    pub fn scroll_into_view(&mut self, id: &str) {
        self.commands.push(ContextCommand::ScrollIntoView(id.to_string()));
    }

    /// Sends `message` to the model once, `delay` from now. For messages
    /// that repeat, see [`crate::Model::subscriptions`].
    pub fn after(&mut self, delay: core::time::Duration, message: M) -> crate::runtime::TimerId {
        let id = crate::runtime::TimerId(self.next_timer_id);
        self.next_timer_id += 1;
        self.pending_timers.push(crate::runtime::timer::PendingTimer { id, delay, message });
        id
    }

    /// Asks the user for text with the on-screen keyboard, which covers the
    /// screen once `update` returns. When the user is done, the model gets
    /// `on_done(Some(text))` on OK, or `on_done(None)` on Cancel:
    ///
    /// ```text
    /// context.edit_text(TextRequest::new("Wi-Fi password").password().min_len(8).max_len(63),
    ///                   |password| Msg::Password(password));
    /// ```
    ///
    /// Asking again while the keyboard is open replaces the request.
    pub fn edit_text(&mut self, request: crate::keyboard::TextRequest, on_done: impl FnOnce(Option<String>) -> M + Send + 'static) {
        self.text_request = Some((request, Box::new(move |text| Some(on_done(text)))));
    }

    /// Stops a timer started with [`Self::after`] that hasn't fired yet.
    pub fn cancel(&mut self, timer: crate::runtime::TimerId) {
        self.pending_timers.retain(|t| t.id != timer);
        self.cancelled_timers.push(timer);
    }
}

impl<M> Default for Context<M> {
    fn default() -> Self {
        Self::new()
    }
}

/// Commands defining hardware-agnostic layout drawing primitives.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum DrawCommand {
    /// Restrict drawing within a specific clipping bounds.
    Clip {
        /// The bounding box of the clipping mask.
        rect: Rect,
    },
    /// Pop the last active clipping boundary.
    PopClip,
    /// Draw a box shadow.
    DrawBoxShadow {
        /// Layout bounds of the target container.
        rect: Rect,
        /// Border corner radius.
        border_radius: f32,
        /// Horizontal offset in pixels.
        offset_x: f32,
        /// Vertical offset in pixels.
        offset_y: f32,
        /// Blur radius in pixels.
        blur_radius: f32,
        /// Spread radius in pixels.
        spread_radius: f32,
        /// Shadow color.
        color: Color,
        /// Inset shadow flag.
        inset: bool,
    },
    /// Draw a styled rectangle.
    DrawRect {
        /// Rect layout bounds.
        rect: Rect,
        /// Optional solid fill color.
        color: Option<Color>,
        /// Optional gradient fill.
        gradient: Option<Gradient>,
        /// Border corner radius.
        border_radius: f32,
        /// The border, resolved: `none` sides have width 0 and every color
        /// is set.
        border: crate::style::Borders,
    },
    /// Draw one line of text.
    DrawText { 
        /// The text content.
        text: String, 
        /// Bounding box.
        rect: Rect,
        /// Text color.
        color: Color, 
        /// Size of the font in pixels.
        font_size: f32,
        /// Font weight (0 for Regular, 1 for Bold).
        weight: u16,
        /// The shaped glyphs of `text`, positioned by layout. Without it the
        /// renderer shapes `text` itself, as one line.
        run: Option<crate::text::GlyphRun>,
    },
    /// Draw a PNG image.
    DrawImage {
        /// Image source path.
        src: String,
        /// Bounding box.
        rect: Rect,
        /// Corner radius to clip image edges.
        border_radius: f32,
        /// How opaque the image is drawn, 0 to 1 (`opacity`).
        opacity: f32,
    },
    /// Draw the check mark of a checked checkbox: a stroke scaled to `rect`,
    /// as Chromium paints it.
    DrawCheckMark {
        /// The checkbox's box.
        rect: Rect,
        /// Stroke color.
        color: Color,
    },
    /// Render custom canvas pixel data.
    DrawCanvas {
        /// The canvas identifier.
        id: String,
        /// Bounding box.
        rect: Rect,
        /// Border corner radius.
        border_radius: f32,
        /// How opaque the pixels are drawn, 0 to 1 (`opacity`).
        opacity: f32,
    },
}

impl DrawCommand {
    /// Makes the command draw `opacity` (0 to 1) as opaque as it did: the
    /// alpha of its colors, or its own `opacity` for images and canvases.
    pub fn fade(&mut self, opacity: f32) {
        let fade = |color: &mut Color| color.a = (color.a as f32 * opacity + 0.5) as u8;
        let fade_copy = |color: &mut Color| {
            fade(color);
            *color
        };
        match self {
            DrawCommand::Clip { .. } | DrawCommand::PopClip => {}
            DrawCommand::DrawBoxShadow { color, .. } | DrawCommand::DrawText { color, .. } | DrawCommand::DrawCheckMark { color, .. } => fade(color),
            DrawCommand::DrawRect { color, gradient, border, .. } => {
                if let Some(color) = color {
                    fade(color);
                }
                // The stops are shared with the style: faded ones are new.
                let faded = |stops: &Arc<[(Color, f32)]>| -> Arc<[(Color, f32)]> {
                    stops.iter().map(|&(mut c, at)| (fade_copy(&mut c), at)).collect()
                };
                match gradient {
                    Some(Gradient::Linear(g)) => g.stops = faded(&g.stops),
                    Some(Gradient::Radial(g)) => g.stops = faded(&g.stops),
                    Some(Gradient::Conic(g)) => g.stops = faded(&g.stops),
                    None => {}
                }
                border.sides_mut().into_iter().filter_map(|side| side.color.as_mut()).for_each(fade);
            }
            DrawCommand::DrawImage { opacity: own, .. } | DrawCommand::DrawCanvas { opacity: own, .. } => *own *= opacity,
        }
    }

    /// Computes the layout bounding box of a draw command, padded for anti-aliasing bleeds.
    pub fn bounds(&self) -> Option<Rect> {
        let pad = 10.0; // Pad bounds generously to catch font overhangs and anti-aliasing bleeds
        let apply_pad = |r: Rect| Rect {
            x: r.x - pad,
            y: r.y - pad,
            width: r.width + pad * 2.0,
            height: r.height + pad * 2.0,
        };

        match self {
            DrawCommand::Clip { rect } => Some(apply_pad(*rect)),
            DrawCommand::PopClip => None,
            DrawCommand::DrawBoxShadow { rect, offset_x, offset_y, blur_radius, spread_radius, .. } => {
                let expand = blur_radius.abs() + spread_radius.max(0.0) + pad;
                Some(Rect {
                    x: rect.x + offset_x - expand,
                    y: rect.y + offset_y - expand,
                    width: rect.width + expand * 2.0,
                    height: rect.height + expand * 2.0,
                })
            }
            // A box is drawn inside its rect, anti-aliased edges included;
            // a conic gradient often only in some sectors of it.
            DrawCommand::DrawRect { rect, color, gradient, border, .. } => {
                let outer = Rect { x: rect.x - 1.0, y: rect.y - 1.0, width: rect.width + 2.0, height: rect.height + 2.0 };
                match gradient {
                    Some(Gradient::Conic(g)) if color.is_none_or(|c| c.a == 0) && !border.is_visible() => {
                        Some(g.painted_bounds(rect).and_then(|r| r.intersect(&outer)).unwrap_or(Rect::new(rect.x, rect.y, 0.0, 0.0)))
                    }
                    _ => Some(outer),
                }
            }
            DrawCommand::DrawText { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawImage { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawCheckMark { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawCanvas { rect, .. } => Some(apply_pad(*rect)),
        }
    }
}

/// Trait defining a text dimensions measurement mechanism.
pub trait TextMeasurer {
    /// Measures the width and height of a string when rendered with specified font parameters.
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32);

    /// Shapes `text` into glyphs with advances, which layout breaks into
    /// lines and renderers draw.
    ///
    /// The default treats every character as one glyph (id 0) as wide as
    /// [`measure_text`](Self::measure_text) says, with the measured height
    /// as ascent. That is enough for layout with measurers that have no
    /// fonts; real renderers implement this to get glyph ids and kerning,
    /// and to apply `features`.
    fn shape(&self, text: &str, font_size: f32, weight: u16, features: crate::text::FontFeatures) -> Arc<crate::text::ShapedText> {
        let _ = features;
        let mut buf = [0u8; 4];
        let glyphs: Vec<_> = text
            .char_indices()
            .filter(|(_, c)| !c.is_control())
            .map(|(i, c)| crate::text::ShapedGlyph {
                cluster: i as u32,
                font: 0,
                id: 0,
                advance: self.measure_text(c.encode_utf8(&mut buf), font_size, weight).0,
            })
            .collect();
        let ascent = self.measure_text("x", font_size, weight).1;
        let metrics = crate::text::LineMetrics { ascent, descent: 0.0, line_gap: 0.0 };
        Arc::new(crate::text::ShapedText::new(text, glyphs, metrics))
    }
}

/// Read-only pixel resources a renderer may draw from. Renderers never load anything
/// themselves; missing images are drawn as placeholders.
#[derive(Clone, Copy)]
pub struct RenderResources<'a> {
    /// Canvas pixel buffers by canvas id.
    pub canvases: &'a HashMap<String, Canvas>,
    /// Decoded images by `src`.
    pub images: &'a crate::images::ImageStore,
}

impl<'a> RenderResources<'a> {
    /// Bundle canvases and images for a render pass.
    pub fn new(canvases: &'a HashMap<String, Canvas>, images: &'a crate::images::ImageStore) -> Self {
        Self { canvases, images }
    }
}

/// Pixel storage of a [`Surface`]. More formats (RGB565) will be added.
#[non_exhaustive]
pub enum Pixels<'a> {
    /// 32-bit pixels `0xXXRRGGBB`, blue in the low byte: DRM/fbdev `XRGB8888`, softbuffer.
    Xrgb8888(&'a mut [u32]),
    /// 32-bit pixels `0xXXBBGGRR`, red in the low byte.
    Xbgr8888(&'a mut [u32]),
}

/// How the display is mounted relative to the UI, clockwise.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    /// Upright.
    #[default]
    Deg0,
    /// Rotated a quarter turn clockwise.
    Deg90,
    /// Upside down.
    Deg180,
    /// Rotated a quarter turn counterclockwise.
    Deg270,
}

impl Rotation {
    /// From degrees (0, 90, 180, 270); `None` for any other angle.
    pub fn from_degrees(degrees: u32) -> Option<Self> {
        match degrees {
            0 => Some(Self::Deg0),
            90 => Some(Self::Deg90),
            180 => Some(Self::Deg180),
            270 => Some(Self::Deg270),
            _ => None,
        }
    }

    /// The angle in degrees.
    pub fn degrees(self) -> u32 {
        match self {
            Self::Deg0 => 0,
            Self::Deg90 => 90,
            Self::Deg180 => 180,
            Self::Deg270 => 270,
        }
    }

    /// Copies `area` of an upright `width` x `height` frame into `display`,
    /// the display's packed rows, turned as the display is mounted, and
    /// converts each pixel with `convert` (to the display's format).
    ///
    /// Drawing into an upright frame and turning it here, once, is much
    /// cheaper than drawing turned: a renderer's rows then run along memory,
    /// where a turned display has them a whole row apart.
    pub fn copy_frame<T>(self, frame: &[u32], width: u32, height: u32, area: Rect, display: &mut [T], convert: impl Fn(u32) -> T) {
        let (w, h) = (width as usize, height as usize);
        let Some(area) = area.round_out().intersect(&Rect::new(0.0, 0.0, width as f32, height as f32)) else {
            return;
        };
        let (x0, y0) = (area.x as usize, area.y as usize);
        let (x1, y1) = (x0 + area.width as usize, y0 + area.height as usize);
        assert!(frame.len() >= w * h && display.len() >= w * h, "a {width}x{height} frame and display");
        match self {
            Self::Deg0 => {
                for y in y0..y1 {
                    let (src, dst) = (&frame[y * w + x0..y * w + x1], &mut display[y * w + x0..y * w + x1]);
                    dst.iter_mut().zip(src).for_each(|(d, &s)| *d = convert(s));
                }
            }
            Self::Deg180 => {
                for y in y0..y1 {
                    let row = (h - 1 - y) * w;
                    let dst = &mut display[row + w - x1..row + w - x0];
                    dst.iter_mut().zip(frame[y * w + x0..y * w + x1].iter().rev()).for_each(|(d, &s)| *d = convert(s));
                }
            }
            // A frame row becomes a display column: copy in square tiles so
            // that reads and writes both stay within a few cache lines.
            Self::Deg90 | Self::Deg270 => {
                const TILE: usize = 16;
                // Display rows are `h` pixels long; frame pixel (x, y) lands
                // at display row `x` (90) or `w - 1 - x` (270), column
                // `h - 1 - y` (90) or `y` (270).
                for ty in (y0..y1).step_by(TILE) {
                    let ty1 = (ty + TILE).min(y1);
                    for tx in (x0..x1).step_by(TILE) {
                        for x in tx..(tx + TILE).min(x1) {
                            let row = if self == Self::Deg90 { x } else { w - 1 - x } * h;
                            // Frame column `x`, rows `ty..ty1`.
                            let mut src = ty * w + x;
                            let mut next = || {
                                // SAFETY: `src` stays below `y1 * w <= frame.len()`
                                // (asserted above): `x < w` and `y < y1 <= h`.
                                let pixel = unsafe { *frame.get_unchecked(src) };
                                src += w;
                                convert(pixel)
                            };
                            if self == Self::Deg90 {
                                display[row + h - ty1..row + h - ty].iter_mut().rev().for_each(|d| *d = next());
                            } else {
                                display[row + ty..row + ty1].iter_mut().for_each(|d| *d = next());
                            }
                        }
                    }
                }
            }
        }
    }
}

/// The pixels a [`Renderer`] draws into: the whole screen, or a band of it
/// (see [`Surface::with_origin`]) when memory is too short for a full frame.
pub struct Surface<'a> {
    pixels: Pixels<'a>,
    width: u32,
    height: u32,
    stride: usize,
    origin: (i32, i32),
    rotation: Rotation,
}

impl<'a> Surface<'a> {
    /// A buffer of `width` x `height` pixels in rows of `width` pixels,
    /// showing the screen from its top-left corner.
    ///
    /// # Panics
    /// If `pixels` holds fewer than `width * height` pixels.
    pub fn new(pixels: Pixels<'a>, width: u32, height: u32) -> Self {
        let surface = Self { pixels, width, height, stride: width as usize, origin: (0, 0), rotation: Rotation::Deg0 };
        surface.check_len();
        surface
    }

    /// Rows are `stride` pixels apart in memory (a display's pitch), for
    /// unrotated surfaces.
    ///
    /// # Panics
    /// If `stride` is less than the width, the surface is rotated, or the
    /// buffer is too short.
    pub fn with_stride(mut self, stride: usize) -> Self {
        assert!(stride >= self.width as usize, "stride {stride} is less than the width {}", self.width);
        assert!(self.rotation == Rotation::Deg0, "rotated surfaces must have packed rows");
        self.stride = stride;
        self.check_len();
        self
    }

    /// Pixel (0, 0) shows the screen point (`x`, `y`): the surface is a band
    /// or tile of a larger screen, as when rendering in strips.
    pub fn with_origin(mut self, x: i32, y: i32) -> Self {
        self.origin = (x, y);
        self
    }

    /// The display is mounted rotated. `width` and `height` stay the size of
    /// the UI; the buffer holds rows of the display, packed (a quarter turn
    /// makes them `height` pixels long).
    pub fn with_rotation(mut self, rotation: Rotation) -> Self {
        self.rotation = rotation;
        self.stride = match rotation {
            Rotation::Deg90 | Rotation::Deg270 => self.height as usize,
            Rotation::Deg0 | Rotation::Deg180 => self.width as usize,
        };
        self.check_len();
        self
    }

    fn check_len(&self) {
        // Rows and pixels per row as stored, which a quarter turn swaps.
        let (rows, columns) = match self.rotation {
            Rotation::Deg90 | Rotation::Deg270 => (self.width, self.height),
            Rotation::Deg0 | Rotation::Deg180 => (self.height, self.width),
        };
        let needed = if rows == 0 { 0 } else { (rows as usize - 1) * self.stride + columns as usize };
        let len = match &self.pixels {
            Pixels::Xrgb8888(p) | Pixels::Xbgr8888(p) => p.len(),
        };
        assert!(len >= needed, "{len} pixels are too few for a {}x{} surface", self.width, self.height);
    }

    /// Width in UI pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in UI pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Pixels from one row to the next in memory.
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// The screen point shown by pixel (0, 0).
    pub fn origin(&self) -> (i32, i32) {
        self.origin
    }

    /// How the display is mounted.
    pub fn rotation(&self) -> Rotation {
        self.rotation
    }

    /// The part of the screen this surface shows.
    pub fn area(&self) -> Rect {
        Rect::new(self.origin.0 as f32, self.origin.1 as f32, self.width as f32, self.height as f32)
    }

    /// The pixel buffer.
    pub fn pixels_mut(&mut self) -> &mut Pixels<'a> {
        &mut self.pixels
    }
}

/// Draws [`DrawCommand`]s into a [`Surface`].
pub trait Renderer {
    /// Draws `commands` into `target`, changing only pixels inside `area`
    /// (screen coordinates). For a full repaint, pass `target.area()`.
    fn render(&mut self, target: &mut Surface<'_>, commands: &[DrawCommand], resources: &RenderResources<'_>, area: Rect);

    /// Starts or stops timing each kind of draw command, for
    /// [`take_profile`](Self::take_profile). The runtime turns it on for
    /// `XERUNE_PROFILE=1`. Renderers that can't measure ignore it.
    fn set_profiling(&mut self, _on: bool) {}

    /// The time spent per kind of draw command since the last call, while
    /// profiling. Empty when the renderer doesn't measure.
    fn take_profile(&mut self) -> Vec<CommandProfile> {
        Vec::new()
    }
}

/// The time a [`Renderer`] spent on one kind of draw command.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CommandProfile {
    /// The kind, e.g. `"rect"` or `"text"`.
    pub kind: &'static str,
    /// Commands of this kind drawn.
    pub count: u32,
    /// The time they took.
    pub time: core::time::Duration,
}

impl CommandProfile {
    /// `count` commands of `kind` that took `time`.
    pub fn new(kind: &'static str, count: u32, time: core::time::Duration) -> Self {
        Self { kind, count, time }
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, ConicGradient, DrawCommand, Gradient, Rect};

    fn conic(from: f32, stops: &[(Color, f32)]) -> DrawCommand {
        DrawCommand::DrawRect {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: None,
            gradient: Some(Gradient::Conic(ConicGradient { from, center: (0.5, 0.5), stops: stops.into(), repeating: false })),
            border_radius: 50.0,
            border: crate::style::Borders::NONE,
        }
    }

    /// A turned copy puts each pixel where a renderer drawing turned would
    /// (`xerune_render`'s `calc_pixel_index`), and only inside the area.
    #[test]
    fn frames_turn_as_the_display_is_mounted() {
        use super::Rotation;
        let (w, h) = (37usize, 21usize);
        let frame: Vec<u32> = (0..(w * h) as u32).map(|i| i + 1).collect();
        let turned = |rotation: Rotation, x: usize, y: usize| match rotation {
            Rotation::Deg0 => y * w + x,
            Rotation::Deg90 => x * h + (h - 1 - y),
            Rotation::Deg180 => (h - 1 - y) * w + (w - 1 - x),
            Rotation::Deg270 => (w - 1 - x) * h + y,
        };
        for rotation in [Rotation::Deg0, Rotation::Deg90, Rotation::Deg180, Rotation::Deg270] {
            let mut display = vec![0u32; w * h];
            rotation.copy_frame(&frame, w as u32, h as u32, Rect::new(0.0, 0.0, w as f32, h as f32), &mut display, |p| p);
            for (y, x) in (0..h).flat_map(|y| (0..w).map(move |x| (y, x))) {
                assert_eq!(display[turned(rotation, x, y)], frame[y * w + x], "{rotation:?} ({x}, {y})");
            }
            let mut display = vec![0u32; w * h];
            let area = Rect::new(3.5, 2.0, 20.0, 17.25);
            rotation.copy_frame(&frame, w as u32, h as u32, area, &mut display, |p| p);
            for (y, x) in (0..h).flat_map(|y| (0..w).map(move |x| (y, x))) {
                let inside = (3..24).contains(&x) && (2..20).contains(&y);
                assert_eq!(display[turned(rotation, x, y)], if inside { frame[y * w + x] } else { 0 }, "{rotation:?} ({x}, {y})");
            }
        }
    }

    const WHITE: Color = Color { r: 255, g: 255, b: 255, a: 255 };
    const CLEAR: Color = Color { r: 0, g: 0, b: 0, a: 0 };

    /// A gauge needle, `conic-gradient(from 90deg, white 0deg 3deg,
    /// transparent 3deg)`, changes a thin box, not the whole dial.
    #[test]
    fn a_conic_wedge_is_bounded_by_its_sector() {
        let bounds = conic(90.0, &[(WHITE, 0.0), (WHITE, 3.0 / 360.0), (CLEAR, 3.0 / 360.0)]).bounds().unwrap();
        // From the center to the right edge, 50 * tan(3deg) = 2.6 px down.
        assert!(bounds.x <= 48.0 && bounds.x + bounds.width >= 100.0, "{bounds:?}");
        assert!(bounds.y <= 48.0 && bounds.y + bounds.height >= 52.6 && bounds.height < 10.0, "{bounds:?}");
    }

    /// Corners between a sector's edges are in its box; transparent stops
    /// paint nothing.
    #[test]
    fn a_conic_sector_includes_its_corners() {
        // 12 o'clock to 6 o'clock clockwise: the right half.
        let half = conic(0.0, &[(WHITE, 0.0), (WHITE, 0.5), (CLEAR, 0.5)]).bounds().unwrap();
        assert!(half.x <= 48.0 && half.x >= 45.0 && half.width >= 52.0, "{half:?}");
        assert!(half.y <= 0.0 && half.y + half.height >= 100.0, "{half:?}");
        // Transparent until 3 o'clock, then painted into the top-left corner.
        let wrap = conic(0.0, &[(CLEAR, 0.25), (WHITE, 0.25)]).bounds().unwrap();
        assert!(wrap.x <= 0.0 && wrap.y <= 0.0 && wrap.x + wrap.width >= 100.0 && wrap.y + wrap.height >= 100.0, "{wrap:?}");
        let none = conic(0.0, &[(CLEAR, 0.0), (CLEAR, 1.0)]).bounds().unwrap();
        assert!(none.is_empty(), "{none:?}");
    }

    #[test]
    fn empty_rects_do_not_grow_a_union() {
        let a = Rect::new(10.0, 10.0, 5.0, 5.0);
        let empty = Rect::new(0.0, 0.0, 0.0, 0.0);
        assert_eq!(a.expand(empty), a);
        assert_eq!(empty.expand(a), a);
        assert_eq!(a.expand(Rect::new(20.0, 0.0, 1.0, 1.0)), Rect::new(10.0, 0.0, 11.0, 15.0));
    }

    #[test]
    fn round_out_covers_partial_pixels() {
        assert_eq!(Rect::new(1.5, 2.25, 3.0, 1.0).round_out(), Rect::new(1.0, 2.0, 4.0, 2.0));
        assert_eq!(Rect::new(-1.5, -0.5, 1.0, 1.0).round_out(), Rect::new(-2.0, -1.0, 2.0, 2.0));
        assert_eq!(Rect::new(3.0, 4.0, 5.0, 6.0).round_out(), Rect::new(3.0, 4.0, 5.0, 6.0));
    }
}
