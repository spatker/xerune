#[path = "../support/mod.rs"]
mod support;
mod showcase_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let mut runtime = xerune::Runtime::new(showcase_impl::ShowcaseModel::default(), fonts.clone());
    runtime.set_interval("tick".to_string(), showcase_impl::TICK_MS);
    xerune::App::new(runtime).title("Xerune Showcase").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
