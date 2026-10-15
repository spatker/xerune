#[path = "../support/mod.rs"]
mod support;
mod animation_css_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(animation_css_impl::AnimationCssModel, fonts.clone());
    xerune::App::new(runtime).title("Xerune Native CSS Animations").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
