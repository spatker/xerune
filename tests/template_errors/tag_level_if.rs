use xerune::*;

#[derive(Debug, PartialEq, XeruneMessage)]
enum Msg {
    Nop,
}

#[derive(XeruneTemplate)]
#[template(source = r#"
<p {% if on %}class="a"{% else %}class="b"{% endif %}>x</p>
"#)]
struct Page {
    on: bool,
    name: String,
    items: Vec<String>,
}

impl Model for Page {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context) {}
}

fn main() {}
