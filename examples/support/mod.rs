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
    caches: &mut RenderCaches,
) where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
{
    #[cfg(not(feature = "fast-renderer"))]
    {
        let mut pixmap = tiny_skia::PixmapMut::from_bytes(
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
        runtime.render(&mut renderer);
    }
}
