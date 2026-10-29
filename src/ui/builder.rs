#![allow(missing_docs)]

use taffy::prelude::*;
use crate::style::RenderData;
use super::node_map::NodeMap;
use super::metadata::NodeMetadata;
use super::{Interaction, Handle};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub struct UiBuilder {
    pub taffy: TaffyTree,
    pub node_metadata: NodeMap<NodeMetadata>,
    pub render_data: NodeMap<RenderData>,
    pub interactions: NodeMap<Interaction>,
    pub node_to_handle: NodeMap<Handle>,
    pub base_styles: NodeMap<(Style, crate::style::ContainerStyle)>,
    pub keyframes: HashMap<String, crate::css::KeyframesAnimation>,
    /// The first `<html>` and `<body>` built, for the canvas background.
    pub html: Option<NodeId>,
    pub body: Option<NodeId>,
    /// Elements with an `id` attribute.
    pub element_ids: Vec<(Cow<'static, str>, NodeId)>,
    /// Elements with a `lang` attribute (see `Ui::langs`).
    pub langs: Vec<(NodeId, String)>,
    /// Each element's [`element_key`].
    pub keys: NodeMap<u64>,
    /// The elements under the pointer, for `:hover` and `:active`.
    pub pointer: PointerState,
    pub hover_handlers: NodeMap<super::HoverHandlers>,
    pub transition_handlers: NodeMap<super::TransitionHandlers>,
    pub value_handlers: NodeMap<super::ValueHandlers>,
    /// The keys of the view this one replaces, to tell new elements:
    /// `@starting-style` rules are computed for those. `None` computes none.
    pub previous_keys: Option<HashSet<u64>>,
    pub starting_styles: NodeMap<(Style, crate::style::ContainerStyle)>,
    pub uses_starting_style: bool,
}

/// Whether the element with `key` is new: not among `previous_keys`, the
/// keys of the view before, which are there when `@starting-style` styles
/// are wanted.
pub(crate) fn is_new(previous_keys: Option<&HashSet<u64>>, key: u64) -> bool {
    previous_keys.is_some_and(|keys| !keys.contains(&key))
}

/// Gives an element's `::before` / `::after` boxes keys from its own `key`.
pub(crate) fn key_generated_boxes(keys: &mut NodeMap<u64>, key: u64, (before, after): super::style_resolution::GeneratedBoxes) {
    // Sites far from any template's.
    for (node, site) in [(before, u64::MAX - 1), (after, u64::MAX - 2)] {
        if let Some(node) = node {
            keys.insert(node, element_key(key, site));
        }
    }
}

/// The keys of the elements the pointer is over and presses: the element
/// under it and all its ancestors (`:hover` and `:active` apply to both).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PointerState {
    /// The hovered elements.
    pub hovered: Vec<u64>,
    /// The elements being pressed.
    pub active: Vec<u64>,
}

/// How far up from an element its nearest hovered (and pressed) element is:
/// 0 when it is hovered itself, `u32::MAX` when none of its ancestors is.
/// Templates compile `:hover` rules into tests of this, since hovered
/// elements are an element and its ancestors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerLevel {
    /// Levels up to the nearest hovered element.
    pub hover: u32,
    /// Levels up to the nearest pressed element.
    pub active: u32,
}

impl PointerLevel {
    /// Nothing hovered or pressed.
    pub const NONE: PointerLevel = PointerLevel { hover: u32::MAX, active: u32::MAX };
}

/// The key of an element: the same element of the view gets the same key
/// in every rebuild, so state that outlives a rebuild (the pointer's
/// `:hover`) can find it again. A template element's key mixes its
/// parent's with its place in the template (`site`) and, in a
/// `{% for %}`, the iteration; an element built at runtime mixes in its
/// index among its parent's elements.
#[inline]
pub fn element_key(parent: u64, site: u64) -> u64 {
    // splitmix64's finalizer: adjacent sites and indices spread apart.
    let mut x = parent ^ site.wrapping_add(0x9e37_79b9_7f4a_7c15).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}


