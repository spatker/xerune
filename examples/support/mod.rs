//! What every example shares: logging and the bundled fonts.

/// Starts logging (`RUST_LOG=info` shows the backend picked) and loads the
/// bundled Roboto, plus color emoji with `--features emoji`.
pub fn setup() -> xerune_render::Fonts {
    let _ = env_logger::try_init();
    let fonts = xerune_render::Fonts::ttf(
        include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
    )
    .expect("bundled Roboto");
    // The color emoji font is ~10 MB; only embed it when asked to.
    #[cfg(feature = "emoji")]
    let fonts = fonts.with_color_emoji(&include_bytes!("../../resources/fonts/NotoColorEmoji.ttf")[..]);
    fonts
}
