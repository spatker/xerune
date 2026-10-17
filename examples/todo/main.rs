#[path = "../support/mod.rs"]
mod support;
mod todo_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    let runtime = xerune::Runtime::new(
        // In the user's language (LANG=de_DE.UTF-8), English for others.
        todo_impl::TodoList::new(&xerune::i18n::system_lang().unwrap_or_default()),
        fonts.clone(),
    );
    xerune::App::new(runtime).title("Xerune Todo").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
