#[path = "../support/mod.rs"]
mod support;
mod todo_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| todo_impl::run_native(render_fn, fonts))
}
