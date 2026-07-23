#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use xerune::{Runtime, Model, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
#[cfg(not(feature = "fast-renderer"))]
use skia_renderer::TinySkiaMeasurer;
#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
#[cfg(feature = "fast-renderer")]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
#[cfg(not(feature = "fast-renderer"))]
pub type Measurer = TinySkiaMeasurer<'static>;
#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
#[cfg(feature = "fast-renderer")]
pub type Measurer = FastMeasurer<'static>;

#[derive(XeruneTemplate, serde::Serialize, serde::Deserialize)]
#[template(path = "calculator.html")]
pub struct CalculatorModel {
    pub display: String,
    pub previous_value: Option<f64>,
    pub pending_operation: Option<String>,
    pub new_input: bool,
}

impl Default for CalculatorModel {
    fn default() -> Self {
        Self {
            display: "0".to_string(),
            previous_value: None,
            pending_operation: None,
            new_input: true,
        }
    }
}

impl CalculatorModel {
    fn format_result(result: f64) -> String {
        if result.is_nan() {
            return "Error".to_string();
        }
        if result.is_infinite() {
            if result.is_sign_positive() {
                return "Infinity".to_string();
            } else {
                return "-Infinity".to_string();
            }
        }

        let simple = format!("{}", result);
        if simple.len() <= 12 {
            return simple;
        }

        if let Some(dot_idx) = simple.find('.') {
            if dot_idx < 12 {
                let mut truncated = simple[..12].to_string();
                if truncated.ends_with('.') {
                    truncated.pop();
                }
                return truncated;
            }
        }

        let sci = format!("{:.5e}", result);
        if sci.len() <= 12 {
            return sci;
        }

        "Overflow".to_string()
    }

    fn calculate(&mut self) {
        if let (Some(prev), Some(op)) = (self.previous_value, &self.pending_operation) {
            if let Ok(current) = self.display.parse::<f64>() {
                let result = match op.as_str() {
                    "+" => prev + current,
                    "-" => prev - current,
                    "*" => prev * current,
                    "/" => {
                        if current != 0.0 {
                            prev / current
                        } else {
                            f64::NAN
                        }
                    }
                    _ => current,
                };
                self.display = Self::format_result(result);
                self.previous_value = Some(result);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, XeruneMessage)]
pub enum Msg {
    Digit(char),
    #[xerune(prefix = "op:")]
    Operation(String),
    Equals,
    Clear,
}

impl Model for CalculatorModel {
    type Message = Msg;

    fn update(&mut self, msg: Self::Message, _context: &mut xerune::Context) {
        match msg {
            Msg::Digit(d) => {
                if self.new_input {
                    self.display = d.to_string();
                    self.new_input = false;
                } else if self.display.len() < 12 {
                    if d == '.' {
                        if !self.display.contains('.') {
                            self.display.push(d);
                        }
                    } else {
                        if self.display == "0" {
                            self.display = d.to_string();
                        } else {
                            self.display.push(d);
                        }
                    }
                }
            }
            Msg::Operation(op) => {
                if !self.new_input {
                    if self.pending_operation.is_some() {
                        self.calculate();
                    } else {
                        self.previous_value = self.display.parse::<f64>().ok();
                    }
                }
                self.pending_operation = Some(op);
                self.new_input = true;
            }
            Msg::Equals => {
                self.calculate();
                self.pending_operation = None;
                self.new_input = true;
            }
            Msg::Clear => {
                self.display = "0".to_string();
                self.previous_value = None;
                self.pending_operation = None;
                self.new_input = true;
            }
        }
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<CalculatorModel, Measurer>, &mut [u32], u32, u32) + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = CalculatorModel::default();
    #[cfg(not(feature = "fast-renderer"))]
    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    #[cfg(feature = "fast-renderer")]
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    
    let runtime = Runtime::new(model, measurer);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run("Xerune Calculator", 400, 500, runtime, render_frame, | _ | {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Calculator", 400, 500, runtime, render_frame, | _ | {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Calculator", 400, 500, runtime, render_frame, | _ | {})?;
    }

    Ok(())
}
