pub struct RenderCaches {
    #[cfg(not(feature = "fast-renderer"))]
    pub image_cache: std::collections::HashMap<String, tiny_skia::Pixmap>,
    #[cfg(not(feature = "fast-renderer"))]
    pub gradient_cache: std::collections::HashMap<String, tiny_skia::Pixmap>,
    #[cfg(not(feature = "fast-renderer"))]
    pub glyph_cache: std::collections::HashMap<(usize, u16, u32, [u8; 4]), tiny_skia::Pixmap>,

    #[cfg(feature = "fast-renderer")]
    pub image_cache: std::collections::HashMap<String, (u32, u32, Vec<u32>)>,
    #[cfg(feature = "fast-renderer")]
    pub glyph_cache: std::collections::HashMap<(usize, u16, u32), fast_renderer::CachedGlyph>,
}

impl RenderCaches {
    pub fn new() -> Self {
        Self {
            image_cache: Default::default(),
            #[cfg(not(feature = "fast-renderer"))]
            gradient_cache: Default::default(),
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
) where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
{
    #[cfg(not(feature = "fast-renderer"))]
    {
        let pixmap = tiny_skia::PixmapMut::from_bytes(
            unsafe { std::slice::from_raw_parts_mut(buffer.as_mut_ptr() as *mut u8, buffer.len() * 4) },
            width,
            height,
        ).unwrap();
        let mut renderer = skia_renderer::TinySkiaRenderer::new(
            pixmap,
            fonts_ref,
            &mut caches.image_cache,
            &mut caches.gradient_cache,
            &mut caches.glyph_cache,
        );
        runtime.render(&mut renderer);
    }

    #[cfg(feature = "fast-renderer")]
    {
        let mut renderer = fast_renderer::FastRenderer::new(
            buffer,
            width,
            height,
            fonts_ref,
            &mut caches.image_cache,
            &mut caches.glyph_cache,
        );
        renderer.font_bytes = _font_bytes_ref;
        runtime.render(&mut renderer);
    }
}

pub fn run_native_app<M, TM, R>(runner: R) -> anyhow::Result<()>
where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
    R: FnOnce(
        Box<dyn FnMut(&mut xerune::Runtime<M, TM>, &mut [u32], u32, u32) + 'static>,
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
            render_frame(runtime, buffer, width, height, fonts_ref, Some(font_bytes), &mut caches);
        },
    );

    runner(render_fn, fonts_ref)
}
