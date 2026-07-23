use crate::graphics::TextMeasurer;

/// A dummy text measurer for the browser environment since text layout is handled natively by the DOM.
#[derive(Clone, Copy)]
pub struct DummyTextMeasurer;

impl TextMeasurer for DummyTextMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (0.0, 0.0)
    }
}

/// Helper macro to export a Model and Message type to a wasm-bindgen compatible BrowserApp wrapper.
#[macro_export]
macro_rules! export_browser_app {
    ($model_type:ty, $msg_type:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen]
        pub struct BrowserApp {
            runtime: $crate::runtime::Runtime<$model_type, $crate::backend::browser_impl::DummyTextMeasurer>,
        }

        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen]
        impl BrowserApp {
            #[wasm_bindgen::prelude::wasm_bindgen(constructor)]
            pub fn new(initial_state_json: &str) -> Self {
                #[cfg(target_arch = "wasm32")]
                console_error_panic_hook::set_once();
                
                // Initialize a logger if console_log is available
                // console_log::init_with_level(log::Level::Debug).ok();
                
                let model: $model_type = if initial_state_json.is_empty() {
                    <$model_type>::default()
                } else {
                    serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
                };
                
                let measurer = $crate::backend::browser_impl::DummyTextMeasurer;
                let runtime = $crate::runtime::Runtime::new(model, measurer);
                Self { runtime }
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn update(&mut self, msg_str: &str) -> bool {
                self.runtime.handle_event($crate::model::InputEvent::Message(msg_str.to_string()))
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn tick(&mut self, now_ms: f64) -> bool {
                let res = self.runtime.tick_at_ms(now_ms as u64);
                res.needs_redraw
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn get_state_json(&self) -> String {
                serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
                self.runtime.context().canvases.get(id).map(|c| c.data.clone())
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn get_canvas_width(&self, id: &str) -> u32 {
                self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
            }

            #[wasm_bindgen::prelude::wasm_bindgen]
            pub fn get_canvas_height(&self, id: &str) -> u32 {
                self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
            }
        }
    };
}
