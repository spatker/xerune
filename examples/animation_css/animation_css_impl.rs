use xerune::{Model, XeruneTemplate};

#[derive(XeruneTemplate, serde::Serialize, serde::Deserialize)]
#[template(path = "animation_css.html")]
pub struct AnimationCssModel;

impl Model for AnimationCssModel {
    type Message = String;

    fn update(&mut self, _msg: Self::Message, _context: &mut xerune::Context) {}
}

