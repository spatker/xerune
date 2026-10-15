#[path = "../support/mod.rs"]
mod support;
mod todo_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(todo_impl::TodoList::default(), fonts.clone());
    xerune::App::new(runtime).title("Xerune Todo").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
