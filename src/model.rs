use crate::graphics::Context;
use crate::alloc_prelude::*;

pub trait Model {
    type Message: core::str::FromStr + Send + Sync + 'static;
    fn view(&self) -> String {
        String::new()
    }
    fn update(&mut self, msg: Self::Message, context: &mut Context);
}

pub enum InputEvent {
    Click { x: f32, y: f32 },
    Hover { x: f32, y: f32 },
    Scroll { x: f32, y: f32, delta_x: f32, delta_y: f32 },
    KeyDown(String),
    KeyUp(String),
    Message(String),
    TextInput { id: String, text: String },
}
