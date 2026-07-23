#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use xerune::{Runtime, Model, XeruneTemplate};

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
#[template(path = "animation_css.html")]
pub struct AnimationCssModel;

impl Model for AnimationCssModel {
    type Message = String;

    fn update(&mut self, _msg: Self::Message, _context: &mut xerune::Context) {}
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(render_frame: impl FnMut(&mut Runtime<AnimationCssModel, Measurer>, &mut [u32], u32, u32) + 'static, fonts_ref: &'static [Font]) -> anyhow::Result<()> {
    let model = AnimationCssModel;
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
        xerune::backend::WinitBackend::new().run("Xerune Native CSS Animations", 800, 600, runtime, render_frame, |_| {})?
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new().run("Xerune Native CSS Animations", 800, 600, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new().run("Xerune Native CSS Animations", 800, 600, runtime, render_frame, |_| {})?;
    }

    Ok(())
}
