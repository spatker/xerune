pub fn render_frame<M, TM>(
    runtime: &mut xerune::Runtime<M, TM>,
    buffer: &mut [u32],
    width: u32,
    height: u32,
    renderer: &mut xerune_render::SoftwareRenderer,
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
        xerune_render::Fonts,
    ) -> anyhow::Result<()>,
{
    let _ = env_logger::try_init();

    let fonts = xerune_render::Fonts::ttf(
        include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
    )
    .expect("bundled Roboto");
    // The color emoji font is ~10 MB; only embed it when asked to.
    #[cfg(feature = "emoji")]
    let fonts = fonts.with_color_emoji(&include_bytes!("../../resources/fonts/NotoColorEmoji.ttf")[..]);

    let mut renderer = xerune_render::SoftwareRenderer::new(fonts.clone());
    let render_fn = Box::new(
        move |runtime: &mut xerune::Runtime<M, TM>, buffer: &mut [u32], width: u32, height: u32| {
            render_frame(runtime, buffer, width, height, &mut renderer)
        },
    );

    runner(render_fn, fonts)
}
