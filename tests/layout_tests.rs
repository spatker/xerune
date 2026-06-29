use xerune::*;
use taffy::prelude::TaffyMaxContent;

struct MockModel;
#[derive(Debug, PartialEq)]
enum MockMsg {
    Tick,
}
impl std::str::FromStr for MockMsg {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tick" => Ok(MockMsg::Tick),
            _ => Err(()),
        }
    }
}

impl Model for MockModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

impl TemplateLayout for MockModel {
    fn stylesheet(&self) -> &'static str {
        ""
    }
    fn build_ui(&self, builder: &mut UiBuilder) -> taffy::NodeId {
        let parent = builder.create_element("div", &[("style", "height: 100px; overflow: scroll;")]);
        let child = builder.create_element("div", &[("style", "height: 200px; flex-shrink: 0;"), ("data-on-click", "test_interaction")]);
        let text = builder.create_text("Content", &[]);
        builder.append_child(child, text);
        builder.append_child(parent, child);
        parent
    }
}

struct MockMeasurer;
impl TextMeasurer for MockMeasurer {
    fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
        (10.0, 10.0)
    }
}

#[test]
fn test_scroll_persistence() {
    let model = MockModel;
    let measurer = MockMeasurer;
    let mut runtime = Runtime::new(model, measurer);
    
    runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);

    let handled = runtime.handle_event(InputEvent::Scroll { 
        x: 10.0, y: 10.0, 
        delta_x: 0.0, delta_y: -10.0 // Scroll down 10px
    });
    
    assert!(handled, "Scroll event should be handled");
    
    let offsets = &runtime.ui.scroll_offsets;
    let offset = offsets.values().next().expect("Should have scroll offset");
    assert_eq!(offset.1, 10.0, "Offset should be 10.0 after first scroll");
    
    runtime.handle_event(InputEvent::Message("tick".to_string()));
    
    let offsets_after = &runtime.ui.scroll_offsets;
    let offset_after = offsets_after.values().next().expect("Should have scroll offset after tick");
    assert_eq!(offset_after.1, 10.0, "Offset should persist after Tick/Ui Recreation");
    
    runtime.handle_event(InputEvent::Scroll { 
        x: 10.0, y: 10.0, 
        delta_x: 0.0, delta_y: -10.0 
    });
    
    let offsets_final = &runtime.ui.scroll_offsets;
    let offset_final = offsets_final.values().next().expect("Should have scroll offset");
    assert_eq!(offset_final.1, 20.0, "Offset should accumulate (10+10=20)");

    runtime.handle_event(InputEvent::Scroll { 
        x: 10.0, y: 10.0, 
        delta_x: 0.0, delta_y: -500.0 // Big scroll down
    });
    
    let offsets_clamped = &runtime.ui.scroll_offsets;
    let offset_clamped = offsets_clamped.values().next().expect("Should have scroll offset");
    assert_eq!(offset_clamped.1, 100.0, "Offset should be clamped to max scroll (100.0)");

    let hit = runtime.ui.hit_test(10.0, 10.0);
    assert!(hit.is_some(), "Should hit child content after scrolling");
    assert_eq!(hit.unwrap().0, "test_interaction".to_string());
}

struct SelectorMockModel;
impl Model for SelectorMockModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

impl TemplateLayout for SelectorMockModel {
    fn stylesheet(&self) -> &'static str {
        r#"
        div {
            color: #ff0000;
            background-color: #00ff00;
        }
        .blue-text {
            color: #0000ff;
        }
        #my-id {
            font-size: 20px;
        }
        "#
    }
    fn build_ui(&self, builder: &mut UiBuilder) -> taffy::NodeId {
        let parent = builder.create_element("div", &[]);
        
        let child1 = builder.create_element("div", &[("class", "blue-text"), ("id", "my-id")]);
        let text1 = builder.create_text("Styled Element", &[]);
        builder.append_child(child1, text1);
        builder.append_child(parent, child1);

        let child2 = builder.create_element("div", &[("class", "blue-text"), ("style", "color: #ffffff;")]);
        let text2 = builder.create_text("Inline Override", &[]);
        builder.append_child(child2, text2);
        builder.append_child(parent, child2);

        parent
    }
}

