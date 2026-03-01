use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: u8, 
    pub g: u8, 
    pub b: u8, 
    pub a: u8,
}

impl Color {
    pub const BLACK: Self = Self { r: 0, g: 0, b: 0, a: 255 };
    pub const WHITE: Self = Self { r: 255, g: 255, b: 255, a: 255 };
    
    pub fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

     pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LinearGradient {
    pub angle: f32, // in degrees
    pub stops: Vec<(Color, f32)>, // Color and position (0.0 to 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

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

    pub fn intersects(&self, other: &Rect) -> bool {
        !(self.x + self.width <= other.x
            || other.x + other.width <= self.x
            || self.y + self.height <= other.y
            || other.y + other.height <= self.y)
    }
}

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub dirty: bool,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
            dirty: true,
        }
    }
}

pub enum ContextCommand {
    ScrollIntoView(String),
}

pub struct Context {
    pub canvases: HashMap<String, Canvas>,
    pub(crate) commands: Vec<ContextCommand>,
}

impl Context {
    pub fn new() -> Self {
        Self {
            canvases: HashMap::new(),
            commands: Vec::new(),
        }
    }
    
    pub fn canvas_mut(&mut self, id: &str) -> Option<&mut Canvas> {
        self.canvases.get_mut(id)
    }

    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.commands.push(ContextCommand::ScrollIntoView(interaction_id.to_string()));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DrawCommand {
    Clip { rect: Rect },
    PopClip,
    DrawRect {
        rect: Rect,
        color: Option<Color>,
        gradient: Option<LinearGradient>,
        border_radius: f32,
        border_width: f32,
        border_color: Option<Color>,
    },
    DrawText { 
        text: String, 
        rect: Rect,
        color: Color, 
        font_size: f32,
        weight: u16,
    },
    DrawImage {
        src: String,
        rect: Rect,
        border_radius: f32,
    },
    DrawCheckbox {
        rect: Rect,
        checked: bool,
        color: Color,
    },
    DrawSlider {
        rect: Rect,
        value: f32,
        color: Color,
    },

    DrawProgress {
        rect: Rect,
        value: f32,
        max: f32,
        color: Color,
    },
    DrawCanvas {
        id: String,
        rect: Rect,
    },
}

impl DrawCommand {
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

pub trait TextMeasurer {
    fn measure_text(&self, text: &str, font_size: f32, weight: u16) -> (f32, f32);
}

pub trait Renderer: TextMeasurer {
    fn render(&mut self, commands: &[DrawCommand], canvases: &HashMap<String, Canvas>, dirty_rect: Option<Rect>);
}
