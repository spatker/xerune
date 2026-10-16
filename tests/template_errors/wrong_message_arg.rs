use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Toggle(usize),
}

// `Toggle` takes an index; passing the name must not compile (it used to
// compile and drop the click at runtime).
#[derive(XeruneTemplate)]
#[template(source = r#"
<div>
    {% for item in items %}<p data-on-click="toggle:{{ item }}">{{ item }}</p>{% endfor %}
</div>
"#)]
struct Page {
    items: Vec<String>,
}

impl Model for Page {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context) {}
}

fn main() {}
