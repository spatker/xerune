#[path = "../support/mod.rs"]
mod support;
mod music_player_impl;

fn main() -> anyhow::Result<()> {
    support::run_native_app(|render_fn, fonts| music_player_impl::run_native(render_fn, fonts))
}
