#[path = "../support/mod.rs"]
mod support;
mod calculator_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(calculator_impl::CalculatorModel::default(), fonts.clone());
    xerune::App::new(runtime).title("Xerune Calculator").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
