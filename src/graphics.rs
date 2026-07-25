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

    /// Expands this rectangle to encompass another rectangle.
    pub fn expand(&self, other: Rect) -> Rect {
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

    /// Schedule a command to scroll the viewport to show a specific interactive element.
    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.commands.push(ContextCommand::ScrollIntoView(interaction_id.to_string()));
    }

    /// Set an interval timer that regularly triggers a message at the specified milliseconds.
    pub fn set_interval(&mut self, message: String, millis: u32) {
        let duration = core::time::Duration::from_millis(millis as u64);
        self.pending_timers.push(crate::runtime::Timer {
            id: 0,
            message,
            interval: duration,
            next_trigger: crate::runtime::time::Instant::now() + duration,
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
            next_trigger: crate::runtime::time::Instant::now() + duration,
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
        /// Optional linear gradient fill.
        gradient: Option<LinearGradient>,
        /// Border corner radius.
        border_radius: f32,
        /// Border line width.
        border_width: f32,
        /// Border outline color.
        border_color: Option<Color>,
        /// Border line style.
        border_style: crate::style::BorderStyle,
        /// Flag indicating bottom-only border.
        border_bottom_only: bool,
    },
    /// Draw a single-line text string.
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
}

/// Trait defining a renderer backend that executes draw commands to present graphics.
pub trait Renderer: TextMeasurer {
    /// Renders a set of draw commands onto the canvas/display buffer, optimizing for a dirty rectangle region.
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>, dirty_rect: Option<Rect>);
}
