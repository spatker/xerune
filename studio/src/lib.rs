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

// 1. Todo App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct TodoApp {
    runtime: xerune::runtime::Runtime<todo_impl::TodoList, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl TodoApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: todo_impl::TodoList = if initial_state_json.is_empty() {
            todo_impl::TodoList::default()
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let runtime = xerune::runtime::Runtime::new(model, measurer);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 2. Calculator App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct CalculatorApp {
    runtime: xerune::runtime::Runtime<calculator_impl::CalculatorModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl CalculatorApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: calculator_impl::CalculatorModel = if initial_state_json.is_empty() {
            calculator_impl::CalculatorModel::default()
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let runtime = xerune::runtime::Runtime::new(model, measurer);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 3. Music Player App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct MusicPlayerApp {
    runtime: xerune::runtime::Runtime<music_player_impl::MusicPlayerModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl MusicPlayerApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str, tracks_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let mut model: music_player_impl::MusicPlayerModel = if initial_state_json.is_empty() {
            music_player_impl::MusicPlayerModel::new(
                if tracks_json.is_empty() { None } else { Some(tracks_json) }
            )
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        if model.tracks.is_empty() && !tracks_json.is_empty() {
            model = music_player_impl::MusicPlayerModel::new(Some(tracks_json));
        }

        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let mut runtime = xerune::runtime::Runtime::new(model, measurer);
        runtime.set_interval("tick".to_string(), 33);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let old_model = self.runtime.model().clone();
        let res = self.runtime.tick_at_ms(now_ms as u64);
        let model_changed = self.runtime.model() != &old_model;
        if res.needs_redraw && model_changed {
            let m = self.runtime.model();
            if m.is_playing != old_model.is_playing {
                log::info!("is_playing changed");
            }
            if m.elapsed_seconds != old_model.elapsed_seconds {
                log::info!("elapsed_seconds changed");
            }
            if m.transition_progress != old_model.transition_progress {
                log::info!("transition_progress changed: {} -> {}", old_model.transition_progress, m.transition_progress);
            }
            if m.visualizer_data != old_model.visualizer_data {
                log::info!("visualizer_data changed");
            }
            if m.list_x != old_model.list_x {
                log::info!("list_x changed: {} -> {}", old_model.list_x, m.list_x);
            }
            if m.player_x != old_model.player_x {
                log::info!("player_x changed: {} -> {}", old_model.player_x, m.player_x);
            }
            if m.current_track_index != old_model.current_track_index {
                log::info!("current_track_index changed");
            }
            if m.hovered_track != old_model.hovered_track {
                log::info!("hovered_track changed");
            }
            if m.active_list_index != old_model.active_list_index {
                log::info!("active_list_index changed");
            }
        }
        res.needs_redraw && model_changed
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 4. Animation App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct AnimationApp {
    runtime: xerune::runtime::Runtime<animation_impl::AnimationModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AnimationApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: animation_impl::AnimationModel = if initial_state_json.is_empty() {
            animation_impl::AnimationModel::new(100)
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let mut runtime = xerune::runtime::Runtime::new(model, measurer);
        runtime.set_interval("tick".to_string(), 16);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 5. CSS Animation App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct AnimationCssApp {
    runtime: xerune::runtime::Runtime<animation_css_impl::AnimationCssModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl AnimationCssApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: animation_css_impl::AnimationCssModel = if initial_state_json.is_empty() {
            animation_css_impl::AnimationCssModel
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let runtime = xerune::runtime::Runtime::new(model, measurer);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 6. Showcase App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct ShowcaseApp {
    runtime: xerune::runtime::Runtime<showcase_impl::ShowcaseModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl ShowcaseApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: showcase_impl::ShowcaseModel = if initial_state_json.is_empty() {
            showcase_impl::ShowcaseModel::default()
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let mut runtime = xerune::runtime::Runtime::new(model, measurer);
        runtime.set_interval("tick".to_string(), 200);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}

// 7. Breakout App WASM Binding
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub struct BreakoutApp {
    runtime: xerune::runtime::Runtime<breakout_impl::BreakoutModel, xerune::backend::browser_impl::DummyTextMeasurer>,
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
impl BreakoutApp {
    #[wasm_bindgen(constructor)]
    pub fn new(initial_state_json: &str) -> Self {
        console_error_panic_hook::set_once();
        
        let model: breakout_impl::BreakoutModel = if initial_state_json.is_empty() {
            breakout_impl::BreakoutModel::default()
        } else {
            serde_json::from_str(initial_state_json).expect("Failed to deserialize initial state")
        };
        
        let measurer = xerune::backend::browser_impl::DummyTextMeasurer;
        let mut runtime = xerune::runtime::Runtime::new(model, measurer);
        runtime.set_interval("tick".to_string(), 16);
        Self { runtime }
    }

    #[wasm_bindgen]
    pub fn update(&mut self, msg_str: &str) -> bool {
        self.runtime.handle_event(xerune::model::InputEvent::Message(msg_str.to_string()))
    }

    #[wasm_bindgen]
    pub fn tick(&mut self, now_ms: f64) -> bool {
        let res = self.runtime.tick_at_ms(now_ms as u64);
        res.needs_redraw
    }

    #[wasm_bindgen]
    pub fn get_state_json(&self) -> String {
        serde_json::to_string(self.runtime.model()).expect("Failed to serialize model")
    }

    #[wasm_bindgen]
    pub fn get_canvas_pixels(&self, id: &str) -> Option<Vec<u8>> {
        self.runtime.context().canvases.get(id).map(|c| c.data.clone())
    }

    #[wasm_bindgen]
    pub fn get_canvas_width(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.width).unwrap_or(0)
    }

    #[wasm_bindgen]
    pub fn get_canvas_height(&self, id: &str) -> u32 {
        self.runtime.context().canvases.get(id).map(|c| c.height).unwrap_or(0)
    }
}
