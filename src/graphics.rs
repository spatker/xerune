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

/// Any CSS gradient image usable as a background.
#[derive(Debug, Clone, PartialEq)]
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
        // `f32::floor` needs std; screen coordinates fit an i32.
        let floor = |v: f32| {
            let t = v as i32 as f32;
            if t > v { t - 1.0 } else { t }
        };
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
pub enum ContextCommand {
    /// Scroll the viewport to ensure a specific interaction element is visible.
    ScrollIntoView(String),
    /// Clear an active timer by message name.
    ClearTimer(String),
}

/// The context containing canvases and commands passed to Model updates.
pub struct Context {
    /// Map of canvas IDs to their respective Canvas pixel buffers.
    pub canvases: HashMap<String, Canvas>,
    pub(crate) commands: Vec<ContextCommand>,
    pub(crate) pending_timers: Vec<crate::runtime::Timer>,
}

impl Context {
    /// Create a new, empty context.
    pub fn new() -> Self {
        Self {
            canvases: HashMap::new(),
            commands: Vec::new(),
            pending_timers: Vec::new(),
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

    /// Schedule a command to scroll the viewport to show a specific interactive element.
    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.commands.push(ContextCommand::ScrollIntoView(interaction_id.to_string()));
    }

    /// Clear an active recurring or timeout timer by message string.
    pub fn clear_interval(&mut self, message: &str) {
        self.commands.push(ContextCommand::ClearTimer(message.to_string()));
    }

    /// Clear an active timeout timer by message string.
    pub fn clear_timeout(&mut self, message: &str) {
        self.commands.push(ContextCommand::ClearTimer(message.to_string()));
    }

    /// Set an interval timer that regularly triggers a message at the specified milliseconds.
    pub fn set_interval(&mut self, message: String, millis: u32) {
        let duration = core::time::Duration::from_millis(millis as u64);
        self.pending_timers.push(crate::runtime::Timer {
            id: 0,
            message,
            interval: duration,
            // Set by the runtime when it registers the timer at the next tick.
            next_trigger: crate::runtime::time::Instant::now(),
            is_recurring: true,
        });
    }

    /// Set a one-shot timeout timer that triggers a message after the specified milliseconds.
    pub fn set_timeout(&mut self, message: String, millis: u32) {
        let duration = core::time::Duration::from_millis(millis as u64);
        self.pending_timers.push(crate::runtime::Timer {
            id: 0,
            message,
            interval: duration,
            // Set by the runtime when it registers the timer at the next tick.
            next_trigger: crate::runtime::time::Instant::now(),
            is_recurring: false,
        });
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

/// Commands defining hardware-agnostic layout drawing primitives.
#[derive(Clone, Debug, PartialEq)]
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
        /// renderer lays `text` out itself (text inputs).
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
    },
    /// Draw a custom style checkbox.
    DrawCheckbox {
        /// Bounding box.
        rect: Rect,
        /// Flag if the checkbox is checked.
        checked: bool,
        /// Theme color.
        color: Color,
    },
    /// Draw a slider control.
    DrawSlider {
        /// Bounding box.
        rect: Rect,
        /// Normalized slider value (0.0 to 1.0).
        value: f32,
        /// Theme color.
        color: Color,
    },

    /// Draw a progress bar control.
    DrawProgress {
        /// Bounding box.
        rect: Rect,
        /// Current value.
        value: f32,
        /// Maximum limit value.
        max: f32,
        /// Theme color.
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
    },
}

impl DrawCommand {
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
            DrawCommand::DrawRect { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawText { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawImage { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawCheckbox { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawSlider { rect, .. } => Some(apply_pad(*rect)),
            DrawCommand::DrawProgress { rect, .. } => Some(apply_pad(*rect)),
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
    /// fonts; real renderers implement this to get glyph ids and kerning.
    fn shape(&self, text: &str, font_size: f32, weight: u16) -> Arc<crate::text::ShapedText> {
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

/// Trait defining a renderer backend that executes draw commands to present graphics.
pub trait Renderer: TextMeasurer {
    /// Renders a set of draw commands onto the canvas/display buffer, optimizing for a dirty rectangle region.
    fn render(&mut self, commands: &[DrawCommand], resources: &RenderResources<'_>, dirty_rect: Option<Rect>);
}

#[cfg(test)]
mod tests {
    use super::Rect;

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
