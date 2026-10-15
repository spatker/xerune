#[path = "../support/mod.rs"]
mod support;
mod breakout_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let mut runtime = xerune::Runtime::new(breakout_impl::BreakoutModel::default(), fonts.clone());
    runtime.set_timeout("tick".to_string(), 16);
    xerune::App::new(runtime).title("Xerune Breakout").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
