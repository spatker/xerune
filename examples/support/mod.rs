//! What every example shares: logging and the bundled fonts.

/// Starts logging (`RUST_LOG=info` shows the backend picked) and loads the
/// bundled Roboto with a Hangul fallback, plus color emoji with `--features emoji`.
pub fn setup() -> xerune_render::Fonts {
    let _ = env_logger::try_init();
    let fonts = xerune_render::Fonts::ttf(
        include_bytes!("../../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../../resources/fonts/Roboto-Bold.ttf")),
    )
    .expect("bundled Roboto");
    // Hangul, which Roboto lacks, for the Korean translations. It adds
    // ~3.9 MB, so ARM boards only get it with `--features hangul`; the
    // examples offer Korean only where it is (`cfg(hangul)` there too).
    #[cfg(any(feature = "hangul", not(target_arch = "arm")))]
    let fonts = fonts
        .with_fallback(
            include_bytes!("../../resources/fonts/XeruneHangul-Regular.otf"),
            Some(include_bytes!("../../resources/fonts/XeruneHangul-Bold.otf")),
        )
        .expect("bundled Hangul font");
    // The color emoji font is ~10 MB; only embed it when asked to.
    #[cfg(feature = "emoji")]
    let fonts = fonts.with_color_emoji(&include_bytes!("../../resources/fonts/NotoColorEmoji.ttf")[..]);
    fonts
}
