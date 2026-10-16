use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Nop,
}

#[derive(XeruneTemplate)]
#[template(path = "tests/template_errors/does_not_exist.html")]
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
