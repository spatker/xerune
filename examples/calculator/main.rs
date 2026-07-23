#[path = "../support/mod.rs"]
mod support;
mod calculator_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| calculator_impl::run_native(render_fn, fonts))
}
