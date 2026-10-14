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
        }
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
        node
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

    /// Creates a text leaf styled from `parent_style` and appends it to
    /// `parent`. Used by derive-generated templates; kept out of line so each
    /// text node in a template doesn't inline its own copy.
    pub fn append_text_node(
        &mut self,
        measurer: &impl crate::TextMeasurer,
        parent: NodeId,
        parent_style: &crate::style::ContainerStyle,
        text: &str,
    ) {
        let mut current_style = parent_style.clone();
        current_style.background_color = None;
        current_style.background_gradient = None;
        current_style.box_shadow = None;
        current_style.border_width = 0.0;
        current_style.border_radius = 0.0;
        current_style.border_color = None;
        current_style.overflow = crate::Overflow::Visible;
        current_style.animation_name = None;
        current_style.animation_duration = 0.0;
        current_style.animation_timing_function = Arc::from("ease");
        current_style.animation_delay = 0.0;
        current_style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
        current_style.animation_direction = Arc::from("normal");
        current_style.animation_fill_mode = Arc::from("none");
        current_style.animation_play_state = Arc::from("running");

        let mut normalized = super::normalize_text(text);
        // Text beside a `{{ }}` value is its own box: keep one edge space so
        // the two don't run together. Whitespace-only text has no content.
        let lead = text.starts_with(char::is_whitespace);
        let trail = text.ends_with(char::is_whitespace);
        if (lead || trail) && !normalized.is_empty() {
            normalized = Cow::Owned(format!("{}{}{}", if lead { " " } else { "" }, normalized, if trail { " " } else { "" }));
        }
        let (width, height) = measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
        let text_layout_style = Style {
            size: Size { width: Dimension::length(width), height: Dimension::length(height) },
            ..Style::default()
        };
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

    /// Applies matched CSS `(property, value)` declarations.
    #[inline(never)]
    pub fn apply(&mut self, declarations: &[(&str, &str)]) {
        crate::css::apply_declarations(declarations, &mut self.style, &mut self.layout);
    }
}
