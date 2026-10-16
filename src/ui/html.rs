//! HTML documents parsed at runtime ([`super::Ui::new`]): html5ever's DOM is
//! built with [`UiBuilder`] and styled by runtime CSS, like any tree built
//! without a compiled template.

use markup5ever_rcdom::{Handle, NodeData};
use taffy::NodeId;
use super::UiBuilder;

/// Builds `handle` (a document or element) and its descendants with
/// `builder`; `<head>`, `<style>`, `<script>` and whitespace-only text are
/// skipped. Returns the root of what was built.
pub fn build_document(handle: &Handle, builder: &mut UiBuilder) -> Option<NodeId> {
    match &handle.data {
        NodeData::Document => handle.children.borrow().iter().find_map(|child| build_document(child, builder)),
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if matches!(tag, "style" | "script" | "head" | "link" | "meta" | "title") {
                return None;
            }
            let owned: Vec<(String, String)> = attrs.borrow().iter()
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
    use crate::style::{ContainerStyle, RenderKind};

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
}
