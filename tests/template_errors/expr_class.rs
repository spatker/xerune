use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Nop,
}

#[derive(XeruneTemplate)]
#[template(source = r#"
<style>.a { color: red; }</style><p class="{{ name }}">x</p>
"#)]
struct Page {
    on: bool,
    name: String,
    items: Vec<String>,
}

impl Model for Page {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

fn main() {}