#[test]
fn test_style_selector_matching() {
    let model = SelectorMockModel;
    let measurer = MockMeasurer;
    let mut runtime = Runtime::new(model, measurer);
    
    runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);
    
    for (node_id, data) in &runtime.ui.render_data {
        match data {
            RenderData::Container(style) => {
                println!("Node {:?}: Container style: bg_color={:?}, color={:?}", node_id, style.background_color, style.color);
            }
            RenderData::Text(text, style) => {
                println!("Node {:?}: Text '{}' style: bg_color={:?}, color={:?}, size={}", node_id, text, style.background_color, style.color, style.font_size);
            }
            _ => {}
        }
    }
    
    let mut found_styled_element = false;
    let mut found_inline_override = false;
    let mut found_green_container = false;
    
    for data in runtime.ui.render_data.values() {
        match data {
            RenderData::Text(text, style) => {
                if text == "Styled Element" {
                    found_styled_element = true;
                    assert_eq!(style.color, Color::from_rgba8(0, 0, 255, 255));
                    assert_eq!(style.font_size, 20.0);
                } else if text == "Inline Override" {
                    found_inline_override = true;
                    assert_eq!(style.color, Color::from_rgba8(255, 255, 255, 255));
                }
            }
            RenderData::Container(style) => {
                if style.background_color == Some(Color::from_rgba8(0, 255, 0, 255)) {
                    found_green_container = true;
                }
            }
            _ => {}
        }
    }
    
    assert!(found_styled_element, "Should have parsed and found 'Styled Element' text");
    assert!(found_inline_override, "Should have parsed and found 'Inline Override' text");
    assert!(found_green_container, "Should have found container with green background color");
}

#[derive(XeruneTemplate)]
#[template(path = "test_template.html")]
struct TestMacroModel {
    value: String,
    items: Vec<String>,
}

impl Model for TestMacroModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

#[test]
fn test_macro_layout_generation() {
    let model = TestMacroModel {
        value: "Hello Macro".to_string(),
        items: vec!["A".to_string(), "B".to_string()],
    };
    let measurer = MockMeasurer;
    let mut runtime = Runtime::new(model, measurer);
    
    runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);

    let mut found_hello_macro = false;
    let mut found_a = false;
    let mut found_b = false;
    
    for data in runtime.ui.render_data.values() {
        match data {
            RenderData::Text(text, style) => {
                if text == "Hello Macro" {
                    found_hello_macro = true;
                    assert_eq!(style.color, Color::from_rgba8(0, 0, 255, 255));
                } else if text == "A" {
                    found_a = true;
                    assert_eq!(style.color, Color::from_rgba8(255, 0, 0, 255));
                } else if text == "B" {
                    found_b = true;
                    assert_eq!(style.color, Color::from_rgba8(255, 0, 0, 255));
                }
            }
            _ => {}
        }
    }
    
    assert!(found_hello_macro, "Should find 'Hello Macro' with class style applied");
    assert!(found_a, "Should find item 'A' within compiled loop");
    assert!(found_b, "Should find item 'B' within compiled loop");
}

#[derive(Clone)]
struct TodoItem {
    title: String,
    completed: bool,
}

#[derive(XeruneTemplate)]
#[template(path = "todo_list.html")]
struct TestTodoModel {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

impl Model for TestTodoModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

#[test]
fn test_todo_layout_comparison() {
    let model = TestTodoModel {
        items: vec![
            TodoItem { title: "Item 1".to_string(), completed: false },
            TodoItem { title: "Item 2".to_string(), completed: true },
        ],
        active_item: 0,
        new_item_title: "abc".to_string(),
    };
    let measurer = MockMeasurer;
    // Since resolve_styles is crate-internal under super::resolve_styles, we can resolve it using Runtime's internal compilations, 
    // or we can test using Runtime::new. Let's do it using Runtime::new which compiles and builds the ui structure.
    let mut runtime = Runtime::new(model, measurer);
    runtime.compute_layout(taffy::geometry::Size {
        width: taffy::prelude::AvailableSpace::Definite(800.0),
        height: taffy::prelude::AvailableSpace::Definite(600.0),
    });

    println!("--- COMPILED LAYOUT TREE ---");
    // Retrieve metadata/render_data/interactions from runtime's internal builders or Ui.
    // Wait, in the original test it was checking ui.taffy, ui.root, builder.node_metadata, etc.
    // Since builder is no longer directly available outside the crate (since resolving styles is part of new_compiled),
    // let's print layout tree by traversing taffy from root.
    // Wait! Let's check how print_layout_tree prints:
    // It reads metadata from `builder.node_metadata` which was populated when building.
    // In our test suite, we can just print the layouts using a simplified printer, or we can check the layout output.
    // Let's implement printing by traversing taffy children, which is very simple.
    fn print_taffy_tree(taffy: &taffy::TaffyTree, node: taffy::NodeId, indent: usize) {
        let prefix = "  ".repeat(indent);
        let layout = taffy.layout(node).unwrap();
        println!("{}Node {:?} layout={:?}", prefix, node, layout);
        if let Ok(children) = taffy.children(node) {
            for child in children {
                print_taffy_tree(taffy, child, indent + 1);
            }
        }
    }
    print_taffy_tree(&runtime.ui.taffy, runtime.ui.root, 0);
    println!("----------------------------");
}
