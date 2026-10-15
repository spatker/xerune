pub fn render_frame<M, TM>(
    runtime: &mut xerune::Runtime<M, TM>,
    buffer: &mut [u32],
    width: u32,
    height: u32,
    renderer: &mut xerune_render::SoftwareRenderer<'static>,
) -> xerune::Damage
where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
{
    let rotation = std::env::var("XERUNE_ROTATION")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .and_then(xerune::Rotation::from_degrees)
        .unwrap_or_default();
    let mut target = xerune::Surface::new(xerune::Pixels::Xrgb8888(buffer), width, height).with_rotation(rotation);
    runtime.render(renderer, &mut target)
}

pub fn run_native_app<M, TM, R>(runner: R) -> anyhow::Result<()>
where
    M: xerune::Model + xerune::ui::TemplateLayout + 'static,
    TM: xerune::TextMeasurer + 'static,
    R: FnOnce(
        Box<dyn FnMut(&mut xerune::Runtime<M, TM>, &mut [u32], u32, u32) -> xerune::Damage + 'static>,
        &'static [fontdue::Font],
    ) -> anyhow::Result<()>,
{
    let _ = env_logger::try_init();

    let font_data = include_bytes!("../../resources/fonts/Roboto-Regular.ttf") as &[u8];
    let roboto_regular = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();
    let font_data_bold = include_bytes!("../../resources/fonts/Roboto-Bold.ttf") as &[u8];
    let roboto_bold = fontdue::Font::from_bytes(font_data_bold, fontdue::FontSettings::default()).unwrap();
    let emoji_font_stub = fontdue::Font::from_bytes(font_data, fontdue::FontSettings::default()).unwrap();

    let fonts = vec![roboto_regular, roboto_bold, emoji_font_stub];
    #[allow(unused_mut)]
    let mut font_bytes = vec![font_data, font_data_bold];
    // The color emoji font is ~10 MB; only embed it when asked to.
    #[cfg(feature = "emoji")]
    font_bytes.push(include_bytes!("../../resources/fonts/NotoColorEmoji.ttf") as &[u8]);
    let font_bytes: &'static [&'static [u8]] = Box::leak(Box::new(font_bytes));
    let fonts_ref: &'static [fontdue::Font] = Box::leak(Box::new(fonts));

    let mut renderer = xerune_render::SoftwareRenderer::new(fonts_ref).with_font_bytes(font_bytes);
    let render_fn = Box::new(
        move |runtime: &mut xerune::Runtime<M, TM>, buffer: &mut [u32], width: u32, height: u32| {
            render_frame(runtime, buffer, width, height, &mut renderer)
        },
    );

    runner(render_fn, fonts_ref)
}
