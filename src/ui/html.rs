//! HTML documents parsed at runtime ([`super::Ui::new`]): html5ever's DOM is
//! built with [`UiBuilder`] and styled by runtime CSS, like any tree built
//! without a compiled template.

use markup5ever_rcdom::{Handle, NodeData};
use taffy::NodeId;
use super::UiBuilder;

/// Builds `handle` (a document or element) and its descendants with
/// `builder`; `<head>`, `<style>`, `<script>` and whitespace-only text are
/// skipped. Elements keep the keys of their `data-xk` (hex), which the
/// template interpreter writes; others are keyed by their place among their
/// siblings. Returns the root of what was built.
pub fn build_document(handle: &Handle, builder: &mut UiBuilder) -> Option<NodeId> {
    match &handle.data {
        NodeData::Document => handle.children.borrow().iter().find_map(|child| build_document(child, builder)),
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "style" | "script" | "head" | "link" | "meta" | "title") {
                return None;
            }
            let mut key = None;
            let owned: Vec<(String, String)> = attrs.borrow().iter()
                .filter(|a| match a.name.local.as_ref() {
                    "data-xk" => {
                        key = u64::from_str_radix(&a.value, 16).ok();
                        false
                    }
                    _ => true,
                })
                .map(|a| (a.name.local.to_string(), a.value.to_string())).collect();
            let attrs: Vec<_> = owned.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
            let get = |key: &str| attrs.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
            let number = |key: &str| get(key).and_then(|v| v.parse::<f32>().ok());
            let node = match (tag, get("type")) {
                ("input", Some("checkbox")) => builder.create_checkbox(get("checked").is_some(), &attrs),
                ("input", Some("range")) => builder.create_slider(number("value").unwrap_or(0.0), &attrs),
                ("input", Some("text") | None) => builder.create_input_text(get("value").unwrap_or(""), &attrs),
                ("progress", _) => builder.create_progress(number("value").unwrap_or(0.0), number("max").unwrap_or(1.0), &attrs),
                ("img", _) => builder.create_image(get("src").unwrap_or(""), &attrs),
                _ => builder.create_element(tag, &attrs),
            };
            builder.node_to_handle.insert(node, handle.clone());
            // The interpreter's key: the element's place in the template,
            // as in a compiled view, so it stays when siblings come and go.
            if let Some(key) = key {
                builder.keys.insert(node, key);
            }
            if let Some(meta) = builder.node_metadata.get_mut(&node) {
                meta.attrs = owned;
            }
            for child in handle.children.borrow().iter() {
                if let Some(child) = build_document(child, builder) {
                    builder.append_child(node, child);
                }
            }
            Some(node)
        }
        NodeData::Text { contents } => {
            let text = contents.borrow();
            if text.trim().is_empty() {
                return None;
            }
            let node = builder.create_text(&text, &[]);
            builder.node_to_handle.insert(node, handle.clone());
            Some(node)
        }
        _ => None,
    }
}

/// The text of every `<style>` element under `handle`, in document order.
pub fn extract_styles(handle: &Handle, css: &mut String) {
    if let NodeData::Element { name, .. } = &handle.data {
        if name.local.as_ref() == "style" {
            for child in handle.children.borrow().iter() {
                if let NodeData::Text { contents } = &child.data {
                    css.push_str(&contents.borrow());
                    css.push('\n');
                }
            }
        }
    }
    for child in handle.children.borrow().iter() {
        extract_styles(child, css);
    }
}

#[cfg(test)]
mod tests {
    use crate::alloc_prelude::*;
    use crate::style::{ContainerStyle, RenderKind};
    use html5ever::tendril::TendrilSink;

