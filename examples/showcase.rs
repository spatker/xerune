use xerune::{Model, Runtime};
use fontdue::Font;
use askama::Template;

mod support;

#[derive(Debug, Clone)]
struct RowData {
    id: String,
    name: String,
    status: String,
}

#[derive(Template)]
#[template(path = "showcase.html")]
struct ShowcaseTemplate<'a> {
    progress_value: f32,
    table_data: &'a [RowData],
    counter: i32,
}

struct ShowcaseModel {
    progress_value: f32,
    table_data: Vec<RowData>,
    counter: i32,
}

impl Model for ShowcaseModel {
    fn view(&self) -> String {
        let template = ShowcaseTemplate {
            progress_value: self.progress_value,
            table_data: &self.table_data,
            counter: self.counter,
        };
        template.render().unwrap()
    }

    fn update(&mut self, msg: &str) {
        match msg {
            "increment_progress" => {
                self.progress_value += 10.0;
                if self.progress_value > 100.0 {
                    self.progress_value = 0.0;
                }
            },
            "increment_counter" => {
                self.counter += 1;
            },
            _ => {}
        }
    }
}

fn main() -> anyhow::Result<()> {
    // Load fonts
    let font_data = include_bytes!("../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let fonts = vec![roboto_regular, roboto_bold];
    let fonts_ref: &'static [Font] = Box::leak(fonts.into_boxed_slice());

    let measurer = skia_renderer::TinySkiaMeasurer { fonts: fonts_ref };

    let model = ShowcaseModel {
        progress_value: 30.0,
        table_data: vec![
            RowData { id: "001".into(), name: "System Core".into(), status: "Online".into() },
            RowData { id: "002".into(), name: "Render Engine".into(), status: "Active".into() },
            RowData { id: "003".into(), name: "Network".into(), status: "Idle".into() },
            RowData { id: "004".into(), name: "Storage".into(), status: "Checking".into() },
            RowData { id: "005".into(), name: "Audio".into(), status: "Muted".into() },
        ],
        counter: 1234,
    };

    let runtime = Runtime::new(model, measurer);

    support::winit_backend::run_app(
        "Xerune Showcase",
        900,
        700,
        runtime,
        fonts_ref,
        None,
    )?;

    Ok(())
}
