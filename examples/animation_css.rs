use fontdue::Font;
use xerune::{Model, Runtime, XeruneTemplate};
use skia_renderer::TinySkiaMeasurer;

mod support;

#[derive(XeruneTemplate)]
#[template(path = "animation_css.html")]
struct ShowcaseModel;

impl Model for ShowcaseModel {
    type Message = String;

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

    let mut caches = support::RenderCaches::new();
    let render_fn = move |runtime: &mut Runtime<_, _>, buffer: &mut [u32], width: u32, height: u32| {
        support::render_frame(runtime, buffer, width, height, fonts_ref, &mut caches);
    };

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new().run(
            "Xerune Native CSS Animations", 
            800, 
            600, 
            runtime, 
            render_fn,
            |_| {}
        ).map_err(|e| anyhow::anyhow!("{:?}", e))?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
         use xerune::backend::Backend;
         xerune::backend::LinuxFbBackend::new().run(
             "Xerune Native CSS Animations", 
             800, 
             600, 
             runtime, 
             render_fn,
             |_| {}
         ).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
         use xerune::backend::Backend;
         xerune::backend::DrmBackend::new().run(
             "Xerune Native CSS Animations", 
             800, 
             600, 
             runtime, 
             render_fn,
             |_| {}
         ).map_err(|e| anyhow::anyhow!("{:?}", e))?;
    }

    Ok(())
}
