//! `::before` and `::after`: boxes generated as an element's first and last
//! child, from the element's `-xerune-before-*` / `-xerune-after-*`
//! declarations (see [`crate::style::ControlPart`]).

use taffy::prelude::*;
use crate::alloc_prelude::*;
use crate::style::{ContainerStyle, ControlPart, RenderData};
use super::node_map::NodeMap;
use super::Interaction;

/// The text of a `content` value; `None` for `none`, `normal` and values
/// that aren't strings (`attr()`, counters, images), which generate no box.
pub(crate) fn content_text(value: &str) -> Option<String> {
    let mut text = String::new();
    let mut rest = value.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let quote = rest.chars().next().filter(|&c| c == '"' || c == '\'')?;
        let mut chars = rest[1..].char_indices();
        let mut end = None;
        while let Some((i, c)) = chars.next() {
            match c {
                c if c == quote => {
                    end = Some(1 + i + 1);
                    break;
                }
                '\\' => {
                    // A hex escape (`\2713`, ended by one optional space) or
                    // an escaped character.
                    let tail = &rest[1 + i + 1..];
                    let hex = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_hexdigit()).len();
                    let hex = hex.min(6);
                    if hex > 0 {
                        let code = u32::from_str_radix(&tail[..hex], 16).ok()?;
                        text.push(char::from_u32(code).filter(|&c| c != '\0').unwrap_or('\u{fffd}'));
                        for _ in 0..hex {
                            chars.next();
                        }
                        if tail[hex..].starts_with(' ') {
                            chars.next();
                        }
                    } else if let Some((_, c)) = chars.next() {
                        if c != '\n' {
                            text.push(c);
                        }
                    }
                }
                c => text.push(c),
            }
        }
        // A string still open at the end of the value ends there.
        rest = end.map_or("", |end| rest[end..].trim_start());
    }
    Some(text)
}

/// Builds `host`'s `part` (`::before` or `::after`) if its `content` makes
/// a box: a `span`-like element inheriting from `host`, with the content as
/// its text. Returns the new node, not yet attached.
pub(crate) fn build(
    part: ControlPart,
    host: &ContainerStyle,
    taffy: &mut TaffyTree,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
) -> Option<NodeId> {
    let declarations = || host.part_declarations.iter().filter(move |d| d.part == part);
    let content = declarations().filter(|d| d.name == "content").last()?;
    let text = content_text(&content.value)?;

    let defaults = crate::defaults::get_default_style("span", host);
    let mut layout = defaults.taffy_style;
    let mut style = defaults.container_style;
    for font_pass in [true, false] {
        for d in declarations().filter(|d| d.name != "content" && crate::css::sets_font_size(&d.name) == font_pass) {
            crate::css::apply_declaration(&d.name, &d.value, &mut style, &mut layout);
        }
    }

    let node = taffy.new_leaf(layout.clone()).ok()?;
    if !text.is_empty() {
        let text_style = super::text_style(&style);
        let normalized = super::collapse_white_space(&text, text_style.white_space);
        if !normalized.is_empty() {
            let leaf_layout = super::text_leaf_style(&text_style);
            let leaf = taffy.new_leaf(leaf_layout.clone()).ok()?;
            render_data.insert(leaf, RenderData::new(crate::style::RenderKind::Text(normalized.into_owned()), text_style.clone()));
            base_styles.insert(leaf, (leaf_layout, text_style));
            let _ = taffy.add_child(node, leaf);
        }
    }
    let parsed = super::attributes::ParsedAttributes::new(defaults.element_type);
    let _ = super::style_resolution::finalize_node_style(node, "span", host, &mut layout, &mut style, &parsed, taffy, render_data, interactions, base_styles);
    Some(node)
}

/// Builds `node`'s `::before` and `::after` boxes and adds them as its
/// first and last child. Only elements whose content is their children
/// have them: not images, canvases, text inputs, ranges or progress bars.
pub(crate) fn generate(
    node: NodeId,
    element_type: crate::defaults::ElementType,
    host: &ContainerStyle,
    taffy: &mut TaffyTree,
    render_data: &mut NodeMap<RenderData>,
    interactions: &mut NodeMap<Interaction>,
    base_styles: &mut NodeMap<(Style, ContainerStyle)>,
) -> (Option<NodeId>, Option<NodeId>) {
    use crate::defaults::ElementType;
    if host.part_declarations.is_empty() || !matches!(element_type, ElementType::Container | ElementType::Checkbox) {
        return (None, None);
    }
    let before = build(ControlPart::Before, host, taffy, render_data, interactions, base_styles);
    if let Some(before) = before {
        let _ = taffy.insert_child_at_index(node, 0, before);
    }
    let after = build(ControlPart::After, host, taffy, render_data, interactions, base_styles);
    if let Some(after) = after {
        let _ = taffy.add_child(node, after);
    }
    (before, after)
}

#[cfg(test)]
mod tests {
    use super::content_text;

    #[test]
    fn content_strings_are_unescaped_and_joined() {
        assert_eq!(content_text("\"\"").as_deref(), Some(""));
        assert_eq!(content_text("'a' \"b\"").as_deref(), Some("ab"));
        assert_eq!(content_text(r#""\2713 x""#).as_deref(), Some("\u{2713}x"));
        assert_eq!(content_text(r#""\"q\\""#).as_deref(), Some("\"q\\"));
        assert_eq!(content_text("none"), None);
        assert_eq!(content_text("normal"), None);
        assert_eq!(content_text("attr(title)"), None);
        assert_eq!(content_text("\"open").as_deref(), Some("open"));
    }
}
