use taffy::prelude::*;
use taffy::TaffyError;

#[cfg(feature = "dynamic-parser")]
use html5ever::parse_document;
#[cfg(feature = "dynamic-parser")]
use html5ever::tendril::TendrilSink;
#[cfg(feature = "dynamic-parser")]
use markup5ever_rcdom::{Handle, NodeData, RcDom};

#[cfg(not(feature = "dynamic-parser"))]
pub type Handle = ();

use std::collections::HashMap;
#[cfg(feature = "dynamic-parser")]
use std::rc::Rc;

#[cfg(feature = "profile")]
use coarse_prof::profile;

#[cfg(not(feature = "profile"))]
macro_rules! profile {
    ($($tt:tt)*) => {};
}

use crate::graphics::{Canvas, DrawCommand, Rect, TextMeasurer};
use crate::style::{ContainerStyle, Overflow, RenderData, Display, TextAlign, Direction, MyJustifyContent, BoxSizing};
use crate::css;
use crate::defaults;

pub type Interaction = String;

#[cfg(feature = "dynamic-parser")]
struct ElementWrapper(Handle);

#[cfg(feature = "dynamic-parser")]
impl simplecss::Element for ElementWrapper {
    fn parent_element(&self) -> Option<Self> {
        let parent_weak = self.0.parent.take();
        let parent_opt = parent_weak.as_ref().and_then(|weak| weak.upgrade());
        self.0.parent.set(parent_weak);
        parent_opt.map(ElementWrapper)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        let parent = self.parent_element()?;
        let siblings = parent.0.children.borrow();
        let index = siblings.iter().position(|child| Rc::ptr_eq(child, &self.0))?;
        if index > 0 {
            for i in (0..index).rev() {
                let sibling = &siblings[i];
                if matches!(sibling.data, NodeData::Element { .. }) {
                    return Some(ElementWrapper(sibling.clone()));
                }
            }
        }
        None
    }

    fn has_local_name(&self, name: &str) -> bool {
        if let NodeData::Element { name: ref element_name, .. } = self.0.data {
            element_name.local.as_ref() == name
        } else {
            false
        }
    }



    fn attribute_matches(&self, local_name: &str, operator: simplecss::AttributeOperator<'_>) -> bool {
        if let NodeData::Element { ref attrs, .. } = self.0.data {
            for attr in attrs.borrow().iter() {
                if attr.name.local.as_ref() == local_name {
                    return operator.matches(attr.value.as_ref());
                }
            }
        }
        false
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => {
                if let Some(parent) = self.parent_element() {
                    let siblings = parent.0.children.borrow();
                    for child in siblings.iter() {
                        if matches!(child.data, NodeData::Element { .. }) {
                            return Rc::ptr_eq(child, &self.0);
                        }
                    }
                    false
                } else {
                    true
                }
            }
            _ => false,
        }
    }
}

#[cfg(feature = "dynamic-parser")]
fn extract_styles(handle: &Handle, css_accumulator: &mut String) {
    if let NodeData::Element { name, .. } = &handle.data {
        if name.local.as_ref() == "style" {
            for child in handle.children.borrow().iter() {
                if let NodeData::Text { contents } = &child.data {
                    css_accumulator.push_str(&contents.borrow());
                    css_accumulator.push('\n');
                }
            }
        }
    }
    for child in handle.children.borrow().iter() {
        extract_styles(child, css_accumulator);
    }
}

pub trait TemplateLayout {
    fn stylesheet(&self) -> &'static str;
    fn build_ui(&self, builder: &mut UiBuilder) -> NodeId;
}

#[derive(Clone, Debug)]
pub struct NodeMetadata {
    pub tag: std::borrow::Cow<'static, str>,
    pub attrs: Vec<(String, String)>,
    pub text: Option<String>,
    pub checked: Option<bool>,
    pub slider_value: Option<f32>,
    pub progress_value: Option<f32>,
    pub progress_max: Option<f32>,
    pub image_src: Option<String>,
    pub canvas_id: Option<String>,
    pub input_text: Option<String>,
    pub class: Option<std::borrow::Cow<'static, str>>,
    pub id: Option<std::borrow::Cow<'static, str>>,
    pub style: Option<String>,
    pub other_attrs: Option<Vec<(String, String)>>,
}

#[derive(Hash, PartialEq, Eq, Clone)]
pub struct StyleCacheKey {
    pub tag: std::borrow::Cow<'static, str>,
    pub class: Option<std::borrow::Cow<'static, str>>,
    pub id: Option<std::borrow::Cow<'static, str>>,
    pub other_attrs: Option<Vec<(String, String)>>,
    pub parent_font_size_bits: u32,
    pub parent_weight: u16,
    pub parent_color_u32: u32,
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

pub struct UiBuilder {
    pub taffy: TaffyTree,
    pub node_metadata: HashMap<NodeId, NodeMetadata>,
    pub render_data: HashMap<NodeId, RenderData>,
    pub interactions: HashMap<NodeId, Interaction>,
    pub node_to_handle: HashMap<NodeId, Handle>,
}

impl UiBuilder {
    pub fn new() -> Self {
        Self::with_capacity(128)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            taffy: TaffyTree::new(),
            node_metadata: HashMap::with_capacity(capacity),
            render_data: HashMap::with_capacity(capacity),
            interactions: HashMap::with_capacity(capacity),
            node_to_handle: HashMap::with_capacity(capacity),
        }
    }

