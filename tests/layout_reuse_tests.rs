//! Layout reuse across rebuilds. A binary of its own: views are built for
//! the process-global viewport, which runtimes in parallel tests set.

use xerune::*;

#[path = "../examples/calculator/calculator_impl.rs"]
#[allow(dead_code)]
mod calculator_impl;
#[path = "../examples/showcase/showcase_impl.rs"]
#[allow(dead_code)]
mod showcase_impl;
#[path = "../examples/todo/todo_impl.rs"]
#[allow(dead_code)]
mod todo_impl;

/// The bundled Roboto, regular and bold, with the examples' Hangul fallback.
fn roboto() -> xerune_render::Fonts {
    xerune_render::Fonts::ttf(
        include_bytes!("../resources/fonts/Roboto-Regular.ttf"),
        Some(include_bytes!("../resources/fonts/Roboto-Bold.ttf")),
    )
    .unwrap()
    .with_fallback(
        include_bytes!("../resources/fonts/XeruneHangul-Regular.otf"),
        Some(include_bytes!("../resources/fonts/XeruneHangul-Bold.otf")),
    )
    .unwrap()
}

/// A view that takes over the previous view's laid out tree
/// (`Ui::reuse_layout`) is laid out exactly as the same view built and
/// laid out from scratch, through interaction sequences of the examples:
/// text and style changes reuse the tree, structural changes (a todo's
/// check dot) don't.
#[test]
fn reused_layout_matches_a_fresh_one() {
    fn check<M: Model + ui::TemplateLayout>(model: M, messages: &[&str], sizes: &[(f32, f32)]) -> usize {
        let fonts = roboto();
        let build = |runtime: &Runtime<M, xerune_render::Fonts>| {
            let validator = |s: &str| s.parse::<M::Message>().is_ok();
            ui::Ui::new_compiled(runtime.model(), &fonts, ContainerStyle::default(), &validator).unwrap()
        };
        let mut runtime = Runtime::new(model, fonts.clone());
        let mut previous: Option<ui::Ui> = None;
        let mut reused = 0;
        for &(width, height) in sizes {
            runtime.set_size(width, height);
            let space = taffy::Size { width: taffy::AvailableSpace::Definite(width), height: taffy::AvailableSpace::Definite(height) };
            for message in messages {
                runtime.handle_messages([message.to_string()]);
                let mut ui = build(&runtime);
                if let Some(mut old) = previous.take() {
                    reused += ui.reuse_layout(&mut old, space) as usize;
                }
                ui.compute_layout(space).unwrap();
                let mut fresh = build(&runtime);
                fresh.compute_layout(space).unwrap();
                let mut stack = vec![fresh.root];
                while let Some(node) = stack.pop() {
                    assert_eq!(ui.taffy.layout(node).unwrap(), fresh.taffy.layout(node).unwrap(), "{message} at {width}x{height}: node {node:?}");
                    assert_eq!(ui.text_lines.get(node), fresh.text_lines.get(node), "{message}: lines of node {node:?}");
                    stack.extend(fresh.taffy.children(node).unwrap());
                }
                previous = Some(ui);
            }
        }
        reused
    }
    let sizes = [(800.0, 480.0), (480.0, 800.0)];
    let calculator = ["digit:7", "digit:3", "digit:9", "clear", "digit:1"];
    assert!(check(calculator_impl::CalculatorModel::default(), &calculator, &sizes) > 0);
    let showcase = ["toggle_wifi", "select_tab:1", "volume_up", "select_tab:2", "increment_speed", "lang:de", "select_tab:0"];
    assert!(check(showcase_impl::ShowcaseModel::default(), &showcase, &sizes) > 0);
    check(todo_impl::TodoList::default(), &["toggle:0", "toggle:1", "toggle:0"], &sizes);
}
