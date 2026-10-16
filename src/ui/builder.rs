#![allow(missing_docs)]

use taffy::prelude::*;
use crate::style::RenderData;
use super::node_map::NodeMap;
use super::metadata::NodeMetadata;
use super::{Interaction, Handle};
use crate::alloc_prelude::*;

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
        }
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
        self.render_data.insert(root, RenderData::Container(default_style.clone()));
        self.base_styles.insert(root, (layout, default_style.clone()));
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

    #[cfg(feature = "std")]
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

    #[cfg(feature = "std")]
    pub fn create_element(&mut self, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        self.note_root_element(tag, id);
        let metadata = NodeMetadata::new(std::borrow::Cow::Owned(tag.to_string()), class, id_val, style, other_attrs);
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_text(&mut self, text: &str, _attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("#text"), None, None, None, None);
        metadata.text = Some(text.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_checkbox(&mut self, checked: bool, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.checked = Some(checked);
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_slider(&mut self, value: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.slider_value = Some(value);
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_input_text(&mut self, value: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.input_text = Some(value.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_progress(&mut self, value: f32, max: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("progress"), class, id_val, style, other_attrs);
        metadata.progress_value = Some(value);
        metadata.progress_max = Some(max);
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
    pub fn create_image(&mut self, src: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("img"), class, id_val, style, other_attrs);
        metadata.image_src = Some(src.to_string());
        self.node_metadata.insert(id, metadata);
        id
    }

    #[cfg(feature = "std")]
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
    ) -> NodeId {
        super::attributes::parse_attribute_slice(el.tag, attrs, &mut el.style, &mut el.layout, &mut el.parsed, message_validator);
        let node = self.taffy.new_leaf(el.layout.clone()).unwrap();
        self.append_child(parent, node);
        if let Some((_, id)) = attrs.iter().find(|(name, _)| name == "id") {
            self.element_ids.push((Cow::Owned(id.clone().into_owned()), node));
        }
        self.note_root_element(el.tag, node);
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
        super::style_resolution::finalize_node_style(
            node,
            el.tag,
            parent_style,
            &mut el.layout,
            &mut el.style,
            &el.parsed,
            &mut self.taffy,
            &mut self.render_data,
            &mut self.interactions,
            &mut self.base_styles,
        );
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
        self.render_data.insert(node, RenderData::Text(normalized.into_owned(), current_style.clone()));
        self.base_styles.insert(node, (text_layout_style, current_style));
        self.append_child(parent, node);
    }
}

#[cfg(feature = "std")]
pub(crate) struct TaffyElementWrapper<'a> {
    pub node: NodeId,
    pub taffy: &'a TaffyTree,
    pub metadata: &'a NodeMap<NodeMetadata>,
    pub meta: &'a NodeMetadata,
}

#[cfg(feature = "std")]
impl<'a> simplecss::Element for TaffyElementWrapper<'a> {
    fn parent_element(&self) -> Option<Self> {
        let mut curr = self.node;
        while let Some(parent) = self.taffy.parent(curr) {
            if let Some(parent_meta) = self.metadata.get(&parent) {
                return Some(TaffyElementWrapper {
                    node: parent,
                    taffy: self.taffy,
                    metadata: self.metadata,
                    meta: parent_meta,
                });
            }
            curr = parent;
        }
        None
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let parent = self.taffy.parent(self.node)?;
        let siblings = self.taffy.children(parent).ok()?;
        let idx = siblings.iter().position(|&x| x == self.node)?;
        if idx > 0 {
            for &sibling in siblings[..idx].iter().rev() {
                if let Some(sibling_meta) = self.metadata.get(&sibling) {
                    return Some(TaffyElementWrapper {
                        node: sibling,
                        taffy: self.taffy,
                        metadata: self.metadata,
                        meta: sibling_meta,
                    });
                }
            }
        }
        None
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.meta.tag == name
    }

    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        if let ("checked", Some(checked)) = (local_name, self.meta.checked) {
            return checked && operator.matches("");
        }
        if local_name == "class" {
            if let Some(ref class) = self.meta.class {
                return operator.matches(class);
            }
        } else if local_name == "id" {
            if let Some(ref id) = self.meta.id {
                return operator.matches(id);
            }
        }
        if let Some(ref other) = self.meta.other_attrs {
            for (k, v) in other {
                if k == local_name {
                    return operator.matches(v);
                }
            }
        }
        for (k, v) in &self.meta.attrs {
            if k == local_name {
                return operator.matches(v);
            }
        }
        false
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => {
                if let Some(parent) = self.taffy.parent(self.node) {
                    if let Ok(siblings) = self.taffy.children(parent) {
                        for &sibling in &siblings {
                            if self.metadata.contains_key(&sibling) {
                                return sibling == self.node;
                            }
                        }
                    }
                }
                true
            }
            _ => false,
        }
    }
}

/// Style of a derive-generated template element while it is being built.
///
/// The element-building steps are out-of-line functions taking this bundle so
/// that the generated `build_ui` doesn't inline style setup and drop glue for
/// every element in the template.
pub struct ElementStyle {
    tag: &'static str,
    layout: Style,
    /// The element's computed style; its children inherit from this.
    pub style: crate::style::ContainerStyle,
    parsed: super::attributes::ParsedAttributes,
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

    /// Starts from the default style of `tag`, inheriting from `parent_style`.
    #[inline(never)]
    pub fn new(tag: &'static str, parent_style: &crate::style::ContainerStyle) -> Self {
        let defaults = crate::defaults::get_default_style(tag, parent_style);
        Self {
            tag,
            layout: defaults.taffy_style,
            style: defaults.container_style,
            parsed: super::attributes::ParsedAttributes::new(defaults.element_type),
        }
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
}
