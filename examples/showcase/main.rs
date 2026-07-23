#[path = "../support/mod.rs"]
mod support;
mod showcase_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| showcase_impl::run_native(render_fn, fonts))
}
