//! The examples in the browser, for the studio (`studio/index.html`) and
//! standalone pages (`examples/standalone.html`); see `xerune.js`.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[path = "../../examples/todo/todo_impl.rs"]
pub mod todo_impl;

#[path = "../../examples/calculator/calculator_impl.rs"]
pub mod calculator_impl;

#[path = "../../examples/music_player/music_player_impl.rs"]
pub mod music_player_impl;

#[path = "../../examples/animation/animation_impl.rs"]
pub mod animation_impl;

#[path = "../../examples/animation_css/animation_css_impl.rs"]
pub mod animation_css_impl;

#[path = "../../examples/showcase/showcase_impl.rs"]
pub mod showcase_impl;

#[path = "../../examples/breakout/breakout_impl.rs"]
pub mod breakout_impl;

/// The model in `json` (a saved state), or `default` when it is empty.
#[cfg(target_arch = "wasm32")]
fn model_or<M: serde::de::DeserializeOwned>(json: &str, default: impl FnOnce() -> M) -> M {
    if json.is_empty() { default() } else { serde_json::from_str(json).expect("Failed to deserialize initial state") }
}

xerune::export_browser_app!(TodoApp, todo_impl::TodoList);
xerune::export_browser_app!(CalculatorApp, calculator_impl::CalculatorModel);
xerune::export_browser_app!(MusicPlayerApp, music_player_impl::MusicPlayerModel);
xerune::export_browser_app!(AnimationApp, animation_impl::AnimationModel);
xerune::export_browser_app!(AnimationCssApp, animation_css_impl::AnimationCssModel);
xerune::export_browser_app!(ShowcaseApp, showcase_impl::ShowcaseModel);
xerune::export_browser_app!(BreakoutApp, breakout_impl::BreakoutModel);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl TodoApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, todo_impl::TodoList::default))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl CalculatorApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, calculator_impl::CalculatorModel::default))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl MusicPlayerApp {
    /// `tracks_json`: the track list (the page fetches it; there is no
    /// file system).
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str, tracks_json: &str) -> Self {
        let tracks = || music_player_impl::MusicPlayerModel::new((!tracks_json.is_empty()).then_some(tracks_json));
        let mut model = model_or(state_json, tracks);
        if model.tracks.is_empty() && !tracks_json.is_empty() {
            model = tracks();
        }
        Self::from_model(model)
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AnimationApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, || animation_impl::AnimationModel::new(100)))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AnimationCssApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, || animation_css_impl::AnimationCssModel))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl ShowcaseApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, showcase_impl::ShowcaseModel::default))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BreakoutApp {
    #[wasm_bindgen(constructor)]
    pub fn new(state_json: &str) -> Self {
        Self::from_model(model_or(state_json, breakout_impl::BreakoutModel::default))
    }
}
