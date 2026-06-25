use askama::Template;
use fontdue::Font;
use xerune::{Model, Runtime};
use skia_renderer::TinySkiaMeasurer;

#[path = "support/mod.rs"]
mod support;

struct ShowcaseModel;

#[derive(Template)]
#[template(path = "animation_css.html")]
struct ShowcaseTemplate;

impl Model for ShowcaseModel {
    type Message = String;

    fn view(&self) -> String {
        let template = ShowcaseTemplate;
        template.render().unwrap()
    }

    fn update(&mut self, _msg: Self::Message, _context: &mut xerune::Context) {}
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
    let model = ShowcaseModel;
    let runtime = Runtime::new(model, measurer);

    #[cfg(not(all(target_os = "linux", feature = "linuxfb", feature = "evdev")))]
    {
        support::winit_backend::run_app(
            "Xerune Native CSS Animations", 
            800, 
            600, 
            runtime, 
            fonts_ref, 
            |_| {}
        )
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev"))]
    {
         support::linux_backend::run_app(
             "Xerune Native CSS Animations", 
             800, 
             600, 
             runtime, 
             fonts_ref, 
             |_| {}
         )
    }
}
