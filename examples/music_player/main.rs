#[path = "../support/mod.rs"]
mod support;
mod music_player_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(music_player_impl::MusicPlayerModel::new(None), fonts.clone());
    xerune::App::new(runtime).title("Xerune Music Player").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
