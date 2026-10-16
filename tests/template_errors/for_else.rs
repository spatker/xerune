use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Nop,
}

#[derive(XeruneTemplate)]
#[template(source = r#"
{% for i in items %}<p>{{ i }}</p>{% else %}<p>none</p>{% endfor %}
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
