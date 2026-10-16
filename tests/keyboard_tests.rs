//! The on-screen keyboard: a model asks for text with `Context::edit_text`,
//! the keyboard covers the screen until OK or Cancel, and the model gets the
//! result as its own message.

use xerune::*;

#[derive(Debug, Clone, XeruneMessage)]
enum Msg {
    EditName,
    EditPassword,
    #[xerune(prefix = "name:")]
    Name(String),
    Cancelled,
    #[xerune(prefix = "keydown:")]
    KeyDown(#[allow(dead_code)] String),
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
    .list { height: 50px; overflow: scroll; }
    .long { height: 500px; flex-shrink: 0; }
</style>
<div>
    <div class="name">Name: {{ name }}</div>
    <div class="list"><div class="long">{{ cancels }} cancels, {{ keys }} keys</div></div>
</div>"#)]
struct Settings {
    name: String,
    cancels: u32,
    keys: u32,
}

impl Model for Settings {
    type Message = Msg;
    fn update(&mut self, msg: Msg, context: &mut Context<Msg>) {
        match msg {
            Msg::EditName => context.edit_text(TextRequest::new("Name").text(&self.name), |name| match name {
                Some(name) => Msg::Name(name),
                None => Msg::Cancelled,
            }),
            Msg::EditPassword => context.edit_text(TextRequest::new("Password").password().min_len(3), |password| match password {
                Some(password) => Msg::Name(password),
                None => Msg::Cancelled,
            }),
            Msg::Name(name) => self.name = name,
            Msg::Cancelled => self.cancels += 1,
            Msg::KeyDown(_) => self.keys += 1,
        }
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, text: &str, font_size: f32, _weight: u16) -> (f32, f32) {
        (text.chars().count() as f32 * font_size * 0.5, font_size)
    }
}

fn start(name: &str) -> Runtime<Settings, MockMeasurer> {
    let mut runtime = Runtime::new(Settings { name: name.to_string(), cancels: 0, keys: 0 }, MockMeasurer);
    runtime.set_size(800.0, 480.0);
    runtime
}

fn send(runtime: &mut Runtime<Settings, MockMeasurer>, messages: &str) -> bool {
    runtime.handle_messages(messages.split(';').map(String::from))
}

fn texts(runtime: &Runtime<Settings, MockMeasurer>) -> Vec<String> {
    runtime.ui.render_data.values().filter_map(|d| match &d.kind {
        RenderKind::Text(t) => Some(t.clone()),
        _ => None,
    }).collect()
}

fn shows(runtime: &Runtime<Settings, MockMeasurer>, text: &str) -> bool {
    texts(runtime).iter().any(|t| t == text)
}

#[test]
fn types_and_confirms() {
    let mut runtime = start("pi");
    assert!(send(&mut runtime, "edit_name"));
    assert!(shows(&runtime, "OK"), "the keyboard shows: {:?}", texts(&runtime));
    assert!(!shows(&runtime, "Name: pi"));
    assert!(shows(&runtime, "pi"));

    send(&mut runtime, "kb:shift;kb:type:X;kb:left;kb:left;kb:backspace;kb:ok");
    assert_eq!(runtime.model().name, "iX");
    assert!(shows(&runtime, "Name: iX"), "the model's view is back: {:?}", texts(&runtime));
}

#[test]
fn cancel_and_keys_by_key_map() {
    let mut runtime = start("pi");
    send(&mut runtime, "edit_name");
    // Keys go to the keyboard, not the model.
    runtime.handle_event(InputEvent::TextInput { id: String::new(), text: "e\r".into() });
    runtime.handle_event(InputEvent::KeyDown("Backspace".into()));
    runtime.handle_event(InputEvent::KeyDown("KeyQ".into()));
    runtime.handle_event(InputEvent::KeyDown("Escape".into()));
    assert_eq!((runtime.model().name.as_str(), runtime.model().cancels, runtime.model().keys), ("pi", 1, 0));

    runtime.set_key_map(KeyMap::default().bind("Next", KeyAction::Activate));
    send(&mut runtime, "edit_name;kb:type:!");
    runtime.handle_event(InputEvent::KeyDown("Next".into()));
    assert_eq!(runtime.model().name, "pi!");

    // Closed: keys are the model's again.
    runtime.handle_event(InputEvent::KeyDown("Escape".into()));
    assert_eq!(runtime.model().keys, 1);
}

#[test]
fn password_is_masked_and_ok_waits_for_min_len() {
    let mut runtime = start("");
    send(&mut runtime, "edit_password;kb:type:a;kb:type:b");
    assert!(shows(&runtime, "\u{2022}\u{2022}"));
    assert!(shows(&runtime, "2 / 3+"));
    send(&mut runtime, "kb:ok");
    assert!(shows(&runtime, "OK"), "still open");
    send(&mut runtime, "kb:reveal");
    assert!(shows(&runtime, "ab"));
    send(&mut runtime, "kb:type:c;kb:ok");
    assert_eq!(runtime.model().name, "abc");
}

#[test]
fn clicking_keys_types() {
    let mut runtime = start("");
    send(&mut runtime, "edit_name");
    let (w, h) = runtime.viewport();
    // The bottom row's middle is the space bar; the row above starts with Shift.
    runtime.handle_event(InputEvent::Click { x: w / 2.0, y: h - 30.0 });
    send(&mut runtime, "kb:type:a;kb:ok");
    assert_eq!(runtime.model().name, " a");
}

#[test]
fn scroll_position_survives_the_keyboard() {
    let mut runtime = start("");
    assert!(runtime.handle_event(InputEvent::Scroll { x: 10.0, y: 40.0, delta_x: 0.0, delta_y: -120.0 }));
    let scrolled = runtime.ui.scroll_offsets.values().copied().collect::<Vec<_>>();
    assert_eq!(scrolled, [(0.0, 120.0)]);
    send(&mut runtime, "edit_name");
    assert!(runtime.ui.scroll_offsets.values().all(|&(_, y)| y == 0.0));
    send(&mut runtime, "kb:cancel");
    assert_eq!(runtime.ui.scroll_offsets.values().copied().collect::<Vec<_>>(), scrolled);
}

#[derive(Debug, Clone, XeruneMessage)]
enum FormMsg {
    #[xerune(prefix = "name:value:")]
    Name(String),
    #[xerune(prefix = "name:text:")]
    Typed(String),
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div>
    <input type="text" id="name" value="{{ name }}" placeholder="Your name">
    <input type="text" id="other" placeholder="Not handled">
</div>"#)]
struct Form {
    name: String,
}

impl Model for Form {
    type Message = FormMsg;
    fn update(&mut self, msg: FormMsg, _context: &mut Context<FormMsg>) {
        match msg {
            FormMsg::Name(name) => self.name = name,
            FormMsg::Typed(typed) => self.name.push_str(&typed),
        }
    }
}

fn form_texts(runtime: &Runtime<Form, MockMeasurer>) -> Vec<String> {
    runtime.ui.render_data.values().filter_map(|d| match &d.kind {
        RenderKind::Text(t) => Some(t.clone()),
        _ => None,
    }).collect()
}

#[test]
fn tapping_an_input_opens_the_keyboard_when_asked_to() {
    let mut runtime = Runtime::new(Form { name: "Ada".into() }, MockMeasurer);
    runtime.set_size(800.0, 480.0);
    // Off: a tap focuses the input, which takes typed text.
    runtime.handle_event(InputEvent::Click { x: 20.0, y: 15.0 });
    assert!(!form_texts(&runtime).iter().any(|t| t == "OK"));
    runtime.handle_event(InputEvent::TextInput { id: String::new(), text: "!".into() });
    assert_eq!(runtime.model().name, "Ada!");

    runtime.set_input_keyboard(true);
    runtime.handle_event(InputEvent::Click { x: 20.0, y: 15.0 });
    let texts = form_texts(&runtime);
    assert!(texts.iter().any(|t| t == "Your name") && texts.iter().any(|t| t == "Ada!"), "{texts:?}");
    runtime.handle_messages(["kb:backspace".to_string(), "kb:type:?".to_string(), "kb:ok".to_string()]);
    assert_eq!(runtime.model().name, "Ada?");

    // Cancel sends nothing; neither does OK for an input the model takes no value for.
    runtime.handle_event(InputEvent::Click { x: 20.0, y: 15.0 });
    runtime.handle_messages(["kb:type:x".to_string(), "kb:cancel".to_string()]);
    runtime.handle_event(InputEvent::Click { x: 190.0, y: 15.0 });
    assert!(form_texts(&runtime).iter().any(|t| t == "Not handled"), "the other input's keyboard");
    runtime.handle_messages(["kb:type:x".to_string(), "kb:ok".to_string()]);
    assert_eq!(runtime.model().name, "Ada?");
    assert!(!form_texts(&runtime).iter().any(|t| t == "OK"), "closed");
}
