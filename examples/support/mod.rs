pub struct RenderCaches {
    pub image_cache: std::collections::HashMap<String, (u32, u32, Vec<u32>)>,
    pub glyph_cache: std::collections::HashMap<(usize, u16, u32), fast_renderer::CachedGlyph>,
}

impl RenderCaches {
    pub fn new() -> Self {
        Self {
            image_cache: Default::default(),
            glyph_cache: Default::default(),
        }
    }
}

pub fn render_frame<M, TM>(
    runtime: &mut xerune::Runtime<M, TM>,
    buffer: &mut [u32],
    width: u32,
    height: u32,
    fonts_ref: &'static [fontdue::Font],
    _font_bytes_ref: Option<&'static [&'static [u8]]>,
    caches: &mut RenderCaches,
) -> Option<xerune::Rect>
where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
{
    let rotation = std::env::var("XERUNE_ROTATION")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);

    let (phys_w, phys_h) = match rotation {
        90 | 270 => (height, width),
        _ => (width, height),
    };

    let mut renderer = fast_renderer::FastRenderer::new(
        buffer,
        width,
        height,
        fonts_ref,
        &mut caches.image_cache,
        &mut caches.glyph_cache,
    );
    if rotation != 0 {
        renderer = renderer.with_rotation(phys_w, phys_h, rotation);
    }
    renderer.font_bytes = _font_bytes_ref;
    runtime.render(&mut renderer)
}

pub fn run_native_app<M, TM, R>(runner: R) -> anyhow::Result<()>
where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
    R: FnOnce(
        Box<dyn FnMut(&mut xerune::Runtime<M, TM>, &mut [u32], u32, u32) -> Option<xerune::Rect> + 'static>,
        &'static [fontdue::Font],
    ) -> anyhow::Result<()>,
{
    let _ = env_logger::try_init();

    let font_data = include_bytes!("../../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = fontdue::Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let font_data_emoji = include_bytes!("../../resources/fonts/NotoColorEmoji.ttf") as &[u8];
    let emoji_font_stub = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();

    let fonts = vec![roboto_regular, roboto_bold, emoji_font_stub];
    let font_bytes: &'static [&'static [u8]] = Box::leak(Box::new(vec![font_data, font_data_bold, font_data_emoji]));
    let fonts_ref: &'static [fontdue::Font] = Box::leak(Box::new(fonts));

    let mut caches = RenderCaches::new();
    let render_fn = Box::new(
        move |runtime: &mut xerune::Runtime<M, TM>, buffer: &mut [u32], width: u32, height: u32| {
            render_frame(runtime, buffer, width, height, fonts_ref, Some(font_bytes), &mut caches)
        },
    );

    runner(render_fn, fonts_ref)
}
