//! Running an app in a browser, for the studio and standalone pages
//! (`xerune.js`): the model runs in wasm, and its view is the template
//! interpreted into HTML (`xerune_template::render_html`), so template
//! edits show without rebuilding the wasm.

use std::collections::BTreeMap;

use crate::graphics::TextMeasurer;

/// A dummy text measurer for the browser environment since text layout is handled natively by the DOM.
#[derive(Clone, Copy)]
pub struct DummyTextMeasurer;

impl TextMeasurer for DummyTextMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (0.0, 0.0)
    }
}

/// Parses the template at `path` from `files_json`, a JSON object of the
/// template's files by path (the page fetched them: there is no file
/// system). Includes resolve relative to the including file.
pub fn parse_template(path: &str, files_json: &str) -> Result<xerune_template::Template, String> {
    let files: BTreeMap<String, String> = serde_json::from_str(files_json).map_err(|e| e.to_string())?;
    let file = |path: &str| -> Result<xerune_template::File, String> {
        let content = files.get(path).ok_or_else(|| format!("template `{path}` wasn't loaded"))?;
        Ok(xerune_template::File {
            name: path.to_string(),
            dir: std::path::PathBuf::from(path.rsplit_once('/').map_or("", |(dir, _)| dir)),
            content: content.clone(),
        })
    };
    xerune_template::parse(file(path)?, &mut |include: &str, dir: &std::path::Path| file(&join(dir, include)))
        .map_err(|e| e.to_string())
}

/// `dir/path`, with `.` and `..` resolved, as the page names the files.
fn join(dir: &std::path::Path, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in dir.to_str().unwrap_or("").split('/').chain(path.split('/')) {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    parts.join("/")
}

/// Exports a wasm-bindgen class `$name` that runs a `$model` in the
/// browser for `xerune.js`. The page hands it the template's files
/// (`set_template`); `view_html` is the template interpreted for the
/// current model, its elements keyed by `data-xk`; the elements' messages
/// (`data-on-click`, ...) come back as text through `update`.
///
/// The class gets everything but a constructor; build it with
/// `$name::from_model(model)` in a `#[wasm_bindgen(constructor)]` of your
/// own. The model must be `Serialize` (its data is what the template
/// reads) and use the `XeruneTemplate` derive.
#[macro_export]
macro_rules! export_browser_app {
    ($name:ident, $model:ty) => {
        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen]
        pub struct $name {
            runtime: $crate::runtime::Runtime<$model, $crate::backend::browser_impl::DummyTextMeasurer>,
            template: Option<$crate::__private::Template>,
        }

        #[cfg(target_arch = "wasm32")]
        impl $name {
            /// Runs `model`.
            pub fn from_model(model: $model) -> Self {
                console_error_panic_hook::set_once();
                let runtime = $crate::runtime::Runtime::new(model, $crate::backend::browser_impl::DummyTextMeasurer);
                Self { runtime, template: None }
            }
        }

        #[cfg(target_arch = "wasm32")]
        #[wasm_bindgen::prelude::wasm_bindgen]
        impl $name {
            /// Reads the template at `path` from `files_json` (its files
            /// by path). On an error the previous template stays.
            pub fn set_template(&mut self, path: &str, files_json: &str) -> Result<(), wasm_bindgen::JsValue> {
                let template = $crate::backend::browser_impl::parse_template(path, files_json)
                    .map_err(|e| wasm_bindgen::JsValue::from_str(&e))?;
                self.template = Some(template);
                Ok(())
            }

            /// The template's CSS.
            pub fn template_css(&self) -> String {
                self.template.as_ref().map(|t| t.css.clone()).unwrap_or_default()
            }

            /// The view as HTML: a `<body>` with its content, elements keyed
            /// by `data-xk`.
            pub fn view_html(&self) -> Result<String, wasm_bindgen::JsValue> {
                let template = self.template.as_ref().ok_or_else(|| wasm_bindgen::JsValue::from_str("no template"))?;
                let model = $crate::__private::to_value(self.runtime.model()).map_err(|e| wasm_bindgen::JsValue::from_str(&e))?;
                $crate::__private::render_html(template, &model, self.runtime.model())
                    .map_err(|e| wasm_bindgen::JsValue::from_str(&e.to_string()))
            }

            /// Sends a message given as text (from `data-on-click`, keys, text input).
            pub fn update(&mut self, message: &str) -> bool {
                self.runtime.handle_event($crate::model::InputEvent::Message(message.to_string()))
            }

            /// Timers, subscriptions and frames; true if the view changed.
            pub fn poll(&mut self) -> bool {
                self.runtime.poll().redraw
            }

            /// The page's viewport size, for `on_resize` and `@media`.
            pub fn set_size(&mut self, width: f32, height: f32) -> bool {
                self.runtime.set_size(width, height)
            }

            pub fn get_state_json(&self) -> String {
                serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
            }

            pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
                self.runtime.context().canvases.get(id).map(|c| c.data.clone())
            }

            pub fn get_canvas_width(&self, id: &str) -> u32 {
                self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
            }

            pub fn get_canvas_height(&self, id: &str) -> u32 {
                self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
            }
        }
    };
}