    struct NoText;
    impl crate::TextMeasurer for NoText {
        fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (0.0, 16.0)
        }
    }

    /// `Ui::new` styles a document with its `<style>`s, like runtime CSS.
    #[test]
    fn documents_are_styled_by_their_style_elements() {
        let html = "<style>p { color: #ff0000 } p::before { content: 'x' }</style><p>hi</p>";
        let ui = super::super::Ui::new(html, &NoText, ContainerStyle::default(), &|_| true).unwrap();
        let mut texts: Vec<_> = ui.render_data.values().filter_map(|d| match (&d.kind, &d.style) {
            (RenderKind::Text(text), style) => Some((text.as_str(), style.color.r)),
            _ => None,
        }).collect();
        texts.sort();
        assert_eq!(texts, [("hi", 255), ("x", 255)]);
        assert!(ui.body.is_some());
    }

    /// An element with a `data-xk` keeps that key when a sibling appears
    /// before it; one without is keyed by its index, which moves.
    #[test]
    fn data_xk_keys_elements() {
        let key_of = |html: &str, text: &str| {
            let ui = super::super::Ui::new(html, &NoText, ContainerStyle::default(), &|_| true).unwrap();
            let text_node = ui.render_data.iter().find(|(_, d)| matches!(&d.kind, RenderKind::Text(t) if t == text)).map(|(n, _)| n).unwrap();
            ui.keys.get(ui.taffy.parent(text_node).unwrap()).copied().unwrap()
        };
        assert_eq!(key_of("<p data-xk=\"2a\">b</p>", "b"), 0x2a);
        assert_eq!(key_of("<p>a</p><p data-xk=\"2a\">b</p>", "b"), 0x2a);
        assert_ne!(key_of("<p>b</p>", "b"), key_of("<p>a</p><p>b</p>", "b"));
    }

    /// A runtime stylesheet is parsed once; its `@media` blocks are
    /// evaluated for the viewport of each build, and cascade with the rest
    /// by specificity. (The only lib test that sets the global viewport.)
    #[test]
    fn media_blocks_apply_to_the_viewport_of_each_build() {
        let html = "<style>p { color: #000000 } @media (min-width: 500px) { p { color: #ff0000 } body p { color: #00ff00 } } body p { color: #0000ff }</style><p>a</p>";
        let color = |width: f32| {
            crate::screen::set_viewport(width, 300.0);
            let ui = super::super::Ui::new(html, &NoText, ContainerStyle::default(), &|_| true).unwrap();
            ui.render_data.values().find_map(|d| match &d.kind {
                RenderKind::Text(_) => Some((d.style.color.r, d.style.color.g, d.style.color.b)),
                _ => None,
            }).unwrap()
        };
        assert_eq!(color(800.0), (0, 0, 255), "`body p` after the block wins over the block's");
        assert_eq!(color(400.0), (0, 0, 255));
        let html_block_last = "<style>body p { color: #0000ff } @media (min-width: 500px) { body p { color: #00ff00 } }</style><p>a</p>";
        let color = |width: f32| {
            crate::screen::set_viewport(width, 300.0);
            let ui = super::super::Ui::new(html_block_last, &NoText, ContainerStyle::default(), &|_| true).unwrap();
            ui.render_data.values().find_map(|d| match &d.kind {
                RenderKind::Text(_) => Some(d.style.color.g),
                _ => None,
            }).unwrap()
        };
        assert_eq!(color(800.0), 255);
        assert_eq!(color(400.0), 0);
    }

    /// `:hover` in runtime stylesheets applies to the pointer's elements,
    /// found by their keys: `<html>` is the root (key 0), then each element
    /// mixes in its index.
    #[test]
    fn hover_rules_apply_to_the_elements_under_the_pointer() {
        use crate::ui::builder::element_key;
        let html = "<style>p { color: #000000 } body:hover p { color: #ff0000 } body p:hover { color: #00ff00 }</style><p>a</p><p>b</p>";
        let dom = html5ever::parse_document(markup5ever_rcdom::RcDom::default(), Default::default()).from_utf8().read_from(&mut html.as_bytes()).unwrap();
        let mut css = String::new();
        super::extract_styles(&dom.document, &mut css);
        let mut builder = super::super::UiBuilder::new();
        let root = super::build_document(&dom.document, &mut builder).unwrap();
        let body = element_key(0, 1);
        builder.pointer.hovered = vec![element_key(body, 2), body, 0];
        let ui = super::super::Ui::from_builder(builder, root, &css, &NoText, ContainerStyle::default(), &|_| true).unwrap();
        let mut texts: Vec<_> = ui.render_data.values().filter_map(|d| match &d.kind {
            RenderKind::Text(text) => Some((text.as_str(), (d.style.color.r, d.style.color.g))),
            _ => None,
        }).collect();
        texts.sort();
        assert_eq!(texts, [("a", (255, 0)), ("b", (0, 255))]);
        assert!(ui.uses_hover && !ui.uses_active);
    }
}
