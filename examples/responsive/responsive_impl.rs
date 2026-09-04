#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fontdue::Font;
use xerune::{screen, Model, Runtime, XeruneMessage, XeruneTemplate};

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
use fast_renderer::FastMeasurer;

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub type Measurer = FastMeasurer<'static>;

/// Demonstrates responsive layout: the template adapts the card grid, sidebar
/// visibility and header styling to the viewport purely via CSS `@media` and
/// `vw`/`vh` units. The model only records the viewport size for the on-screen
/// badge — no layout branching happens in Rust.
#[derive(XeruneTemplate)]
#[template(path = "responsive.html")]
pub struct ResponsiveModel {
    pub cards: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub breakpoint: String,
}

impl Default for ResponsiveModel {
    fn default() -> Self {
        Self {
            cards: (1..=6).map(|i| format!("Panel {}", i)).collect(),
            width: 0,
            height: 0,
            breakpoint: "mobile".to_string(),
        }
    }
}

#[derive(Debug, Clone, XeruneMessage)]
pub enum Msg {
    #[xerune(prefix = "refresh:")]
    Refresh(String),
}

impl Model for ResponsiveModel {
    type Message = Msg;

    fn update(&mut self, _msg: Self::Message, _context: &mut xerune::Context) {}

    fn on_resize(&mut self, width: f32, height: f32, _context: &mut xerune::Context) {
        self.width = width as u32;
        self.height = height as u32;
        self.breakpoint = match screen::breakpoint() {
            screen::Breakpoint::Mobile => "mobile",
            screen::Breakpoint::Tablet => "tablet",
            screen::Breakpoint::Desktop => "desktop",
        }
        .to_string();
    }
}

#[cfg(all(not(target_arch = "wasm32"), any(feature = "winit", feature = "linuxfb", feature = "drm")))]
pub fn run_native(
    render_frame: impl FnMut(&mut Runtime<ResponsiveModel, Measurer>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static,
    fonts_ref: &'static [Font],
) -> anyhow::Result<()> {
    let model = ResponsiveModel::default();
    let measurer = FastMeasurer { fonts: fonts_ref.into() };
    let runtime = Runtime::new(model, measurer);

    #[cfg(not(any(
        all(target_os = "linux", feature = "linuxfb", feature = "evdev"),
        all(target_os = "linux", feature = "drm", feature = "evdev")
    )))]
    {
        use xerune::backend::Backend;
        xerune::backend::WinitBackend::new()
            .run("Xerune Responsive", 900, 600, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "linuxfb", feature = "evdev", not(feature = "drm")))]
    {
        use xerune::backend::Backend;
        xerune::backend::LinuxFbBackend::new()
            .run("Xerune Responsive", 900, 600, runtime, render_frame, |_| {})?;
    }

    #[cfg(all(target_os = "linux", feature = "drm", feature = "evdev"))]
    {
        use xerune::backend::Backend;
        xerune::backend::DrmBackend::new()
            .run("Xerune Responsive", 900, 600, runtime, render_frame, |_| {})?;
    }

    Ok(())
}
