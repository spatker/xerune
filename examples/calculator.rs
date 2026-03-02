use askama::Template;
use fontdue::Font;

use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

#[derive(Template)]
#[template(path = "calculator.html")]
struct CalculatorTemplate<'a> {
    display: &'a str,
}

struct CalculatorModel {
    display: String,
    previous_value: Option<f64>,
    pending_operation: Option<String>,
    new_input: bool,
}

impl CalculatorModel {
    fn new() -> Self {
        Self {
            display: "0".to_string(),
            previous_value: None,
            pending_operation: None,
            new_input: true,
        }
    }

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
                            // Simple error handling
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

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Digit(char),
    Operation(String),
    Equals,
    Clear,
}

impl std::str::FromStr for Msg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(digit_str) = s.strip_prefix("digit:") {
            if let Some(c) = digit_str.chars().next() {
                return Ok(Msg::Digit(c));
            }
        }
        if let Some(op_str) = s.strip_prefix("op:") {
            if op_str != "None" {
                return Ok(Msg::Operation(op_str.to_string()));
            } else {
                return Err(());
            }
        }
        match s {
            "equals" => Ok(Msg::Equals),
            "clear" => Ok(Msg::Clear),
            _ => Err(()),
        }
    }
}

impl Model for CalculatorModel {
    type Message = Msg;

    fn view(&self) -> String {
        let template = CalculatorTemplate {
            display: &self.display,
        };
        template.render().unwrap()
    }

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

fn main() -> anyhow::Result<()> {
    env_logger::init();
    
    // Load fonts
    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular, roboto_bold];
    let fonts_ref: &'static [Font] = Box::leak(fonts.into_boxed_slice());

    let measurer = TinySkiaMeasurer { fonts: fonts_ref };
    let model = CalculatorModel::new();
    let runtime = Runtime::new(model, measurer);
    
    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app(
            "Xerune Calculator", 
            400, 
            500, 
            runtime, 
            fonts_ref, 
            |_| {} // No periodic ticks needed for basic calculator
        )
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
         support::linux_backend::run_app(
             "Xerune Calculator", 
             400, 
             500, 
             runtime, 
             fonts_ref, 
             |_| {}
         )
    }
}
