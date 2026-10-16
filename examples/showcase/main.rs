#[path = "../support/mod.rs"]
mod support;
mod showcase_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(showcase_impl::ShowcaseModel::default(), fonts.clone());
    xerune::App::new(runtime).title("Xerune Showcase").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