    fn parse_attrs_cow(attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> (Option<std::borrow::Cow<'static, str>>, Option<std::borrow::Cow<'static, str>>, Option<String>, Option<Vec<(String, String)>>) {
        let mut class = None;
        let mut id = None;
        let mut style = None;
        let mut other_attrs = None;

        for (k, v) in attrs {
            match k.as_ref() {
                "class" => class = Some(v.clone()),
                "id" => id = Some(v.clone()),
                "style" => style = Some(v.to_string()),
                _ => {
                    let vec = other_attrs.get_or_insert_with(Vec::new);
                    vec.push((k.to_string(), v.to_string()));
                }
            }
        }
        (class, id, style, other_attrs)
    }

    pub fn create_element_cow(&mut self, tag: std::borrow::Cow<'static, str>, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag,
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_text_cow(&mut self, text: std::borrow::Cow<'static, str>, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("#text"),
            attrs: Vec::new(),
            text: Some(text.into_owned()),
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_checkbox_cow(&mut self, checked: bool, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: Some(checked),
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_slider_cow(&mut self, value: f32, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: Some(value),
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_input_text_cow(&mut self, value: std::borrow::Cow<'static, str>, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: Some(value.into_owned()),
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_progress_cow(&mut self, value: f32, max: f32, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("progress"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: Some(value),
            progress_max: Some(max),
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_image_cow(&mut self, src: std::borrow::Cow<'static, str>, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("img"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: Some(src.into_owned()),
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_canvas_cow(&mut self, canvas_id: std::borrow::Cow<'static, str>, attrs: &[(std::borrow::Cow<'static, str>, std::borrow::Cow<'static, str>)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs_cow(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("canvas"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: Some(canvas_id.into_owned()),
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
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
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed(intern_string(tag)),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_text(&mut self, text: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("#text"),
            attrs: Vec::new(),
            text: Some(text.to_string()),
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_checkbox(&mut self, checked: bool, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: Some(checked),
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_slider(&mut self, value: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: Some(value),
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_input_text(&mut self, value: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("input"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: None,
            input_text: Some(value.to_string()),
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_progress(&mut self, value: f32, max: f32, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("progress"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: Some(value),
            progress_max: Some(max),
            image_src: None,
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_image(&mut self, src: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("img"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: Some(src.to_string()),
            canvas_id: None,
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn create_canvas(&mut self, canvas_id: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = self.taffy.new_leaf(Style::default()).unwrap();
        let (class, id_val, style, other_attrs) = Self::parse_attrs(attrs);
        let metadata = NodeMetadata {
            tag: std::borrow::Cow::Borrowed("canvas"),
            attrs: Vec::new(),
            text: None,
            checked: None,
            slider_value: None,
            progress_value: None,
            progress_max: None,
            image_src: None,
            canvas_id: Some(canvas_id.to_string()),
            input_text: None,
            class,
            id: id_val,
            style,
            other_attrs,
        };
        self.node_metadata.insert(id, metadata);
        id
    }

    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        let _ = self.taffy.add_child(parent, child);
    }
}

struct TaffyElementWrapper<'a> {
    node: NodeId,
    taffy: &'a TaffyTree,
    metadata: &'a HashMap<NodeId, NodeMetadata>,
    meta: &'a NodeMetadata,
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

struct CachedStyles {
    stylesheet: simplecss::StyleSheet<'static>,
    keyframes: HashMap<String, css::KeyframesAnimation>,
    has_nth_or_last_child: bool,
}

static STYLESHEET_CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<&'static str, &'static CachedStyles>>> = std::sync::OnceLock::new();

pub struct Ui {
    pub taffy: TaffyTree,
    pub render_data: HashMap<NodeId, RenderData>,
    pub interactions: HashMap<NodeId, Interaction>,
    pub scroll_offsets: HashMap<NodeId, (f32, f32)>,
    pub root: NodeId,
    pub node_to_handle: HashMap<NodeId, Handle>,
    pub base_styles: HashMap<NodeId, (Style, ContainerStyle)>,
    pub keyframes: HashMap<String, css::KeyframesAnimation>,
}

impl Ui {
    #[cfg(feature = "dynamic-parser")]
    pub fn new(
        html: &str, 
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        profile!("ui_new_internal");
        let mut taffy = TaffyTree::new();
        let mut render_data = HashMap::new();
        let mut interactions = HashMap::new();
        let mut node_to_handle = HashMap::new();
        let mut base_styles = HashMap::new();

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut html.as_bytes())
            .unwrap();

        preprocess_dom(&dom.document);

        let mut css_str = String::new();
        extract_styles(&dom.document, &mut css_str);
        
        let keyframes = css::parse_keyframes(&css_str);
        
        let re_nth = regex::Regex::new(r":nth-child\(\s*(\d+)\s*\)").unwrap();
        let css_str = re_nth.replace_all(&css_str, ".nth-child-$1").into_owned();
        let css_str = css_str.replace(":last-child", ".last-child");
        let re_slash = regex::Regex::new(r"/[\d\.]+").unwrap();
        let css_str = re_slash.replace_all(&css_str, "").into_owned();
        
        let stylesheet = simplecss::StyleSheet::parse(&css_str);

        let root = dom_to_taffy(
            &mut taffy, 
            &dom.document, 
            measurer, 
            &mut render_data, 
            &mut interactions, 
            default_style,
            message_validator,
            &stylesheet,
            &mut node_to_handle,
            &mut base_styles,
        ).ok_or(TaffyError::ChildIndexOutOfBounds { parent: NodeId::new(0), child_index: 0, child_count: 0 })?;  

        Ok(Self {
            taffy,
            render_data,
            interactions,
            scroll_offsets: HashMap::new(),
            root,
            node_to_handle,
            base_styles,
            keyframes,
        })
    }

    fn preprocess_compiled_tree(
        taffy: &taffy::TaffyTree,
        node_metadata: &mut HashMap<NodeId, NodeMetadata>,
        node: NodeId,
    ) {
        if let Some(meta) = node_metadata.get(&node) {
            if meta.tag == "#text" {
                return;
            }
        } else {
            return;
        }

        if let Ok(children) = taffy.children(node) {
            let mut child_elements = Vec::new();
            for &child in &children {
                if let Some(child_meta) = node_metadata.get(&child) {
                    if child_meta.tag != "#text" {
                        child_elements.push(child);
                    }
                }
                Self::preprocess_compiled_tree(taffy, node_metadata, child);
            }

            let num_children = child_elements.len();
            for (i, child) in child_elements.iter().enumerate() {
                if let Some(child_meta) = node_metadata.get_mut(child) {
                    let index_class = format!("nth-child-{}", i + 1);
                    let is_last = i + 1 == num_children;
                    
                    // 1. Mutate optimized class field
                    if let Some(ref mut class) = child_meta.class {
                        let class_str = class.to_mut();
                        class_str.push(' ');
                        class_str.push_str(&index_class);
                        if is_last {
                            class_str.push_str(" last-child");
                        }
                    } else {
                        let mut class_val = index_class.clone();
                        if is_last {
                            class_val.push_str(" last-child");
                        }
                        child_meta.class = Some(std::borrow::Cow::Owned(class_val));
                    }

                    // 2. Mutate backward-compatible attrs field
                    let mut found_class = false;
                    for (k, v) in &mut child_meta.attrs {
                        if k == "class" {
                            v.push(' ');
                            v.push_str(&index_class);
                            if is_last {
                                v.push_str(" last-child");
                            }
                            found_class = true;
                            break;
                        }
                    }
                    if !found_class && !child_meta.attrs.is_empty() {
                        let mut class_val = index_class;
                        if is_last {
                            class_val.push_str(" last-child");
                        }
                        child_meta.attrs.push(("class".to_string(), class_val));
                    }
                }
            }
        }
    }

    pub fn new_compiled(
        model: &impl TemplateLayout,
        measurer: &impl TextMeasurer,
        default_style: ContainerStyle,
        message_validator: &impl Fn(&str) -> bool,
    ) -> Result<Self, TaffyError> {
        profile!("ui_new_compiled");
        let mut builder = UiBuilder::new();
        let root = {
            profile!("build_ui");
            model.build_ui(&mut builder)
        };

        let stylesheet_str = model.stylesheet();
        
        let cache = STYLESHEET_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
        let mut cache_guard = cache.lock().unwrap();
        let cached = if let Some(&c) = cache_guard.get(stylesheet_str) {
            c
        } else {
            let has_nth_or_last_child = stylesheet_str.contains(":nth-child") || stylesheet_str.contains(":last-child");
            let keyframes = css::parse_keyframes(stylesheet_str);
            
            let re_nth = regex::Regex::new(r":nth-child\(\s*(\d+)\s*\)").unwrap();
            let css_str = re_nth.replace_all(stylesheet_str, ".nth-child-$1").into_owned();
            let css_str = css_str.replace(":last-child", ".last-child");
            let re_slash = regex::Regex::new(r"/[\d\.]+").unwrap();
            let css_str = re_slash.replace_all(&css_str, "").into_owned();
            
            let static_css_str: &'static str = Box::leak(css_str.into_boxed_str());
            let stylesheet = simplecss::StyleSheet::parse(static_css_str);
            
            let cached_val: &'static CachedStyles = Box::leak(Box::new(CachedStyles {
                stylesheet,
                keyframes,
                has_nth_or_last_child,
            }));
            cache_guard.insert(stylesheet_str, cached_val);
            cached_val
        };
        // Drop cache_guard before resolving styles to prevent deadlocks and free up lock
        drop(cache_guard);

        // Preprocess the compiled tree to add nth-child and last-child classes before resolving styles
        if cached.has_nth_or_last_child {
            Self::preprocess_compiled_tree(&builder.taffy, &mut builder.node_metadata, root);
        }

        let mut base_styles = HashMap::with_capacity(128);
        let mut style_cache = HashMap::with_capacity(64);
        
        {
            profile!("resolve_styles");
            resolve_styles(
                &mut builder.taffy,
                root,
                measurer,
                &mut builder.render_data,
                &mut builder.interactions,
                default_style,
                message_validator,
                &cached.stylesheet,
                &builder.node_metadata,
                &mut base_styles,
                &mut style_cache,
            );
        }

        Ok(Self {
            taffy: builder.taffy,
            render_data: builder.render_data,
            interactions: builder.interactions,
            scroll_offsets: HashMap::new(),
            root,
            node_to_handle: builder.node_to_handle,
            base_styles,
            keyframes: cached.keyframes.clone(),
        })
    }

    pub fn handle_scroll(&mut self, x: f32, y: f32, delta_x: f32, delta_y: f32) -> bool {
        profile!("handle_scroll");
        if let Some(mut node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
            loop {
                if let Some(RenderData::Container(style)) = self.render_data.get(&node) {
                    if style.overflow == Overflow::Scroll {
                         let (mut sx, mut sy) = self.scroll_offsets.get(&node).copied().unwrap_or((0.0, 0.0));
                         sx -= delta_x;
                         sy -= delta_y;
                         
                         if let Ok(layout) = self.taffy.layout(node) {
                             let container_width = layout.size.width;
                             let container_height = layout.size.height;
                             
                             let mut content_width = 0.0f32;
                             let mut content_height = 0.0f32;
                             
                             if let Ok(children) = self.taffy.children(node) {
                                 for child in children {
                                     if let Ok(child_layout) = self.taffy.layout(child) {
                                         let right = child_layout.location.x + child_layout.size.width;
                                         let bottom = child_layout.location.y + child_layout.size.height;
                                         if right > content_width { content_width = right; }
                                         if bottom > content_height { content_height = bottom; }
                                     }
                                 }
                             }
                             
                             let max_sx = (content_width - container_width).max(0.0);
                             let max_sy = (content_height - container_height).max(0.0);
                             
                             sx = sx.clamp(0.0, max_sx);
                             sy = sy.clamp(0.0, max_sy);
                         }

                         self.scroll_offsets.insert(node, (sx, sy));
                         return true;
                    }
                }
                
                if let Some(parent) = self.taffy.parent(node) {
                    node = parent;
                } else {
                    break;
                }
            }
        }
        false
    }

    pub fn scroll_into_view(&mut self, interaction_id: &str) {
        let node_opt = self.interactions.iter().find(|(_, v)| *v == interaction_id).map(|(k, _)| *k);
        if let Some(node) = node_opt {
             let mut current = node;
             while let Some(parent) = self.taffy.parent(current) {
                 if let Some(RenderData::Container(style)) = self.render_data.get(&parent) {
                     if style.overflow == Overflow::Scroll {
                         if let Ok(parent_layout) = self.taffy.layout(parent) {
                             if let Ok(node_layout) = self.taffy.layout(node) {
                                  let (ck, cy) = self.scroll_offsets.get(&parent).copied().unwrap_or((0.0, 0.0));
                                  let node_y = node_layout.location.y; 
                                  
                                  let mut new_y = cy;
                                  if node_y < cy {
                                      new_y = node_y;
                                  } else if node_y + node_layout.size.height > cy + parent_layout.size.height {
                                      new_y = node_y + node_layout.size.height - parent_layout.size.height;
                                  }
                                  self.scroll_offsets.insert(parent, (ck, new_y));
                                  return;
                             }
                         }
                     }
                 }
                 current = parent;
             }
        }
    }

    pub fn compute_layout(&mut self, available_space: Size<AvailableSpace>) -> Result<(), TaffyError> {
        profile!("taffy_layout");
        self.taffy.compute_layout(self.root, available_space)?;
        Ok(())
    }

    pub fn build_commands(&self, _canvases: &HashMap<String, Canvas>, focused_id: Option<&str>) -> Vec<DrawCommand> {
        layout_to_draw_commands(
            &self.taffy,
            self.root,
            &self.render_data,
            &self.scroll_offsets,
            0.0,
            0.0,
            focused_id
        )
    }

    pub fn hit_test(&self, x: f32, y: f32) -> Option<(Interaction, NodeId)> {
         profile!("hit_test");
         if let Some(clicked_node) = hit_test_recursive(&self.taffy, self.root, &self.scroll_offsets, &self.render_data, x, y, 0.0, 0.0) {
             let mut current = Some(clicked_node);
             while let Some(node) = current {
                 if let Some(act) = self.interactions.get(&node) {
                     return Some((act.clone(), clicked_node));
                 }
                 current = self.taffy.parent(node);
             }
             return Some((String::new(), clicked_node)); // Return clicked node even without interaction
         }
         None
    }
}

// Struct to hold parsed attributes for easier passing
struct ParsedAttributes {
    element_type: defaults::ElementType,
    slider_value: f32,
    progress_value: f32,
    progress_max: f32,
    checkbox_checked: bool,
    interaction_id: Option<String>,
    image_src: String,
    canvas_id: String,
    element_id: Option<String>,
    text_input_text: Option<String>,
}

impl ParsedAttributes {
    fn new(element_type: defaults::ElementType) -> Self {
        Self {
            element_type,
            slider_value: 0.0,
            progress_value: 0.0,
            progress_max: 1.0,
            checkbox_checked: false,
            interaction_id: None,
            image_src: String::new(),
            canvas_id: String::new(),
            element_id: None,
            text_input_text: None,
        }
    }
}

fn parse_attributes_generic<'a>(
    tag: &str,
    attrs: impl IntoIterator<Item = (&'a str, &'a str)>,
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &impl Fn(&str) -> bool,
) {
    for (name, value) in attrs {
        match name {
            "id" => {
                parsed.canvas_id = value.to_string();
                parsed.element_id = Some(value.to_string());
            },
            "style" => css::parse_inline_style(value, current_style, layout_style),
            "type" if tag == "input" => {
                if value == "checkbox" {
                    parsed.element_type = defaults::ElementType::Checkbox;
                    layout_style.size = Size { width: length(20.0), height: length(20.0) };
                    layout_style.margin = taffy::geometry::Rect { left: length(5.0), right: length(5.0), top: length(0.0), bottom: length(0.0) };
                } else if value == "range" {
                    parsed.element_type = defaults::ElementType::Slider;
                    layout_style.size = Size { width: length(100.0), height: length(20.0) };
                } else if value == "text" {
                    let text_defaults = defaults::get_default_style("input_text", current_style);
                    parsed.element_type = text_defaults.element_type;
                    *layout_style = text_defaults.taffy_style;
                    *current_style = text_defaults.container_style;
                }
            },
            "value" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.slider_value = v.clamp(0.0, 1.0);
                    parsed.progress_value = v; 
                }
                parsed.text_input_text = Some(value.to_string());
            },
            "max" => {
                if let Ok(v) = value.parse::<f32>() {
                    parsed.progress_max = v;
                }
            },
            "checked" => {
                if value == "false" {
                    parsed.checkbox_checked = false;
                } else {
                    parsed.checkbox_checked = true;
                }
            },
            "width" => {
                 if let Ok(w) = value.parse::<f32>() {
                     layout_style.size.width = length(w);
                 }
             },
             "height" => {
                 if let Ok(h) = value.parse::<f32>() {
                     layout_style.size.height = length(h);
                 }
             },
             "src" => parsed.image_src = value.to_string(),
             "data-on-click" => {
                 if !message_validator(value) {
                     log::warn!("Invalid message in data-on-click: {}", value);
                 }
                 parsed.interaction_id = Some(value.to_string());
             }
             _ => {
                 log::debug!("Ignoring attribute: {} on tag: {}", name, tag);
             }
        }
    }
}

#[cfg(feature = "dynamic-parser")]
fn parse_attributes(
    tag: &str,
    attrs: &std::cell::Ref<Vec<markup5ever::Attribute>>,
    current_style: &mut ContainerStyle,
    layout_style: &mut Style,
    parsed: &mut ParsedAttributes,
    message_validator: &impl Fn(&str) -> bool,
) {
    parse_attributes_generic(
        tag,
        attrs.iter().map(|attr| (attr.name.local.as_ref(), attr.value.as_ref())),
        current_style,
        layout_style,
        parsed,
        message_validator,
    );
}

fn process_element_type(
    id: NodeId,
    parsed: &ParsedAttributes,
    current_style: ContainerStyle,
    render_data: &mut HashMap<NodeId, RenderData>,
) {
    match parsed.element_type {
        defaults::ElementType::Image => {
            render_data.insert(id, RenderData::Image(parsed.image_src.clone(), current_style));
        },
        defaults::ElementType::Checkbox => {
            render_data.insert(id, RenderData::Checkbox(parsed.checkbox_checked, current_style));
        },
        defaults::ElementType::Slider => {
            render_data.insert(id, RenderData::Slider(parsed.slider_value, current_style));
        },
        defaults::ElementType::Progress => {
            render_data.insert(id, RenderData::Progress(parsed.progress_value, parsed.progress_max, current_style));
        },
        defaults::ElementType::Canvas => {
            render_data.insert(id, RenderData::Canvas(parsed.canvas_id.clone(), current_style));
        },
        defaults::ElementType::TextInput => {
            render_data.insert(id, RenderData::TextInput(parsed.element_id.clone().unwrap_or_default(), parsed.text_input_text.clone(), current_style));
        },
        _ => {
            render_data.insert(id, RenderData::Container(current_style));
        }
    }
}

#[cfg(feature = "dynamic-parser")]
fn dom_to_taffy(
    taffy: &mut TaffyTree,
    handle: &Handle,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &simplecss::StyleSheet<'_>,
    node_to_handle: &mut HashMap<NodeId, Handle>,
    base_styles: &mut HashMap<NodeId, (Style, ContainerStyle)>,
) -> Option<NodeId> {
    
    let mut current_style = parent_style.clone();
    current_style.background_color = None;
    current_style.background_gradient = None;
    current_style.border_width = 0.0;
    current_style.border_radius = 0.0;
    current_style.border_color = None;
    current_style.overflow = Overflow::Visible;

    match &handle.data {
        NodeData::Document => {
             let mut children = Vec::new();
             for child in handle.children.borrow().iter() {
                 if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator, stylesheet, node_to_handle, base_styles) {
                     children.push(id);
                 }
            }
            let id = taffy.new_with_children(Style::default(), &children).ok()?;
            render_data.insert(id, RenderData::Container(current_style.clone()));
            base_styles.insert(id, (Style::default(), current_style));
            node_to_handle.insert(id, handle.clone());
            Some(id)
        },
        
        NodeData::Element { name, attrs, .. } => {
            let tag = name.local.as_ref();
            if tag == "style" || tag == "script" || tag == "head" {
                return None;
            }
            
            let defaults = defaults::get_default_style(tag, &current_style); 
            let mut layout_style = defaults.taffy_style;
            current_style = defaults.container_style;
            
            let mut parsed = ParsedAttributes::new(defaults.element_type);
            
            // Match CSS stylesheet rules
            let el_wrapper = ElementWrapper(handle.clone());
            for rule in &stylesheet.rules {
                if rule.selector.matches(&el_wrapper) {
                    for decl in &rule.declarations {
                        let name_lower;
                        let name = if decl.name.chars().any(|c| c.is_ascii_uppercase()) {
                            name_lower = decl.name.to_lowercase();
                            &name_lower
                        } else {
                            decl.name
                        };
                        css::apply_declaration(name, decl.value, &mut current_style, &mut layout_style);
                    }
                }
            }
            parse_attributes(tag, &attrs.borrow(), &mut current_style, &mut layout_style, &mut parsed, message_validator);





            // Resolve logical properties (inline-size, block-size, etc.)
            if let Some(d) = current_style.inline_size {
                layout_style.size.width = d;
                if d == taffy::style::Dimension::length(d.value()) {
                    current_style.width = Some(d.value());
                }
            }
            if let Some(d) = current_style.block_size {
                layout_style.size.height = d;
                if d == taffy::style::Dimension::length(d.value()) {
                    current_style.height = Some(d.value());
                }
            }
            if let Some(d) = current_style.min_inline_size {
                layout_style.min_size.width = d;
            }
            if let Some(d) = current_style.max_inline_size {
                layout_style.max_size.width = d;
            }
            if let Some(d) = current_style.min_block_size {
                layout_style.min_size.height = d;
            }
            if let Some(d) = current_style.max_block_size {
                layout_style.max_size.height = d;
            }

            // Convert box sizes to match Taffy's expected box model:
            // - Taffy expects Style::size to be border-box.
            // - Taffy expects Style::min_size and Style::max_size to be content-box.
            let add_h = current_style.padding_left + current_style.padding_right + current_style.border_width * 2.0;
            let add_v = current_style.padding_top + current_style.padding_bottom + current_style.border_width * 2.0;

            if current_style.box_sizing == BoxSizing::ContentBox {
                layout_style.size.width = to_border_box(layout_style.size.width, add_h);
                layout_style.size.height = to_border_box(layout_style.size.height, add_v);
            } else {
                layout_style.min_size.width = to_content_box(layout_style.min_size.width, add_h);
                layout_style.min_size.height = to_content_box(layout_style.min_size.height, add_v);
                layout_style.max_size.width = to_content_box(layout_style.max_size.width, add_h);
                layout_style.max_size.height = to_content_box(layout_style.max_size.height, add_v);
            }

            if layout_style.position == Position::Absolute {
                enum Alignment {
                    Start,
                    End,
                    Center,
                }

                if layout_style.inset.left.is_auto() && layout_style.inset.right.is_auto() {
                    let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
                    let h_align = if is_parent_row {
                        let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                        match jc {
                            MyJustifyContent::FlexStart => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::FlexEnd => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            MyJustifyContent::SpaceBetween => {
                                if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Start => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::End => {
                                if parent_style.direction == Direction::Rtl {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Left => {
                                Alignment::Start
                            }
                            MyJustifyContent::Right => {
                                Alignment::End
                            }
                        }
                    } else {
                        let align = layout_style.align_self.map(|s| match s {
                            AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                            AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                            AlignSelf::Center => AlignItems::Center,
                            AlignSelf::Baseline => AlignItems::Baseline,
                            AlignSelf::Stretch => AlignItems::Stretch,
                        }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                        match align {
                            AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                                if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            AlignItems::FlexEnd | AlignItems::End => {
                                if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            AlignItems::Center => {
                                Alignment::Center
                            }
                        }
                    };

                    match h_align {
                        Alignment::Start => {
                            layout_style.inset.left = length(parent_style.padding_left);
                        }
                        Alignment::End => {
                            if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                                layout_style.inset.left = length(pw - cw + parent_style.padding_left);
                            } else {
                                layout_style.inset.right = length(parent_style.padding_right);
                            }
                        }
                        Alignment::Center => {
                            if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                                layout_style.inset.left = length((pw - cw) / 2.0 + parent_style.padding_left);
                            } else {
                                layout_style.inset.left = length(parent_style.padding_left);
                            }
                        }
                    }
                }

                if layout_style.inset.top.is_auto() && layout_style.inset.bottom.is_auto() {
                    let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
                    let v_align = if !is_parent_row {
                        let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                        match jc {
                            MyJustifyContent::FlexStart => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::FlexEnd => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                                Alignment::Center
                            }
                            MyJustifyContent::SpaceBetween => {
                                if parent_style.flex_direction == FlexDirection::Column {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            MyJustifyContent::Start | MyJustifyContent::Left | MyJustifyContent::Right => {
                                Alignment::Start
                            }
                            MyJustifyContent::End => {
                                Alignment::End
                            }
                        }
                    } else {
                        let align = layout_style.align_self.map(|s| match s {
                            AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                            AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                            AlignSelf::Center => AlignItems::Center,
                            AlignSelf::Baseline => AlignItems::Baseline,
                            AlignSelf::Stretch => AlignItems::Stretch,
                        }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                        match align {
                            AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                                if parent_style.flex_wrap == FlexWrap::WrapReverse {
                                    Alignment::End
                                } else {
                                    Alignment::Start
                                }
                            }
                            AlignItems::FlexEnd | AlignItems::End => {
                                if parent_style.flex_wrap == FlexWrap::WrapReverse {
                                    Alignment::Start
                                } else {
                                    Alignment::End
                                }
                            }
                            AlignItems::Center => {
                                Alignment::Center
                            }
                        }
                    };

                    match v_align {
                        Alignment::Start => {
                            layout_style.inset.top = length(parent_style.padding_top);
                        }
                        Alignment::End => {
                            if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                                layout_style.inset.top = length(ph - ch + parent_style.padding_top);
                            } else {
                                layout_style.inset.bottom = length(parent_style.padding_bottom);
                            }
                        }
                        Alignment::Center => {
                            if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                                layout_style.inset.top = length((ph - ch) / 2.0 + parent_style.padding_top);
                            } else {
                                layout_style.inset.top = length(parent_style.padding_top);
                            }
                        }
                    }
                }
            }
            
            let mut children = Vec::new();
            if !matches!(parsed.element_type, defaults::ElementType::Image | defaults::ElementType::Checkbox | defaults::ElementType::Slider | defaults::ElementType::Progress | defaults::ElementType::Canvas) {
                for child in handle.children.borrow().iter() {
                     if let Some(id) = dom_to_taffy(taffy, child, text_measurer, render_data, interactions, current_style.clone(), message_validator, stylesheet, node_to_handle, base_styles) {
                         children.push(id);
                     }
                }
            }

            if current_style.display == Display::Flex {
                children.sort_by_key(|child_id| {
                    render_data.get(child_id).map(|data| data.style().order).unwrap_or(0)
                });
            }

            if current_style.display == Display::None {
                layout_style.display = taffy::style::Display::None;
            } else if current_style.display != Display::Flex {
                let mut has_inline_child = false;
                for child_id in &children {
                    if let Some(child_data) = render_data.get(child_id) {
                        match child_data {
                            RenderData::Container(child_style) => {
                                if child_style.display == Display::InlineBlock || child_style.is_floated {
                                    has_inline_child = true;
                                    break;
                                }
                            }
                            _ => {
                                has_inline_child = true;
                                break;
                            }
                        }
                    }
                }

                if has_inline_child || tag == "tr" {
                    layout_style.flex_direction = FlexDirection::Row;
                    layout_style.flex_wrap = FlexWrap::Wrap;
                    if let Some(align) = current_style.text_align {
                        match align {
                            TextAlign::Right => layout_style.justify_content = Some(JustifyContent::FlexEnd),
                            TextAlign::Center => layout_style.justify_content = Some(JustifyContent::Center),
                            TextAlign::Left => layout_style.justify_content = Some(JustifyContent::FlexStart),
                        }
                    }
                } else {
                    layout_style.flex_direction = FlexDirection::Column;
                }
            }

            if parent_style.display != Display::Flex && current_style.display == Display::Block {
                if layout_style.size.width.is_auto() {
                    layout_style.size.width = Dimension::percent(1.0);
                }
            }

            layout_style.border = taffy::geometry::Rect {
                left: length(current_style.border_width),
                right: length(current_style.border_width),
                top: length(current_style.border_width),
                bottom: length(current_style.border_width),
            };

            if current_style.direction == Direction::Rtl {
                match layout_style.flex_direction {
                    FlexDirection::Row | FlexDirection::RowReverse => {
                        layout_style.flex_direction = match layout_style.flex_direction {
                            FlexDirection::Row => FlexDirection::RowReverse,
                            FlexDirection::RowReverse => FlexDirection::Row,
                            other => other,
                        };
                        layout_style.justify_content = match layout_style.justify_content {
                            Some(JustifyContent::FlexStart) | None => Some(JustifyContent::FlexEnd),
                            Some(JustifyContent::FlexEnd) => Some(JustifyContent::FlexStart),
                            other => other,
                        };
                    }
                    FlexDirection::Column | FlexDirection::ColumnReverse => {
                        layout_style.align_items = match layout_style.align_items {
                            Some(AlignItems::FlexStart) | None => Some(AlignItems::FlexEnd),
                            Some(AlignItems::FlexEnd) => Some(AlignItems::FlexStart),
                            other => other,
                        };
                        layout_style.align_content = match layout_style.align_content {
                            Some(AlignContent::FlexStart) | None => Some(AlignContent::FlexEnd),
                            Some(AlignContent::FlexEnd) => Some(AlignContent::FlexStart),
                            other => other,
                        };
                    }
                }
            }

            // Taffy's min_size and max_size constraints are relative to the border-box,
            // but CSS specifies them relative to the content-box. So we adjust them here.
            if !layout_style.max_size.height.is_auto() {
                let val = layout_style.max_size.height.value();
                if layout_style.max_size.height == Dimension::length(val) {
                    layout_style.max_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.max_size.width.is_auto() {
                let val = layout_style.max_size.width.value();
                if layout_style.max_size.width == Dimension::length(val) {
                    layout_style.max_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
                }
            }
            if !layout_style.min_size.height.is_auto() {
                let val = layout_style.min_size.height.value();
                if layout_style.min_size.height == Dimension::length(val) {
                    layout_style.min_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
                }
            }
            if !layout_style.min_size.width.is_auto() {
                let val = layout_style.min_size.width.value();
                if layout_style.min_size.width == Dimension::length(val) {
                    layout_style.min_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
                }
            }

            let is_parent_flex = parent_style.display == Display::Flex;
            if is_parent_flex {
                let resolved_align = match layout_style.align_self {
                    None => parent_style.align_items.unwrap_or(AlignItems::Stretch),
                    Some(AlignSelf::FlexStart) => AlignItems::FlexStart,
                    Some(AlignSelf::FlexEnd) => AlignItems::FlexEnd,
                    Some(AlignSelf::Center) => AlignItems::Center,
                    Some(AlignSelf::Baseline) => AlignItems::Baseline,
                    Some(AlignSelf::Stretch) => AlignItems::Stretch,
                    Some(AlignSelf::Start) => AlignItems::Start,
                    Some(AlignSelf::End) => AlignItems::End,
                };
                if resolved_align == AlignItems::Baseline {
                    let is_column = parent_style.flex_direction == FlexDirection::Column 
                        || parent_style.flex_direction == FlexDirection::ColumnReverse;
                    if is_column {
                        layout_style.align_self = Some(AlignSelf::FlexStart);
                    }
                }
            }



            let id = taffy.new_with_children(layout_style.clone(), &children).ok()?;

            process_element_type(id, &parsed, current_style.clone(), render_data);

            if let Some(interaction) = parsed.interaction_id {
                interactions.insert(id, interaction);
            }

            base_styles.insert(id, (layout_style, current_style));
            node_to_handle.insert(id, handle.clone());
            Some(id)
        },
        
        NodeData::Text { contents } => {
            let text = contents.borrow();
            let normalized = normalize_text(&text);
            
            if normalized.is_empty() {
                None
            } else {
                let (width, height) = text_measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                let text_layout_style = Style {
                    size: Size { width: length(width), height: length(height) },
                    ..Style::default()
                };
                let id = taffy.new_leaf(text_layout_style.clone()).ok()?;
                render_data.insert(id, RenderData::Text(normalized.into_owned(), current_style.clone()));
                base_styles.insert(id, (text_layout_style, current_style));
                node_to_handle.insert(id, handle.clone());
                Some(id)
            }
        },

        _ => {
            log::warn!("Unsupported NodeData encountered");
            None
        }
    }
}

fn layout_to_draw_commands(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    offset_x: f32,
    offset_y: f32,
    focused_id: Option<&str>,
) -> Vec<DrawCommand> {
    let mut commands = Vec::new();
    traverse_layout(taffy, root, render_data, scroll_offsets, offset_x, offset_y, &mut commands, focused_id);
    commands
}

fn traverse_layout(
    taffy: &TaffyTree,
    root: NodeId,
    render_data: &HashMap<NodeId, RenderData>,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    offset_x: f32,
    offset_y: f32,
    commands: &mut Vec<DrawCommand>,
    focused_id: Option<&str>,
) {
    let layout = match taffy.layout(root) {
        Ok(l) => l,
        Err(_) => return,
    };
    
    let x = offset_x + layout.location.x;
    let y = offset_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    let mut overflow = Overflow::Visible;
    let rect = Rect { x, y, width, height };

    if let Some(data) = render_data.get(&root) {
        let maybe_style = match data {
            RenderData::Container(style) => Some(style),
            RenderData::Text(_, style) => Some(style),
            _ => None,
        };

        if let Some(style) = maybe_style {
            overflow = style.overflow;
            if style.background_color.is_some() || style.background_gradient.is_some() || style.border_width > 0.0 {
                 commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: style.border_width,
                    border_color: style.border_color,
                });
            }
        }

        match data {
            RenderData::Text(text, style) => {
                commands.push(DrawCommand::DrawText {
                    text: text.clone(),
                    rect,
                    color: style.color,
                    font_size: style.font_size,
                    weight: style.weight,
                });
            },
            RenderData::Image(src, style) => {
                 commands.push(DrawCommand::DrawImage {
                    src: src.clone(),
                    rect,
                    border_radius: style.border_radius,
                });
            },
            RenderData::Checkbox(checked, style) => {
                commands.push(DrawCommand::DrawCheckbox {
                    rect,
                    checked: *checked,
                    color: style.color,
                });
            },
            RenderData::Slider(value, style) => {
                 commands.push(DrawCommand::DrawSlider {
                    rect,
                    value: *value,
                    color: style.color,
                });
            },
            RenderData::Progress(value, max, style) => {
                 commands.push(DrawCommand::DrawProgress {
                    rect,
                    value: *value,
                    max: *max,
                    color: style.color,
                });
            },
            RenderData::Canvas(id, _) => {
                commands.push(DrawCommand::DrawCanvas {
                    id: id.clone(),
                    rect,
                });
            },
            RenderData::TextInput(id, text, style) => {
                let is_focused = focused_id == Some(id.as_str()) && !id.is_empty();
                
                let focus_outline_width = if is_focused { 2.0 } else { 0.0 };
                let focus_outline_color = if is_focused { Some(crate::Color::from_rgba8(0, 122, 255, 255)) } else { None };

                // Draw background and focus ring
                commands.push(DrawCommand::DrawRect {
                    rect,
                    color: style.background_color,
                    gradient: style.background_gradient.clone(),
                    border_radius: style.border_radius,
                    border_width: if is_focused { focus_outline_width } else { style.border_width },
                    border_color: if is_focused { focus_outline_color } else { style.border_color },
                });

                // Draw text if present
                if let Some(t) = text {
                    if !t.is_empty() {
                        let text_rect = Rect {
                            x: rect.x + 8.0, // basic padding
                            y: rect.y + 5.0,
                            width: rect.width - 16.0,
                            height: rect.height - 10.0,
                        };
                        commands.push(DrawCommand::DrawText {
                            text: t.clone(),
                            rect: text_rect,
                            color: style.color,
                            font_size: style.font_size,
                            weight: style.weight,
                        });
                    }
                }
            },
            _ => {} 
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::Clip { rect });
    }

    let mut child_offset_x = x;
    let mut child_offset_y = y;

    if overflow == Overflow::Scroll {
        if let Some((sx, sy)) = scroll_offsets.get(&root) {
             child_offset_x -= sx;
             child_offset_y -= sy;
        }
    }

    if let Ok(children) = taffy.children(root) {
        for child in children {
            traverse_layout(taffy, child, render_data, scroll_offsets, child_offset_x, child_offset_y, commands, focused_id);
        }
    }

    if overflow != Overflow::Visible {
        commands.push(DrawCommand::PopClip);
    }
}

pub fn hit_test_recursive(
    taffy: &TaffyTree,
    root: NodeId,
    scroll_offsets: &HashMap<NodeId, (f32, f32)>,
    render_data: &HashMap<NodeId, RenderData>,
    x: f32,
    y: f32,
    abs_x: f32,
    abs_y: f32,
) -> Option<NodeId> {
    let layout = taffy.layout(root).ok()?;
    let left = abs_x + layout.location.x;
    let top = abs_y + layout.location.y;
    let width = layout.size.width;
    let height = layout.size.height;

    if x >= left && x <= left + width && y >= top && y <= top + height {
        let mut child_abs_x = left;
        let mut child_abs_y = top;

        if let Some(data) = render_data.get(&root) {
            let overflow = match data {
                RenderData::Container(style) => style.overflow,
                RenderData::TextInput(_, _, style) => style.overflow,
                _ => Overflow::Visible,
            };

            if overflow == Overflow::Scroll {
                if let Some((sx, sy)) = scroll_offsets.get(&root) {
                    child_abs_x -= sx;
                    child_abs_y -= sy;
                }
            }
        }

        if let Ok(children) = taffy.children(root) {
             for child in children.iter().rev() {
                 if let Some(hit) = hit_test_recursive(taffy, *child, scroll_offsets, render_data, x, y, child_abs_x, child_abs_y) {
                     return Some(hit);
                 }
             }
        }
        return Some(root);
    }
    None
}

fn normalize_text(text: &str) -> std::borrow::Cow<'_, str> {
    let mut needs_normalization = false;
    let mut last_was_space = false;
    let mut is_first = true;
    for c in text.chars() {
        if c.is_whitespace() {
            if is_first || last_was_space || c != ' ' {
                needs_normalization = true;
                break;
            }
            last_was_space = true;
        } else {
            last_was_space = false;
        }
        is_first = false;
    }
    if last_was_space && !text.is_empty() {
        needs_normalization = true;
    }
    if needs_normalization {
        std::borrow::Cow::Owned(text.split_whitespace().collect::<Vec<&str>>().join(" "))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

pub trait ToDisplayString {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str>;
}

impl ToDisplayString for str {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self)
    }
}

impl ToDisplayString for String {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(self.as_str())
    }
}

impl<T: ToDisplayString + ?Sized> ToDisplayString for &T {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        (*self).to_display_string()
    }
}

impl ToDisplayString for f32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for f64 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for i32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for u32 {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for usize {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Owned(self.to_string())
    }
}

impl ToDisplayString for bool {
    fn to_display_string(&self) -> std::borrow::Cow<'_, str> {
        std::borrow::Cow::Borrowed(if *self { "true" } else { "false" })
    }
}

pub(crate) fn resolve_styles(
    taffy: &mut TaffyTree,
    node: NodeId,
    text_measurer: &impl TextMeasurer,
    render_data: &mut HashMap<NodeId, RenderData>,
    interactions: &mut HashMap<NodeId, Interaction>,
    parent_style: ContainerStyle,
    message_validator: &impl Fn(&str) -> bool,
    stylesheet: &simplecss::StyleSheet<'_>,
    node_metadata: &HashMap<NodeId, NodeMetadata>,
    base_styles: &mut HashMap<NodeId, (Style, ContainerStyle)>,
    style_cache: &mut HashMap<StyleCacheKey, (Style, ContainerStyle)>,
) {
    let meta = match node_metadata.get(&node) {
        Some(m) => m,
        None => return,
    };

    if meta.tag == "#text" {
        let mut current_style = parent_style;
        current_style.background_color = None;
        current_style.background_gradient = None;
        current_style.border_width = 0.0;
        current_style.border_radius = 0.0;
        current_style.border_color = None;
        current_style.overflow = Overflow::Visible;
        current_style.animation_name = None;
        current_style.animation_duration = 0.0;
        current_style.animation_timing_function = std::sync::Arc::from("ease");
        current_style.animation_delay = 0.0;
        current_style.animation_iteration_count = crate::style::AnimationIterationCount::Count(1.0);
        current_style.animation_direction = std::sync::Arc::from("normal");
        current_style.animation_fill_mode = std::sync::Arc::from("none");
        current_style.animation_play_state = std::sync::Arc::from("running");

        if let Some(ref text) = meta.text {
            let normalized = normalize_text(text);
            if !normalized.is_empty() {
                let (width, height) = text_measurer.measure_text(&normalized, current_style.font_size, current_style.weight);
                let text_layout_style = Style {
                    size: Size { width: length(width), height: length(height) },
                    ..Style::default()
                };
                let _ = taffy.set_style(node, text_layout_style.clone());
                render_data.insert(node, RenderData::Text(normalized.into_owned(), current_style.clone()));
                base_styles.insert(node, (text_layout_style, current_style));
            }
        }
        return;
    }

    let tag = &meta.tag;
    let element_type = defaults::get_default_style(tag, &ContainerStyle::default()).element_type;
    let mut parsed = ParsedAttributes::new(element_type);

    if let Some(checked) = meta.checked {
        parsed.checkbox_checked = checked;
    }
    if let Some(slider_value) = meta.slider_value {
        parsed.slider_value = slider_value;
    }
    if let Some(progress_value) = meta.progress_value {
        parsed.progress_value = progress_value;
    }
    if let Some(progress_max) = meta.progress_max {
        parsed.progress_max = progress_max;
    }
    if let Some(ref image_src) = meta.image_src {
        parsed.image_src = image_src.clone();
    }
    if let Some(ref canvas_id) = meta.canvas_id {
        parsed.canvas_id = canvas_id.clone();
    }
    if let Some(ref input_text) = meta.input_text {
        parsed.text_input_text = Some(input_text.clone());
    }

    let c = parent_style.color;
    let parent_color_u32 = ((c.r as u32) << 24) | ((c.g as u32) << 16) | ((c.b as u32) << 8) | (c.a as u32);
    let cache_key = StyleCacheKey {
        tag: meta.tag.clone(),
        class: meta.class.clone(),
        id: meta.id.clone(),
        other_attrs: meta.other_attrs.clone(),
        parent_font_size_bits: parent_style.font_size.to_bits(),
        parent_weight: parent_style.weight,
        parent_color_u32,
    };

    let (mut layout_style, mut current_style) = if let Some(cached_styles) = style_cache.get(&cache_key) {
        cached_styles.clone()
    } else {
        let defaults = defaults::get_default_style(tag, &parent_style);
        let mut l_style = defaults.taffy_style;
        let mut c_style = defaults.container_style;

        let el_wrapper = TaffyElementWrapper {
            node,
            taffy,
            metadata: node_metadata,
            meta,
        };
        for rule in &stylesheet.rules {
            if rule.selector.matches(&el_wrapper) {
                for decl in &rule.declarations {
                    let name_lower;
                    let name = if decl.name.chars().any(|c| c.is_ascii_uppercase()) {
                        name_lower = decl.name.to_lowercase();
                        &name_lower
                    } else {
                        decl.name
                    };
                    css::apply_declaration(name, decl.value, &mut c_style, &mut l_style);
                }
            }
        }
        let pair = (l_style, c_style);
        style_cache.insert(cache_key, pair.clone());
        pair
    };

    // 1. Process id attribute
    let id_str = meta.id.as_deref().or_else(|| {
        for (k, v) in &meta.attrs {
            if k == "id" {
                return Some(v.as_str());
            }
        }
        None
    });
    if let Some(id_str) = id_str {
        parsed.canvas_id = id_str.to_string();
        parsed.element_id = Some(id_str.to_string());
    }

    // 2. Process inline styles
    let style_str = meta.style.as_deref().or_else(|| {
        for (k, v) in &meta.attrs {
            if k == "style" {
                return Some(v.as_str());
            }
        }
        None
    });
    if let Some(style_str) = style_str {
        css::parse_inline_style(style_str, &mut current_style, &mut layout_style);
    }

    // 3. Process other attributes using generic parser
    if let Some(ref other) = meta.other_attrs {
        parse_attributes_generic(
            tag,
            other.iter().map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    } else {
        // Fallback for when attributes are directly populated in meta.attrs (e.g., in WPT runner)
        // We filter out "class" and "style" since they are already processed / matched.
        parse_attributes_generic(
            tag,
            meta.attrs.iter().filter(|(k, _)| k != "style" && k != "class" && k != "id").map(|(k, v)| (k.as_str(), v.as_str())),
            &mut current_style,
            &mut layout_style,
            &mut parsed,
            message_validator,
        );
    }

    if let Some(d) = current_style.inline_size {
        layout_style.size.width = d;
        if d == taffy::style::Dimension::length(d.value()) {
            current_style.width = Some(d.value());
        }
    }
    if let Some(d) = current_style.block_size {
        layout_style.size.height = d;
        if d == taffy::style::Dimension::length(d.value()) {
            current_style.height = Some(d.value());
        }
    }
    if let Some(d) = current_style.min_inline_size {
        layout_style.min_size.width = d;
    }
    if let Some(d) = current_style.max_inline_size {
        layout_style.max_size.width = d;
    }
    if let Some(d) = current_style.min_block_size {
        layout_style.min_size.height = d;
    }
    if let Some(d) = current_style.max_block_size {
        layout_style.max_size.height = d;
    }

    let add_h = current_style.padding_left + current_style.padding_right + current_style.border_width * 2.0;
    let add_v = current_style.padding_top + current_style.padding_bottom + current_style.border_width * 2.0;

    if current_style.box_sizing == BoxSizing::ContentBox {
        layout_style.size.width = to_border_box(layout_style.size.width, add_h);
        layout_style.size.height = to_border_box(layout_style.size.height, add_v);
    } else {
        layout_style.min_size.width = to_content_box(layout_style.min_size.width, add_h);
        layout_style.min_size.height = to_content_box(layout_style.min_size.height, add_v);
        layout_style.max_size.width = to_content_box(layout_style.max_size.width, add_h);
        layout_style.max_size.height = to_content_box(layout_style.max_size.height, add_v);
    }

    if layout_style.position == Position::Absolute {
        enum Alignment {
            Start,
            End,
            Center,
        }

        if layout_style.inset.left.is_auto() && layout_style.inset.right.is_auto() {
            let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
            let h_align = if is_parent_row {
                let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                match jc {
                    MyJustifyContent::FlexStart => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    MyJustifyContent::FlexEnd => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                        Alignment::Center
                    }
                    MyJustifyContent::SpaceBetween => {
                        if (parent_style.flex_direction == FlexDirection::Row) ^ (parent_style.direction == Direction::Rtl) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    MyJustifyContent::Start => {
                        if parent_style.direction == Direction::Rtl {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    MyJustifyContent::End => {
                        if parent_style.direction == Direction::Rtl {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    MyJustifyContent::Left => {
                        Alignment::Start
                    }
                    MyJustifyContent::Right => {
                        Alignment::End
                    }
                }
            } else {
                let align = layout_style.align_self.map(|s| match s {
                    AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                    AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                    AlignSelf::Center => AlignItems::Center,
                    AlignSelf::Baseline => AlignItems::Baseline,
                    AlignSelf::Stretch => AlignItems::Stretch,
                }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                match align {
                    AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                        if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    AlignItems::FlexEnd | AlignItems::End => {
                        if (parent_style.direction == Direction::Rtl) ^ (parent_style.flex_wrap == FlexWrap::WrapReverse) {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    AlignItems::Center => {
                        Alignment::Center
                    }
                }
            };

            match h_align {
                Alignment::Start => {
                    layout_style.inset.left = length(parent_style.padding_left);
                }
                Alignment::End => {
                    if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                        layout_style.inset.left = length(pw - cw + parent_style.padding_left);
                    } else {
                        layout_style.inset.right = length(parent_style.padding_right);
                    }
                }
                Alignment::Center => {
                    if let (Some(pw), Some(cw)) = (parent_style.width, current_style.width) {
                        layout_style.inset.left = length((pw - cw) / 2.0 + parent_style.padding_left);
                    } else {
                        layout_style.inset.left = length(parent_style.padding_left);
                    }
                }
            }
        }

        if layout_style.inset.top.is_auto() && layout_style.inset.bottom.is_auto() {
            let is_parent_row = parent_style.flex_direction == FlexDirection::Row || parent_style.flex_direction == FlexDirection::RowReverse;
            let v_align = if !is_parent_row {
                let jc = parent_style.justify_content.unwrap_or(MyJustifyContent::FlexStart);
                match jc {
                    MyJustifyContent::FlexStart => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    MyJustifyContent::FlexEnd => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    MyJustifyContent::Center | MyJustifyContent::SpaceAround | MyJustifyContent::SpaceEvenly => {
                        Alignment::Center
                    }
                    MyJustifyContent::SpaceBetween => {
                        if parent_style.flex_direction == FlexDirection::Column {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    MyJustifyContent::Start | MyJustifyContent::Left | MyJustifyContent::Right => {
                        Alignment::Start
                    }
                    MyJustifyContent::End => {
                        Alignment::End
                    }
                }
            } else {
                let align = layout_style.align_self.map(|s| match s {
                    AlignSelf::FlexStart | AlignSelf::Start => AlignItems::FlexStart,
                    AlignSelf::FlexEnd | AlignSelf::End => AlignItems::FlexEnd,
                    AlignSelf::Center => AlignItems::Center,
                    AlignSelf::Baseline => AlignItems::Baseline,
                    AlignSelf::Stretch => AlignItems::Stretch,
                }).unwrap_or_else(|| parent_style.align_items.unwrap_or(AlignItems::Stretch));

                match align {
                    AlignItems::FlexStart | AlignItems::Stretch | AlignItems::Baseline | AlignItems::Start => {
                        if parent_style.flex_wrap == FlexWrap::WrapReverse {
                            Alignment::End
                        } else {
                            Alignment::Start
                        }
                    }
                    AlignItems::FlexEnd | AlignItems::End => {
                        if parent_style.flex_wrap == FlexWrap::WrapReverse {
                            Alignment::Start
                        } else {
                            Alignment::End
                        }
                    }
                    AlignItems::Center => {
                        Alignment::Center
                    }
                }
            };

            match v_align {
                Alignment::Start => {
                    layout_style.inset.top = length(parent_style.padding_top);
                }
                Alignment::End => {
                    if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                        layout_style.inset.top = length(ph - ch + parent_style.padding_top);
                    } else {
                        layout_style.inset.bottom = length(parent_style.padding_bottom);
                    }
                }
                Alignment::Center => {
                    if let (Some(ph), Some(ch)) = (parent_style.height, current_style.height) {
                        layout_style.inset.top = length((ph - ch) / 2.0 + parent_style.padding_top);
                    } else {
                        layout_style.inset.top = length(parent_style.padding_top);
                    }
                }
            }
        }
    }

    let children = taffy.children(node).unwrap_or_default();
    for &child in &children {
        resolve_styles(
            taffy,
            child,
            text_measurer,
            render_data,
            interactions,
            current_style.clone(),
            message_validator,
            stylesheet,
            node_metadata,
            base_styles,
            style_cache,
        );
    }

    if current_style.display == Display::Flex {
        if let Ok(mut chs) = taffy.children(node) {
            chs.sort_by_key(|child_id| {
                render_data.get(child_id).map(|data| data.style().order).unwrap_or(0)
            });
            let _ = taffy.set_children(node, &chs);
        }
    }

    if current_style.display == Display::None {
        layout_style.display = taffy::style::Display::None;
    } else if current_style.display != Display::Flex {
        let mut has_inline_child = false;
        if let Ok(chs) = taffy.children(node) {
            for child_id in &chs {
                if let Some(child_data) = render_data.get(child_id) {
                    match child_data {
                        RenderData::Container(child_style) => {
                            if child_style.display == Display::InlineBlock || child_style.is_floated {
                                has_inline_child = true;
                                break;
                            }
                        }
                        _ => {
                            has_inline_child = true;
                            break;
                        }
                    }
                }
            }
        }

        if has_inline_child || tag == "tr" {
            layout_style.flex_direction = FlexDirection::Row;
            layout_style.flex_wrap = FlexWrap::Wrap;
            if let Some(align) = current_style.text_align {
                match align {
                    TextAlign::Right => layout_style.justify_content = Some(JustifyContent::FlexEnd),
                    TextAlign::Center => layout_style.justify_content = Some(JustifyContent::Center),
                    TextAlign::Left => layout_style.justify_content = Some(JustifyContent::FlexStart),
                }
            }
        } else {
            layout_style.flex_direction = FlexDirection::Column;
        }
    }

    if parent_style.display != Display::Flex && current_style.display == Display::Block {
        if layout_style.size.width.is_auto() {
            layout_style.size.width = Dimension::percent(1.0);
        }
    }

    layout_style.border = taffy::geometry::Rect {
        left: length(current_style.border_width),
        right: length(current_style.border_width),
        top: length(current_style.border_width),
        bottom: length(current_style.border_width),
    };

    if current_style.direction == Direction::Rtl {
        match layout_style.flex_direction {
            FlexDirection::Row | FlexDirection::RowReverse => {
                layout_style.flex_direction = match layout_style.flex_direction {
                    FlexDirection::Row => FlexDirection::RowReverse,
                    FlexDirection::RowReverse => FlexDirection::Row,
                    other => other,
                };
                layout_style.justify_content = match layout_style.justify_content {
                    Some(JustifyContent::FlexStart) | None => Some(JustifyContent::FlexEnd),
                    Some(JustifyContent::FlexEnd) => Some(JustifyContent::FlexStart),
                    other => other,
                };
            }
            FlexDirection::Column | FlexDirection::ColumnReverse => {
                layout_style.align_items = match layout_style.align_items {
                    Some(AlignItems::FlexStart) | None => Some(AlignItems::FlexEnd),
                    Some(AlignItems::FlexEnd) => Some(AlignItems::FlexStart),
                    other => other,
                };
                layout_style.align_content = match layout_style.align_content {
                    Some(AlignContent::FlexStart) | None => Some(AlignContent::FlexEnd),
                    Some(AlignContent::FlexEnd) => Some(AlignContent::FlexStart),
                    other => other,
                };
            }
        }
    }

    if !layout_style.max_size.height.is_auto() {
        let val = layout_style.max_size.height.value();
        if layout_style.max_size.height == Dimension::length(val) {
            layout_style.max_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
        }
    }
    if !layout_style.max_size.width.is_auto() {
        let val = layout_style.max_size.width.value();
        if layout_style.max_size.width == Dimension::length(val) {
            layout_style.max_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
        }
    }
    if !layout_style.min_size.height.is_auto() {
        let val = layout_style.min_size.height.value();
        if layout_style.min_size.height == Dimension::length(val) {
            layout_style.min_size.height = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_top + current_style.padding_bottom);
        }
    }
    if !layout_style.min_size.width.is_auto() {
        let val = layout_style.min_size.width.value();
        if layout_style.min_size.width == Dimension::length(val) {
            layout_style.min_size.width = Dimension::length(val + current_style.border_width * 2.0 + current_style.padding_left + current_style.padding_right);
        }
    }

    let is_parent_flex = parent_style.display == Display::Flex;
    if is_parent_flex {
        let resolved_align = match layout_style.align_self {
            None => parent_style.align_items.unwrap_or(AlignItems::Stretch),
            Some(AlignSelf::FlexStart) => AlignItems::FlexStart,
            Some(AlignSelf::FlexEnd) => AlignItems::FlexEnd,
            Some(AlignSelf::Center) => AlignItems::Center,
            Some(AlignSelf::Baseline) => AlignItems::Baseline,
            Some(AlignSelf::Stretch) => AlignItems::Stretch,
            Some(AlignSelf::Start) => AlignItems::Start,
            Some(AlignSelf::End) => AlignItems::End,
        };
        if resolved_align == AlignItems::Baseline {
            let is_column = parent_style.flex_direction == FlexDirection::Column 
                || parent_style.flex_direction == FlexDirection::ColumnReverse;
            if is_column {
                layout_style.align_self = Some(AlignSelf::FlexStart);
            }
        }
    }

    let _ = taffy.set_style(node, layout_style.clone());

    process_element_type(node, &parsed, current_style.clone(), render_data);

    if let Some(interaction) = parsed.interaction_id {
        interactions.insert(node, interaction);
    }

    base_styles.insert(node, (layout_style, current_style));
}

#[cfg(feature = "dynamic-parser")]
fn preprocess_dom(handle: &Handle) {
    if let NodeData::Element { .. } = handle.data {
        // First, recursively process all child element nodes
        let mut child_elements = Vec::new();
        for child in handle.children.borrow().iter() {
            if let NodeData::Element { .. } = child.data {
                child_elements.push(child.clone());
            }
            preprocess_dom(child);
        }
        
        // Add nth-child and last-child classes to each child element
        let num_children = child_elements.len();
        for (i, child) in child_elements.iter().enumerate() {
            if let NodeData::Element { attrs: ref child_attrs, .. } = child.data {
                let index_class = format!("nth-child-{}", i + 1);
                let is_last = i + 1 == num_children;
                
                let mut attrs_borrow = child_attrs.borrow_mut();
                let mut class_attr_opt = None;
                for attr in attrs_borrow.iter_mut() {
                    if attr.name.local.as_ref() == "class" {
                        class_attr_opt = Some(attr);
                        break;
                    }
                }
                
                if let Some(class_attr) = class_attr_opt {
                    let mut val = class_attr.value.to_string();
                    val.push_str(" ");
                    val.push_str(&index_class);
                    if is_last {
                        val.push_str(" last-child");
                    }
                    class_attr.value = val.into();
                } else {
                    let mut class_val = index_class;
                    if is_last {
                        class_val.push_str(" last-child");
                    }
                    attrs_borrow.push(markup5ever::Attribute {
                        name: markup5ever::QualName::new(
                            None,
                            markup5ever::ns!(),
                            markup5ever::LocalName::from("class"),
                        ),
                        value: class_val.into(),
                    });
                }
            }
        }
    } else if let NodeData::Document = handle.data {
        for child in handle.children.borrow().iter() {
            preprocess_dom(child);
        }
    }
}

fn to_border_box(dim: taffy::style::Dimension, add: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length(dim.value() + add)
    } else {
        dim
    }
}

fn to_content_box(dim: taffy::style::Dimension, sub: f32) -> taffy::style::Dimension {
    if dim == taffy::style::Dimension::length(dim.value()) {
        taffy::style::Dimension::length((dim.value() - sub).max(0.0))
    } else {
        dim
    }
}
