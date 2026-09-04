#[path = "../support/mod.rs"]
mod support;
mod responsive_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| responsive_impl::run_native(render_fn, fonts))
}
