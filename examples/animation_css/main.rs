#[path = "../support/mod.rs"]
mod support;
mod animation_css_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| animation_css_impl::run_native(render_fn, fonts))
}
