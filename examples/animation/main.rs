#[path = "../support/mod.rs"]
mod support;
mod animation_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let mut runtime = xerune::Runtime::new(animation_impl::AnimationModel::new(100), fonts.clone());
    runtime.set_interval("tick".to_string(), 16);
    xerune::App::new(runtime).title("Xerune Animation Benchmark").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
