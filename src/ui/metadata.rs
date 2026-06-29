use taffy::prelude::NodeId;

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
    pub children: Vec<NodeId>,
}

impl NodeMetadata {
    pub fn new(
        tag: std::borrow::Cow<'static, str>,
        class: Option<std::borrow::Cow<'static, str>>,
        id: Option<std::borrow::Cow<'static, str>>,
        style: Option<String>,
        other_attrs: Option<Vec<(String, String)>>,
    ) -> Self {
        Self {
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
            id,
            style,
            other_attrs,
            children: Vec::new(),
        }
    }
}
