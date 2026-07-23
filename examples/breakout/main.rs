#[path = "../support/mod.rs"]
mod support;
mod breakout_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| breakout_impl::run_native(render_fn, fonts))
}
