#[path = "../support/mod.rs"]
mod support;
mod showcase_impl;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fonts = support::setup();
    // In the user's language (LANG=de_DE.UTF-8), English for others.
    let lang = xerune::i18n::system_lang().unwrap_or_default();
    let model = showcase_impl::ShowcaseModel { lang, ..Default::default() };
    let runtime = xerune::Runtime::new(model, fonts.clone());
    xerune::App::new(runtime).title("Xerune Showcase").run(xerune_render::SoftwareRenderer::new(fonts))?;
    Ok(())
}
