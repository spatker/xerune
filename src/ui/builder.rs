use taffy::prelude::*;
use crate::style::RenderData;
use super::node_map::NodeMap;
use super::metadata::NodeMetadata;
use super::{Interaction, Handle};

pub struct UiBuilder {
    pub taffy: TaffyTree,
    pub node_metadata: NodeMap<NodeMetadata>,
    pub render_data: NodeMap<RenderData>,
    pub interactions: NodeMap<Interaction>,
    pub node_to_handle: NodeMap<Handle>,
}

fn intern_string(s: &str) -> &'static str {
    thread_local! {
        static CACHE: std::cell::RefCell<std::collections::HashSet<&'static str>> = std::cell::RefCell::new(std::collections::HashSet::new());
    }
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(&interned) = cache.get(s) {
            interned
        } else {
            let interned: &'static str = Box::leak(s.to_string().into_boxed_str());
            cache.insert(interned);
            interned
        }
    })
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
        }
    }

    fn parse_attrs_cow(attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> (Option<std::borrow::Cow<'static, str>>, Option<std::borrow::Cow<'static, str>>, Option<String>, Option<Vec<(String, String)>>) {
        let mut class = None;
        let mut id = None;
        let mut style = None;
        let mut other_attrs = None;

        for (k, v) in attrs {
            match k.as_ref() {
                "class" => class = Some(std::mem::take(v)),
                "id" => id = Some(std::mem::take(v)),
                "style" => style = Some(std::mem::take(v).into_owned()),
                _ => {
                    let vec = other_attrs.get_or_insert_with(Vec::new);
                    vec.push((std::mem::take(k).into_owned(), std::mem::take(v).into_owned()));
                }
            }
        }
        (class, id, style, other_attrs)
    }

    pub fn create_element_cow(&mut self, tag: std::borrow::Cow<'static, str>, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata::new(tag, class, id_val, style, other_attrs);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_text_cow(&mut self, text: std::borrow::Cow<'static, str>, _attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("#text"), None, None, None, None);
        metadata.text = Some(text.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_checkbox_cow(&mut self, checked: bool, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.checked = Some(checked);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_slider_cow(&mut self, value: f32, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.slider_value = Some(value);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_input_text_cow(&mut self, value: std::borrow::Cow<'static, str>, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("input"), class, id_val, style, other_attrs);
        metadata.input_text = Some(value.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_progress_cow(&mut self, value: f32, max: f32, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("progress"), class, id_val, style, other_attrs);
        metadata.progress_value = Some(value);
        metadata.progress_max = Some(max);
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_image_cow(&mut self, src: std::borrow::Cow<'static, str>, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("img"), class, id_val, style, other_attrs);
        metadata.image_src = Some(src.into_owned());
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_canvas_cow(&mut self, canvas_id: std::borrow::Cow<'static, str>, attrs: &mut [(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let mut metadata = NodeMetadata::new(std::borrow::Cow::Borrowed("canvas"), class, id_val, style, other_attrs);
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
                "class" => class = Some(std::borrow::Cow::Borrowed(intern_string(v))),
                "id" => id = Some(std::borrow::Cow::Borrowed(intern_string(v))),
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
        let metadata = NodeMetadata::new(std::borrow::Cow::Borrowed(intern_string(tag)), class, id_val, style, other_attrs);
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
}

pub(crate) struct TaffyElementWrapper<'a> {
    pub node: NodeId,
    pub taffy: &'a TaffyTree,
    pub metadata: &'a NodeMap<NodeMetadata>,
    pub meta: &'a NodeMetadata,
}

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
