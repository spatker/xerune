use xerune::*;

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum Msg {
    Volume(u32),
    Seek,
}

// `data-on-input="seek"` sends `seek:<value>`: a unit `Seek` can't take it.
#[derive(XeruneTemplate)]
#[template(source = r#"
<div>
    <input type="range" data-on-input="volume">
    <input type="range" data-on-change="seek">
</div>
"#)]
struct Page;

impl Model for Page {
    type Message = Msg;
    fn update(&mut self, _msg: Msg, _context: &mut Context<Self::Message>) {}
}

fn main() {}
