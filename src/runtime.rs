use std::str::FromStr;
use taffy::prelude::*;
use std::collections::HashMap;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Context, DrawCommand, Rect, TextMeasurer, Renderer};
use crate::style::{ContainerStyle, RenderData};
use crate::model::{InputEvent, Model};
use crate::ui::Ui;

pub struct Runtime<M, R> {
    model: M,
    measurer: R,
    pub(crate) ui: Ui,
    default_style: ContainerStyle,
    pub(crate) scroll_offsets: HashMap<NodeId, (f32, f32)>, // Persist scroll offsets
    cached_size: Size<AvailableSpace>,
    context: Context,
    last_html: String,
    last_commands: Vec<DrawCommand>,
}

impl<M: Model, R: TextMeasurer> Runtime<M, R> {
    pub fn new(model: M, measurer: R) -> Self {
         let default_style = ContainerStyle::default();
         let html = model.view();
         let validator = |s: &str| M::Message::from_str(s).is_ok();
         let ui = Ui::new(&html, &measurer, default_style.clone(), &validator).unwrap();
         
         let mut context = Context::new();
         // Initial sync of canvases
         Runtime::<M, R>::sync_canvases(&ui, &mut context);

         Self {
             model,
             measurer,
             ui,
             default_style,
             scroll_offsets: HashMap::new(),
             cached_size: Size::MAX_CONTENT,
             context,
             last_html: html,
             last_commands: Vec::new(),
         }
    }

    fn sync_canvases(ui: &Ui, context: &mut Context) {
        for (node_id, data) in &ui.render_data {
            if let RenderData::Canvas(id, _style) = data {
                if !context.canvases.contains_key(id) {
                     // Try to get size from Taffy style (set by CSS or attributes)
                     let mut width = 200;
                     let mut height = 200;

                     if let Ok(style) = ui.taffy.style(*node_id) {
                         let w_dim = style.size.width;
                         if !w_dim.is_auto() {
                             let val = w_dim.value();
                             if w_dim == Dimension::length(val) {
                                  width = val as u32;
                             }
                         }

                         let h_dim = style.size.height;
                         if !h_dim.is_auto() {
                             let val = h_dim.value();
                             if h_dim == Dimension::length(val) {
                                 height = val as u32;
                             }
                         }
                     }

                     context.canvases.insert(id.clone(), crate::graphics::Canvas::new(width, height));
                }
            }
        }
    }

    fn restore_scroll(&mut self) {
        self.ui.scroll_offsets = self.scroll_offsets.clone();
    }

    pub fn handle_event(&mut self, event: InputEvent) -> bool {
        match event {
            InputEvent::Click { x, y } => {
                if let Some(msg_str) = self.ui.hit_test(x, y) {
                    return self.process_message_str(&msg_str);
                }
                return false; // Should return false if not handled or empty
            }
            InputEvent::Message(msg_str) => {
                self.process_message_str(&msg_str)
            }
            InputEvent::Scroll { x, y, delta_x, delta_y } => {
                if self.ui.handle_scroll(x, y, delta_x, delta_y) {
                    self.scroll_offsets = self.ui.scroll_offsets.clone();
                    return true;
                }
                false
            }
            InputEvent::KeyDown(key) => {
                let msg_str = format!("keydown:{}", key);
                self.process_message_str(&msg_str)
            }
            InputEvent::KeyUp(key) => {
                let msg_str = format!("keyup:{}", key);
                self.process_message_str(&msg_str)
            }
             _ => false
        }
    }
    
    fn process_message_str(&mut self, msg_str: &str) -> bool {
        if let Ok(msg) = M::Message::from_str(msg_str) {
            {
                profile!("update");
                self.model.update(msg, &mut self.context);
            }
            let html = {
                profile!("view");
                self.model.view()
            };
            
            let mut dirty = false;
            
            // Optimization: Only rebuild UI if HTML changed
            if html != self.last_html {
                self.last_html = html.clone();
                // Recreate UI to reflect changes
                self.ui = {
                    profile!("ui_new");
                    let validator = |s: &str| M::Message::from_str(s).is_ok();
                    Ui::new(&html, &self.measurer, self.default_style.clone(), &validator).unwrap()
                };
                {
                    profile!("compute_layout");
                    let _ = self.ui.compute_layout(self.cached_size);
                }
                Runtime::<M, R>::sync_canvases(&self.ui, &mut self.context);
                self.restore_scroll();
                dirty = true;
            }

            let commands: Vec<_> = self.context.commands.drain(..).collect();
            for cmd in commands {
                match cmd {
                    crate::graphics::ContextCommand::ScrollIntoView(id) => {
                        self.scroll_into_view(&id);
                        dirty = true;
                    }
                }
            }

            if !dirty {
                // Only trigger redraw if a *visible* canvas is dirty
                for cmd in &self.last_commands {
                    if let DrawCommand::DrawCanvas { id, .. } = cmd {
                        if let Some(canvas) = self.context.canvases.get(id) {
                            if canvas.dirty {
                                dirty = true;
                                break; // We DO NOT reset canvas.dirty here!
                            }
                        }
                    }
                }
            }

            dirty
        } else {
            log::debug!("Unhandled or failed to parse message: {}", msg_str);
            false
        }
    }
    
    pub fn render(&mut self, renderer: &mut impl Renderer) {
        profile!("render");
        let commands = self.ui.build_commands(&self.context.canvases);
        
        let mut dirty_region: Option<Rect> = None;

        // Compare with last_commands
        let max_len = commands.len().max(self.last_commands.len());
        for i in 0..max_len {
            let cmd1 = commands.get(i);
            let cmd2 = self.last_commands.get(i);

            if cmd1 != cmd2 {
                if let Some(cmd) = cmd1 {
                    if let Some(b) = cmd.bounds() {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(b)),
                            None => Some(b),
                        };
                    }
                }
                if let Some(cmd) = cmd2 {
                    if let Some(b) = cmd.bounds() {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(b)),
                            None => Some(b),
                        };
                    }
                }
            }
        }

        // Also invalidate for dirty canvases
        for cmd in &commands {
            if let DrawCommand::DrawCanvas { id, rect } = cmd {
                if let Some(canvas) = self.context.canvases.get(id) {
                    if canvas.dirty {
                        dirty_region = match dirty_region {
                            Some(dr) => Some(dr.expand(*rect)),
                            None => Some(*rect),
                        };
                    }
                }
            }
        }

        // After expanding dirty_region bounds to cover the changes,
        // reset the canvas dirty flags.
        for canvas in self.context.canvases.values_mut() {
            canvas.dirty = false;
        }

        renderer.render(&commands, &self.context.canvases, dirty_region);
        self.last_commands = commands;
    }

    pub fn set_size(&mut self, width: f32, height: f32) {
         let _ = self.ui.compute_layout(Size {
            width: length(width),
             height: length(height),
         });
    }

    pub fn compute_layout(&mut self, size: Size<AvailableSpace>) {
        self.cached_size = size;
        let _ = self.ui.compute_layout(size);
    }
    
    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        self.ui.scroll_into_view(interaction_id);
        self.scroll_offsets = self.ui.scroll_offsets.clone();
    }
}