impl UiBuilder {
    pub fn new() -> Self {
        Self::with_capacity(128)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            taffy: TaffyTree::new(),
            node_metadata: NodeMap::with_capacity(capacity),
            render_data: NodeMap::with_capacity(capacity),
            interactions: NodeMap::with_capacity(capacity),
            node_to_handle: NodeMap::with_capacity(capacity),
            base_styles: NodeMap::with_capacity(capacity),
            keyframes: HashMap::new(),
            html: None,
            body: None,
            element_ids: Vec::new(),
            langs: Vec::new(),
            keys: NodeMap::with_capacity(capacity),
            pointer: PointerState::default(),
            hover_handlers: NodeMap::new(),
            transition_handlers: NodeMap::new(),
            value_handlers: NodeMap::new(),
            previous_keys: None,
            starting_styles: NodeMap::new(),
            uses_starting_style: false,
        }
    }

    /// The template has `@starting-style` rules.
    pub fn note_starting_style(&mut self) {
        self.uses_starting_style = true;
    }

    /// Whether the element with `key` is new in this view and its
    /// `@starting-style` style is wanted.
    pub fn is_new(&self, key: u64) -> bool {
        is_new(self.previous_keys.as_ref(), key)
    }

    /// Reads `attrs` into the starting style `el` of a new element, as
    /// [`Self::begin_element`] does for its style.
    #[inline(never)]
    pub fn starting_attributes(&self, el: &mut ElementStyle, attrs: &[(Cow<'static, str>, Cow<'_, str>)], message_validator: &dyn Fn(&str) -> bool) {
        el.read_attributes(attrs.iter().map(|(k, v)| (k.as_ref(), v.as_ref())), message_validator);
    }

    /// Keeps `el`, the style `node` starts from by `@starting-style`.
    #[inline(never)]
    pub fn set_starting_style(&mut self, node: NodeId, el: ElementStyle, parent_style: &crate::style::ContainerStyle) {
        self.starting_styles.insert(node, el.into_box(parent_style));
    }

    /// The [`PointerLevel`] of the element with `key`, whose parent's is
    /// `parent`.
    #[inline]
    pub fn pointer_level(&self, key: u64, parent: PointerLevel) -> PointerLevel {
        let level = |chain: &[u64], parent: u32| if chain.contains(&key) { 0 } else { parent.saturating_add(1) };
        PointerLevel { hover: level(&self.pointer.hovered, parent.hover), active: level(&self.pointer.active, parent.active) }
    }

    /// The initial containing block: a block box the size of the viewport,
    /// with the inherited `default_style`. Generated `build_ui` code starts
    /// here.
    pub fn begin_root(&mut self, default_style: &crate::style::ContainerStyle) -> NodeId {
        let layout = Style {
            display: Display::Block,
            size: Size { width: Dimension::percent(1.0), height: Dimension::percent(1.0) },
            ..Style::default()
        };
        let root = self.taffy.new_leaf(layout.clone()).unwrap();
        self.render_data.insert(root, RenderData::new(crate::style::RenderKind::Container, default_style.clone()));
        if default_style.animates() {
            self.base_styles.insert(root, (layout, default_style.clone()));
        }
        root
    }

    /// The template's `@keyframes` rules.
    pub fn set_keyframes(&mut self, keyframes: HashMap<String, crate::css::KeyframesAnimation>) {
        self.keyframes = keyframes;
    }

    fn parse_attrs_cow(attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> (Option<Cow<'static, str>>, Option<Cow<'static, str>>, Option<String>, Option<Vec<(String, String)>>) {
        let mut class = None;
        let mut id = None;
        let mut style = None;
        let mut other_attrs = None;

        for (k, v) in attrs {
            match k.as_ref() {
                "class" => class = Some(core::mem::take(v)),
                "id" => id = Some(core::mem::take(v)),
                "style" => style = Some(core::mem::take(v).into_owned()),
                _ => {
                    let vec = other_attrs.get_or_insert_with(Vec::new);
                    vec.push((core::mem::take(k).into_owned(), core::mem::take(v).into_owned()));
                }
            }
        }
        (class, id, style, other_attrs)
    }

    pub fn create_element_cow(&mut self, tag: Cow<'static, str>, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        self.note_root_element(&tag, id);
        let metadata = NodeMetadata::new(tag, class, id_val, style, other_attrs);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_text_cow(&mut self, text: Cow<'static, str>, _attrs: &[(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let mut metadata = NodeMetadata::new(Cow::Borrowed("#text"), None, None, None, None);
        metadata.text = Some(text.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_checkbox_cow(&mut self, checked: bool, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.checked = Some(checked);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_slider_cow(&mut self, value: f32, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.slider_value = Some(value);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_input_text_cow(&mut self, value: Cow<'static, str>, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.input_text = Some(value.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_progress_cow(&mut self, value: f32, max: f32, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("progress"), class, id_val, style, other_attrs);
        metadata.progress_value = Some(value);
        metadata.progress_max = Some(max);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_image_cow(&mut self, src: Cow<'static, str>, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("img"), class, id_val, style, other_attrs);
        metadata.image_src = Some(src.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_canvas_cow(&mut self, canvas_id: Cow<'static, str>, attrs: &mut [(Cow<'static, str>, Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(Cow::Borrowed("canvas"), class, id_val, style, other_attrs);
        metadata.canvas_id = Some(canvas_id.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    fn parse_attrs(attrs: &[(&str, &str)]) -> (Option<std::borrow::Cow<'static, str>>, Option<std::borrow::Cow<'static, str>>, Option<String>, Option<Vec<(String, String)>>) {
        let mut class = None;
        let mut id = None;
        let mut style = None;
        let mut other_attrs = None;

        for &(k, v) in attrs {
            match k {
                "class" => class = Some(std::borrow::Cow::Owned(v.to_string())),
                "id" => id = Some(std::borrow::Cow::Owned(v.to_string())),
                "style" => style = Some(v.to_string()),
                _ => {
                    let vec = other_attrs.get_or_insert_with(Vec::new);
                    vec.push((k.to_string(), v.to_string()));
                }
            }
        }
        (class, id, style, other_attrs)
    }

    pub fn create_element(&mut self, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        self.note_root_element(tag, id);
        let metadata = NodeMetadata::new(std::borrow::Cow::Owned(tag.to_string()), class, id_val, style, other_attrs);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_text(&mut self, text: &str, _attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("#text"), None, None, None, None);
        metadata.text = Some(text.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_checkbox(&mut self, checked: bool, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.checked = Some(checked);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_slider(&mut self, value: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.slider_value = Some(value);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_input_text(&mut self, value: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.input_text = Some(value.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_progress(&mut self, value: f32, max: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("progress"), class, id_val, style, other_attrs);
        metadata.progress_value = Some(value);
        metadata.progress_max = Some(max);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_image(&mut self, src: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("img"), class, id_val, style, other_attrs);
        metadata.image_src = Some(src.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_canvas(&mut self, canvas_id: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("canvas"), class, id_val, style, other_attrs);
        metadata.canvas_id = Some(canvas_id.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let _ = self.taffy.add_child(parent, child);
        if let Some(meta) = self.node_metadata.get_mut(&parent) {
            meta.children.push(child);
        }
    }

    /// Parses `el`'s attributes, then creates its taffy node and appends it to
    /// `parent`. Returns the new node; pass it and `el` to
    /// [`finish_element`](Self::finish_element) once its children are built.
    #[inline(never)]
    pub fn begin_element(
        &mut self,
        el: &mut ElementStyle,
        parent: NodeId,
        attrs: &[(Cow<'static, str>, Cow<'_, str>)],
        message_validator: &dyn Fn(&str) -> bool,
        key: u64,
    ) -> NodeId {
        el.read_attributes(attrs.iter().map(|(k, v)| (k.as_ref(), v.as_ref())), message_validator);
        // `finish_element` sets the layout; nothing reads it before.
        let node = self.taffy.new_leaf(Style::default()).unwrap();
        self.keys.insert(node, key);
        self.append_child(parent, node);
        if let Some((_, id)) = attrs.iter().find(|(name, _)| name == "id") {
            self.element_ids.push((Cow::Owned(id.clone().into_owned()), node));
        }
        if let Some((_, lang)) = attrs.iter().find(|(name, _)| name == "lang") {
            self.langs.push((node, lang.clone().into_owned()));
        }
        self.note_root_element(&el.tag, node);
        node
    }

    /// Remembers the first `<html>` and `<body>`, for the canvas background.
    fn note_root_element(&mut self, tag: &str, node: NodeId) {
        match tag {
            "html" if self.html.is_none() => self.html = Some(node),
            "body" if self.body.is_none() => self.body = Some(node),
            _ => {}
        }
    }

    /// Resolves `el`'s final style after its children have been appended.
    #[inline(never)]
    pub fn finish_element(&mut self, mut el: ElementStyle, node: NodeId, parent_style: &crate::style::ContainerStyle) {
        if !el.parsed.hover.is_empty() {
            self.hover_handlers.insert(node, core::mem::take(&mut el.parsed.hover));
        }
        if !el.parsed.transition.is_empty() {
            self.transition_handlers.insert(node, core::mem::take(&mut el.parsed.transition));
        }
        if !el.parsed.value.is_empty() {
            self.value_handlers.insert(node, core::mem::take(&mut el.parsed.value));
        }
        let generated = super::style_resolution::finalize_node_style(
            node,
            &el.tag,
            parent_style,
            el.layout,
            el.style,
            &el.parsed,
            &mut self.taffy,
            &mut self.render_data,
            &mut self.interactions,
            &mut self.base_styles,
        );
        if let Some(&key) = self.keys.get(node) {
            key_generated_boxes(&mut self.keys, key, generated);
        }
    }

    /// Creates a text leaf for a run of text styled from `parent_style` and
    /// appends it to `parent`. Used by derive-generated templates; kept out of line so each
    /// text node in a template doesn't inline its own copy.
    pub fn append_text_node(
        &mut self,
        _measurer: &impl crate::TextMeasurer,
        parent: NodeId,
        parent_style: &crate::style::ContainerStyle,
        text: &str,
    ) {
        let current_style = super::text_style(parent_style);

        // The derive passes whole runs of text and `{{ }}` values, so the
        // spaces between pieces are inside `text`.
        let normalized = super::collapse_white_space(text, current_style.white_space);
        // Sized by the layout's measure function (see `Ui::compute_layout`).
        let text_layout_style = super::text_leaf_style(&current_style);
        let node = self.taffy.new_leaf(text_layout_style.clone()).unwrap();
        if current_style.animates() {
            self.base_styles.insert(node, (text_layout_style, current_style.clone()));
        }
        self.render_data.insert(node, RenderData::new(crate::style::RenderKind::Text(normalized.into_owned()), current_style));
        self.append_child(parent, node);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TaffyElementWrapper<'a> {
    pub node: NodeId,
    pub taffy: &'a TaffyTree,
    pub metadata: &'a NodeMap<NodeMetadata>,
    pub meta: &'a NodeMetadata,
    pub keys: &'a NodeMap<u64>,
    pub pointer: &'a PointerState,
}

impl<'a> TaffyElementWrapper<'a> {
    fn at(&self, node: NodeId) -> Option<Self> {
        self.metadata.get(&node).map(|meta| TaffyElementWrapper { node, meta, ..*self })
    }

    /// The element sibling `step` (-1 or 1) children away.
    fn sibling(&self, step: isize) -> Option<Self> {
        let parent = self.taffy.parent(self.node)?;
        let siblings = self.taffy.children(parent).ok()?;
        let index = siblings.iter().position(|&x| x == self.node)?;
        let element = |&sibling: &NodeId| self.at(sibling).filter(|s| s.meta.tag != "#text");
        if step < 0 {
            siblings[..index].iter().rev().find_map(element)
        } else {
            siblings[index + 1..].iter().find_map(element)
        }
    }
}

impl<'a> xerune_css::selector::Element for TaffyElementWrapper<'a> {
    fn parent(&self) -> Option<Self> {
        core::iter::successors(self.taffy.parent(self.node), |&n| self.taffy.parent(n)).find_map(|n| self.at(n))
    }

    fn prev_sibling(&self) -> Option<Self> {
        self.sibling(-1)
    }

    fn next_sibling(&self) -> Option<Self> {
        self.sibling(1)
    }

    fn tag(&self) -> &str {
        &self.meta.tag
    }

    fn attribute(&self, name: &str) -> Option<Cow<'_, str>> {
        let field = match name {
            "class" => self.meta.class.as_deref(),
            "id" => self.meta.id.as_deref(),
            _ => None,
        };
        field
            .or_else(|| self.meta.other_attrs.iter().flatten().find(|(k, _)| k == name).map(|(_, v)| v.as_str()))
            .or_else(|| self.meta.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()))
            .map(Cow::Borrowed)
    }

    fn is_empty(&self) -> bool {
        self.meta.children.is_empty()
    }

    fn state(&self, state: xerune_css::selector::State) -> bool {
        use xerune_css::selector::State;
        let in_chain = |chain: &[u64]| self.keys.get(self.node).is_some_and(|key| chain.contains(key));
        match state {
            State::Checked => self.meta.checked == Some(true),
            State::Hover => in_chain(&self.pointer.hovered),
            State::Active => in_chain(&self.pointer.active),
            _ => false,
        }
    }
}

/// Style of a derive-generated template element while it is being built.
///
/// The element-building steps are out-of-line functions taking this bundle so
/// that the generated `build_ui` doesn't inline style setup and drop glue for
/// every element in the template.
#[derive(Clone)]
pub struct ElementStyle {
    tag: Cow<'static, str>,
    layout: Style,
    /// The element's computed style; its children inherit from this.
    pub style: crate::style::ContainerStyle,
    parsed: super::attributes::ParsedAttributes,
}

/// The message prefix of a `data-on-input` / `data-on-change` (`attribute`),
/// if `M` takes it with a value.
fn value_handler<M: core::str::FromStr>(attribute: &str, prefix: &str) -> Option<String> {
    if format!("{prefix}:0").parse::<M>().is_err() {
        crate::diag::warn_once(&format!("{attribute} is not a message prefix of the model that takes a value"), prefix);
        return None;
    }
    Some(prefix.to_string())
}

impl ElementStyle {
    /// Clicking the element sends `message` (a compiled `data-on-click`).
    pub fn on_click<M: 'static>(&mut self, message: M) {
        self.parsed.interaction_id = Some(Interaction::Message(Rc::new(message)));
    }

    /// Clicking the element sends `message` parsed as `M` (a `data-on-click`
    /// the template can't build directly, like `digit:7`). Parsed now, so a
    /// mistake is reported when the view is built, not silently on click.
    pub fn on_click_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_click(message),
            Err(_) => crate::diag::warn_once("data-on-click is not a message of the model", message),
        }
    }

    /// The pointer entering the element sends `message` (a compiled
    /// `data-on-hover`).
    pub fn on_hover<M: 'static>(&mut self, message: M) {
        self.parsed.hover.enter = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_hover`] with a message parsed as `M`.
    pub fn on_hover_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_hover(message),
            Err(_) => crate::diag::warn_once("data-on-hover is not a message of the model", message),
        }
    }

    /// The pointer leaving the element sends `message` (a compiled
    /// `data-on-hover-out`).
    pub fn on_hover_out<M: 'static>(&mut self, message: M) {
        self.parsed.hover.leave = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_hover_out`] with a message parsed as `M`.
    pub fn on_hover_out_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_hover_out(message),
            Err(_) => crate::diag::warn_once("data-on-hover-out is not a message of the model", message),
        }
    }

    /// A transition of the element being created sends `message` (a compiled
    /// `data-on-transitionrun`).
    pub fn on_transitionrun<M: 'static>(&mut self, message: M) {
        self.parsed.transition.run = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_transitionrun`] with a message parsed as `M`.
    pub fn on_transitionrun_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_transitionrun(message),
            Err(_) => crate::diag::warn_once("data-on-transitionrun is not a message of the model", message),
        }
    }

    /// A transition of the element starting to move sends `message` (a compiled
    /// `data-on-transitionstart`).
    pub fn on_transitionstart<M: 'static>(&mut self, message: M) {
        self.parsed.transition.start = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_transitionstart`] with a message parsed as `M`.
    pub fn on_transitionstart_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_transitionstart(message),
            Err(_) => crate::diag::warn_once("data-on-transitionstart is not a message of the model", message),
        }
    }

    /// A transition of the element ending sends `message` (a compiled
    /// `data-on-transitionend`).
    pub fn on_transitionend<M: 'static>(&mut self, message: M) {
        self.parsed.transition.end = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_transitionend`] with a message parsed as `M`.
    pub fn on_transitionend_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_transitionend(message),
            Err(_) => crate::diag::warn_once("data-on-transitionend is not a message of the model", message),
        }
    }

    /// A transition of the element being cancelled sends `message` (a compiled
    /// `data-on-transitioncancel`).
    pub fn on_transitioncancel<M: 'static>(&mut self, message: M) {
        self.parsed.transition.cancel = Some(Interaction::Message(Rc::new(message)));
    }

    /// [`Self::on_transitioncancel`] with a message parsed as `M`.
    pub fn on_transitioncancel_text<M: core::str::FromStr + 'static>(&mut self, message: &str) {
        match message.parse::<M>() {
            Ok(message) => self.on_transitioncancel(message),
            Err(_) => crate::diag::warn_once("data-on-transitioncancel is not a message of the model", message),
        }
    }

    /// Dragging the element (a range input) sends `prefix:<value>` parsed
    /// as `M` for each new value (a `data-on-input`). Checked now with the
    /// value `0`, so a prefix the model doesn't take is reported when the
    /// view is built.
    pub fn on_input<M: core::str::FromStr + 'static>(&mut self, prefix: &str) {
        self.parsed.value.input = value_handler::<M>("data-on-input", prefix);
    }

    /// Letting go of the element (a range input) with a new value sends
    /// `prefix:<value>` parsed as `M` (a `data-on-change`); see [`Self::on_input`].
    pub fn on_change<M: core::str::FromStr + 'static>(&mut self, prefix: &str) {
        self.parsed.value.change = value_handler::<M>("data-on-change", prefix);
    }

    /// Starts from the default style of `tag`, inheriting from `parent_style`.
    #[inline(never)]
    pub fn new(tag: &'static str, parent_style: &crate::style::ContainerStyle) -> Self {
        Self::with_defaults(Cow::Borrowed(tag), tag, parent_style)
    }

    /// An element `tag` (of a tree built at runtime) starting from the
    /// defaults [`get_default_style`](crate::defaults::get_default_style)
    /// has for `defaults_key`.
    pub(crate) fn with_defaults(tag: Cow<'static, str>, defaults_key: &str, parent_style: &crate::style::ContainerStyle) -> Self {
        let defaults = crate::defaults::get_default_style(defaults_key, parent_style);
        Self {
            tag,
            layout: defaults.taffy_style,
            style: defaults.container_style,
            parsed: super::attributes::ParsedAttributes::new(defaults.element_type),
        }
    }

    /// Reads the element's attributes (`style`, `type`, `value`, ...).
    pub(crate) fn read_attributes<'a>(&mut self, attrs: impl IntoIterator<Item = (&'a str, &'a str)>, message_validator: &dyn Fn(&str) -> bool) {
        super::attributes::parse_attributes_generic(&self.tag, attrs, &mut self.style, &mut self.layout, &mut self.parsed, &message_validator);
    }

    /// The element's layout and style once its box is resolved: what a
    /// new element starts from by `@starting-style`.
    pub(crate) fn into_box(mut self, parent_style: &crate::style::ContainerStyle) -> (Style, crate::style::ContainerStyle) {
        super::style_resolution::resolve_box(parent_style, &mut self.layout, &mut self.style);
        (self.layout, self.style)
    }

    /// Like [`ElementStyle::new`] for an `<input>` with a literal `type`:
    /// starts from that widget's defaults so matched CSS can override them.
    #[inline(never)]
    pub fn new_input(input_type: &str, parent_style: &crate::style::ContainerStyle) -> Self {
        let mut el = Self::new("input", parent_style);
        if let Some(key) = crate::defaults::input_defaults_key(input_type) {
            let defaults = crate::defaults::get_default_style(key, parent_style);
            el.layout = defaults.taffy_style;
            el.style = defaults.container_style;
            el.parsed.element_type = defaults.element_type;
        }
        el
    }

    /// Applies matched CSS `(property, value)` declarations.
    #[inline(never)]
    pub fn apply(&mut self, declarations: &[(&str, &str)]) {
        crate::css::apply_declarations(declarations, &mut self.style, &mut self.layout);
    }

    /// Applies a template's matched CSS declarations, parsed once.
    #[inline(never)]
    pub fn apply_declared(&mut self, declarations: &crate::css::declared::Declarations) {
        declarations.apply(&mut self.style, &mut self.layout);
    }

    /// Applies the `!important` declarations of the `style` attribute in
    /// `attrs`, after the stylesheet's.
    pub fn apply_important_style(&mut self, attrs: &[(Cow<'static, str>, Cow<'_, str>)]) {
        if let Some((_, style)) = attrs.iter().find(|(name, _)| name == "style") {
            self.apply_important_inline(style);
        }
    }

    /// Applies the `!important` declarations of a `style` attribute.
    pub(crate) fn apply_important_inline(&mut self, style: &str) {
        crate::css::parse_inline_style_important(style, &mut self.style, &mut self.layout);
    }

    /// Applies CSS declarations for the element's pseudo-element `part`
    /// (`::before`, a slider's thumb), kept as written until it is built.
    pub fn apply_part(&mut self, part: crate::style::ControlPart, declarations: &[(&str, &str)]) {
        self.style.part_declarations.extend(declarations.iter().map(|&(name, value)| crate::style::PartDeclaration {
            part,
            name: name.into(),
            value: value.into(),
        }));
    }
}
