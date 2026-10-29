use xerune::*;
use taffy::prelude::TaffyMaxContent;

/// The viewport (`vw`, `vh`, `@media`) is process-global and tests run in
/// parallel: `set_size` stores it and then rebuilds reading it back, so
/// concurrent calls must not interleave.
fn set_size<M: Model + ui::TemplateLayout, R: TextMeasurer>(runtime: &mut Runtime<M, R>, width: f32, height: f32) {
    static VIEWPORT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = VIEWPORT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    runtime.set_size(width, height);
}

/// The bundled Roboto, regular and bold, as the examples use it.
fn roboto() -> xerune_render::Fonts {
    use std::sync::OnceLock;
    static FONTS: OnceLock<xerune_render::Fonts> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            xerune_render::Fonts::ttf(
                include_bytes!("../resources/fonts/Roboto-Regular.ttf"),
                Some(include_bytes!("../resources/fonts/Roboto-Bold.ttf")),
            )
            .unwrap()
            // As the examples: Hangul from a fallback font.
            .with_fallback(
                include_bytes!("../resources/fonts/XeruneHangul-Regular.otf"),
                Some(include_bytes!("../resources/fonts/XeruneHangul-Bold.otf")),
            )
            .unwrap()
        })
        .clone()
}

struct MockModel;
#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum MockMsg {
    Tick,
}

impl Model for MockModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

impl TemplateLayout for MockModel {
    fn stylesheet(&self) -> &'static str {
        ""
    }
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _measurer: &impl TextMeasurer,
        _default_style: &ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
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
    assert!(matches!(hit.unwrap().0, Some(ui::Interaction::Text(text)) if text == "test_interaction"));
}

struct SelectorMockModel;
impl Model for SelectorMockModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
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
    fn build_ui(
        &self,
        builder: &mut UiBuilder,
        _measurer: &impl TextMeasurer,
        _default_style: &ContainerStyle,
        _message_validator: &impl Fn(&str) -> bool,
    ) -> taffy::NodeId {
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
        match (&data.kind, &data.style) {
            (RenderKind::Container, style) => {
                println!("Node {:?}: Container style: bg_color={:?}, color={:?}", node_id, style.background_color, style.color);
            }
            (RenderKind::Text(text), style) => {
                println!("Node {:?}: Text '{}' style: bg_color={:?}, color={:?}, size={}", node_id, text, style.background_color, style.color, style.font_size);
            }
            _ => {}
        }
    }
    
    let mut found_styled_element = false;
    let mut found_inline_override = false;
    let mut found_green_container = false;
    
    for data in runtime.ui.render_data.values() {
        match (&data.kind, &data.style) {
            (RenderKind::Text(text), style) => {
                if text == "Styled Element" {
                    found_styled_element = true;
                    assert_eq!(style.color, Color::from_rgba8(0, 0, 255, 255));
                    assert_eq!(style.font_size, 20.0);
                } else if text == "Inline Override" {
                    found_inline_override = true;
                    assert_eq!(style.color, Color::from_rgba8(255, 255, 255, 255));
                }
            }
            (RenderKind::Container, style) => {
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
#[template(path = "tests/test_template.html")]
struct TestMacroModel {
    value: String,
    items: Vec<String>,
}

impl Model for TestMacroModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
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
        match (&data.kind, &data.style) {
            (RenderKind::Text(text), style) => {
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
    tag: String,
    completed: bool,
}

impl TodoItem {
    fn new(title: &str, completed: bool) -> Self {
        Self { title: title.to_string(), tag: "work".to_string(), completed }
    }
}

#[derive(Debug, PartialEq, Clone, XeruneMessage)]
enum TestTodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
    #[xerune(prefix = "todo_input:text:")]
    TodoInput(String),
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
    #[xerune(prefix = "lang:")]
    Lang(String),
}

#[derive(XeruneTemplate)]
#[template(path = "examples/todo/templates/todo_list.html")]
struct TestTodoModel {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
    lang: String,
}

/// Shows the template's keys in place of translations.
impl TestTodoModel {
    fn t<'a>(&self, key: &'a str) -> &'a str {
        key
    }

    fn langs(&self) -> Vec<&'static str> {
        vec!["en"]
    }

    fn lang_is(&self, lang: &str) -> bool {
        lang == self.lang
    }

    fn lang_name<'a>(&self, lang: &'a str) -> &'a str {
        lang
    }

    fn tag_name(&self, tag: &str) -> String {
        tag.to_string()
    }

    fn done_count(&self) -> usize {
        self.items.iter().filter(|i| i.completed).count()
    }

    fn open_count(&self) -> usize {
        self.items.len() - self.done_count()
    }

    fn done_pct(&self) -> usize {
        self.done_count() * 100 / self.items.len().max(1)
    }
}

impl Model for TestTodoModel {
    type Message = TestTodoMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

#[test]
fn test_todo_layout_comparison() {
    let model = TestTodoModel {
        items: vec![
            TodoItem::new("Item 1", false),
            TodoItem::new("Item 2", true),
        ],
        active_item: 0,
        new_item_title: "abc".to_string(),
        lang: "en".to_string(),
    };
    let measurer = MockMeasurer;
    // Since resolve_styles is crate-internal under super::resolve_styles, we can resolve it using Runtime's internal compilations, 
    // or we can test using Runtime::new. Let's do it using Runtime::new which compiles and builds the ui structure.
    let mut runtime = Runtime::new(model, measurer);
    crate::set_size(&mut runtime, 800.0, 600.0);

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

#[test]
fn test_tiled_rendering_identity() {
    use std::collections::HashMap;
    use xerune_render::SoftwareRenderer;

    let fonts = roboto();

    // Setup model and layout
    let model = TestTodoModel {
        items: vec![
            TodoItem::new("Item 1", false),
            TodoItem::new("Item 2", true),
        ],
        active_item: 0,
        new_item_title: "abc".to_string(),
        lang: "en".to_string(),
    };
    let measurer = fonts.clone();
    let mut runtime = Runtime::new(model, measurer);
    crate::set_size(&mut runtime, 800.0, 600.0);

    // Get render commands
    let commands = runtime.ui.build_commands(&HashMap::new(), None);

    // 1. Full-screen rendering
    let (canvases, images) = (HashMap::new(), xerune::ImageStore::new());
    let resources = xerune::RenderResources::new(&canvases, &images);
    let mut full_buffer = vec![0u32; 800 * 600];
    {
        let mut target = Surface::new(Pixels::Xrgb8888(&mut full_buffer), 800, 600);
        let area = target.area();
        SoftwareRenderer::new(fonts.clone()).render(&mut target, &commands, &resources, area);
    }

    // 2. Rendering in 50-row bands, each copied to the screen
    let mut tiled_buffer = vec![0u32; 800 * 600];
    let mut renderer = SoftwareRenderer::new(fonts.clone());
    let mut band = vec![0u32; 800 * 50];
    for y in (0..600).step_by(50) {
        band.fill(0);
        let mut target = Surface::new(Pixels::Xrgb8888(&mut band), 800, 50).with_origin(0, y as i32);
        let area = target.area();
        renderer.render(&mut target, &commands, &resources, area);
        tiled_buffer[y * 800..(y + 50) * 800].copy_from_slice(&band);
    }

    // 3. Compare pixel buffers
    assert_eq!(full_buffer, tiled_buffer, "Full screen rendering and tiled rendering outputs must be pixel-perfect identical");
}

#[test]
fn test_bitmap_font_rendering() {
    use std::collections::HashMap;
    use xerune_render::SoftwareRenderer;

    let fonts = xerune_render::Fonts::bitmap(&xerune::font::DEFAULT_ROBOTO);

    let commands = vec![
        DrawCommand::DrawText {
            text: "Hello World!".to_string(),
            rect: Rect { x: 10.0, y: 10.0, width: 200.0, height: 30.0 },
            color: Color::new(255, 255, 255, 255),
            font_size: 16.0,
            weight: 0,
            run: None,
        }
    ];

    let mut buffer = vec![0u32; 200 * 50];
    let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), 200, 50);
    let area = target.area();
    SoftwareRenderer::new(fonts.clone()).render(&mut target, &commands, &xerune::RenderResources::new(&HashMap::new(), &xerune::ImageStore::new()), area);

    // Verify that some pixels are drawn (not all zeros)
    let non_zero_count = buffer.iter().filter(|&&pixel| pixel != 0).count();
    assert!(non_zero_count > 0, "Bitmap font should have rasterized some non-zero pixels onto the buffer");
}

#[test]
fn test_touch_scrolling_and_clicking() {
    struct TouchMockModel;
    #[derive(Debug, PartialEq, Clone, XeruneMessage)]
    enum TouchMockMsg {
        #[xerune(rename = "click_action")]
        ClickMsg,
    }
    impl Model for TouchMockModel {
        type Message = TouchMockMsg;
        fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
    }
    impl TemplateLayout for TouchMockModel {
        fn stylesheet(&self) -> &'static str { "" }
        fn build_ui(
            &self,
            builder: &mut UiBuilder,
            _measurer: &impl TextMeasurer,
            _default_style: &ContainerStyle,
            _message_validator: &impl Fn(&str) -> bool,
        ) -> taffy::NodeId {
            let parent = builder.create_element("div", &[("style", "width: 100px; height: 100px; overflow: scroll;")]);
            let child = builder.create_element("div", &[("style", "width: 100px; height: 200px; flex-shrink: 0;"), ("data-on-click", "click_action")]);
            builder.append_child(parent, child);
            parent
        }
    }

    let model = TouchMockModel;
    let measurer = MockMeasurer;
    let mut runtime = Runtime::new(model, measurer);
    
    runtime.compute_layout(taffy::geometry::Size::MAX_CONTENT);

    // 1. Test touch click (finger tap with little/no movement)
    let handled = runtime.handle_event(InputEvent::TouchStart { id: 1, x: 10.0, y: 10.0 });
    assert!(!handled, "TouchStart should not trigger redraw on its own");
    
    let handled = runtime.handle_event(InputEvent::TouchEnd { id: 1, x: 11.0, y: 11.0 });
    assert!(handled, "TouchEnd within threshold should trigger a click redraw");

    // 2. Test touch scroll (finger drag with enough movement)
    let handled = runtime.handle_event(InputEvent::TouchStart { id: 2, x: 10.0, y: 10.0 });
    assert!(!handled);
    
    // Drag finger UP (y decreases: e.g. to 0.0) -> scrolls DOWN (content offset sy increases)
    let handled = runtime.handle_event(InputEvent::TouchMove { id: 2, x: 10.0, y: 0.0 });
    assert!(handled, "TouchMove past threshold should handle scroll and trigger redraw");
    
    let offsets = &runtime.ui.scroll_offsets;
    let offset = offsets.values().next().expect("Should have scroll offset");
    assert_eq!(offset.1, 10.0, "Scroll offset y should be 10.0");

    // End touch scroll
    let handled = runtime.handle_event(InputEvent::TouchEnd { id: 2, x: 10.0, y: 0.0 });
    assert!(!handled, "TouchEnd after scrolling should not trigger click or redraw");
}

#[test]
fn test_box_shadow_and_border_parsing() {
    use xerune::css::parse_box_shadow;
    use xerune::style::{ContainerStyle, BorderStyle};
    use taffy::style::Style;

    // Test box-shadow parsing
    let shadow = parse_box_shadow("0px 4px 10px 2px rgba(0, 0, 0, 0.5)").expect("Should parse box shadow");
    assert_eq!(shadow.offset_x, 0.0);
    assert_eq!(shadow.offset_y, 4.0);
    assert_eq!(shadow.blur_radius, 10.0);
    assert_eq!(shadow.spread_radius, 2.0);
    assert_eq!(shadow.color, Color::from_rgba8(0, 0, 0, 127));
    assert!(!shadow.inset);

    let inset_shadow = parse_box_shadow("inset 2px 2px 5px #ff0000").expect("Should parse inset shadow");
    assert!(inset_shadow.inset);
    assert_eq!(inset_shadow.color, Color::from_rgba8(255, 0, 0, 255));

    // Test inline style parsing for border-style and box-shadow
    let mut style = ContainerStyle::default();
    let mut taffy_style = Style::default();
    css::parse_inline_style("border: 2px dashed #00ff00; box-shadow: 0px 8px 16px rgba(0,0,0,0.4);", &mut style, &mut taffy_style);
    
    let green = Color::from_rgba8(0, 255, 0, 255);
    assert_eq!(style.border, xerune::style::Borders::all(xerune::style::BorderSide { width: 2.0, color: Some(green), style: BorderStyle::Dashed }));
    assert!(style.box_shadow.is_some());
}

#[test]
fn borders_are_per_side() {
    use xerune::style::{BorderSide, BorderStyle, Borders, ContainerStyle};
    let border = |css: &str| {
        let mut style = ContainerStyle::default();
        css::parse_inline_style(css, &mut style, &mut taffy::style::Style::default());
        style.border
    };
    let red = Color::from_rgba8(255, 0, 0, 255);
    let b = border("border-left: 5px solid red");
    assert_eq!(b.left, BorderSide::solid(5.0, red));
    assert_eq!(b.top, BorderSide::INITIAL, "other sides stay unset");
    assert_eq!(b.widths().top, 0.0);
    // Later declarations win per side.
    let b = border("border: 1px solid red; border-bottom-width: 3px; border-right-color: rgba(0, 0, 255, 0.5)");
    assert_eq!((b.top.width, b.bottom.width), (1.0, 3.0));
    assert_eq!(b.right.color, Some(Color::from_rgba8(0, 0, 255, 127)));
    // Box shorthands: top, right/left, bottom.
    let b = border("border-style: solid; border-width: 1px thick 2px");
    assert_eq!([b.top.width, b.right.width, b.bottom.width, b.left.width], [1.0, 5.0, 2.0, 5.0]);
    // Without a style there is no border; without a color it is the text color.
    assert!(!border("border: 4px red").is_visible());
    let b = border("border: solid").resolved(red);
    assert_eq!(b.top, BorderSide { width: 3.0, color: Some(red), style: BorderStyle::Solid });
    // A value that can't be parsed invalidates the whole declaration.
    assert_eq!(border("border: 1px solid var(--line)"), Borders::NONE);
}

#[test]
fn test_dirty_rect_rasterization_culling() {
    use std::collections::HashMap;
    use xerune_render::SoftwareRenderer;

    let fonts = xerune_render::Fonts::bitmap(&xerune::font::DEFAULT_ROBOTO);

    let commands = vec![
        DrawCommand::DrawRect {
            rect: Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 },
            color: Some(Color::new(255, 0, 0, 255)),
            gradient: None,
            border_radius: 0.0,
            border: xerune::style::Borders::NONE,
        },
        DrawCommand::DrawRect {
            rect: Rect { x: 100.0, y: 100.0, width: 50.0, height: 50.0 },
            color: Some(Color::new(0, 255, 0, 255)),
            gradient: None,
            border_radius: 0.0,
            border: xerune::style::Borders::NONE,
        },
    ];

    let mut buffer = vec![0u32; 200 * 200];

    // Render with the area restricted to the second rectangle only
    let mut target = Surface::new(Pixels::Xrgb8888(&mut buffer), 200, 200);
    SoftwareRenderer::new(fonts.clone()).render(&mut target, &commands, &xerune::RenderResources::new(&HashMap::new(), &xerune::ImageStore::new()), Rect::new(90.0, 90.0, 70.0, 70.0));

    // Verify first rect (0,0..50,50) was culled and untouched (0)
    let first_rect_pixel = buffer[0];
    assert_eq!(first_rect_pixel, 0, "First rect should not have been rendered because it falls outside dirty_rect");

    // Verify second rect (100,100) was rendered with green color (0xFF00FF00)
    let second_rect_pixel = buffer[100 * 200 + 100];
    assert_eq!(second_rect_pixel, 0xFF00FF00, "Second rect should have been rendered within dirty_rect");
}

#[test]
fn test_zero_copy_canvas_blitting() {
    use xerune_render::blit_image_rgba;

    let mut buffer = vec![0u32; 100 * 100];
    // 2x2 RGBA image
    let canvas_rgba = vec![
        255, 0, 0, 255,   // Red
        0, 255, 0, 255,   // Green
        0, 0, 255, 255,   // Blue
        255, 255, 255, 255 // White
    ];

    blit_image_rgba(
        &mut buffer,
        100,
        100,
        100,
        &Rect::new(10.0, 10.0, 2.0, 2.0),
        0.0,
        2,
        2,
        &canvas_rgba,
        1.0,
        false,
        None,
        0,
    );

    assert_eq!(buffer[10 * 100 + 10], 0xFFFF0000); // Red (ARGB)
    assert_eq!(buffer[10 * 100 + 11], 0xFF00FF00); // Green (ARGB)
    assert_eq!(buffer[11 * 100 + 10], 0xFF0000FF); // Blue (ARGB)
    assert_eq!(buffer[11 * 100 + 11], 0xFFFFFFFF); // White (ARGB)
}




#[derive(XeruneTemplate)]
#[template(source = "<html><body><p>short</p></body></html>")]
struct HtmlRootModel;

impl Model for HtmlRootModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

#[test]
fn test_html_root_fills_viewport_width() {
    // <html> is block-level: it, and the blocks inside it, take the full
    // viewport width instead of shrink-wrapping their content (less body's
    // 8px margins, as in a browser).
    let mut runtime = Runtime::new(HtmlRootModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let html = runtime.ui.taffy.children(runtime.ui.root).unwrap()[0];
    let body = runtime.ui.taffy.children(html).unwrap()[0];
    assert_eq!(runtime.ui.taffy.layout(html).unwrap().size.width, 800.0);
    assert_eq!(runtime.ui.taffy.layout(body).unwrap().size.width, 784.0);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>body { margin: 0 }</style><div style="width: 50px; height: 20px" data-on-click="buy:{{ id }}"></div>"#)]
struct StringMessageModel {
    id: i64,
    got: Vec<String>,
}

impl Model for StringMessageModel {
    type Message = String;
    fn update(&mut self, msg: String, _context: &mut Context<String>) {
        self.got.push(msg);
    }
}

#[test]
fn string_messages_take_template_arguments() {
    // `prefix:{{ expr }}` is a typed constructor for a message enum; for
    // `String` messages it is the text.
    let mut runtime = Runtime::new(StringMessageModel { id: 7, got: Vec::new() }, MockMeasurer);
    crate::set_size(&mut runtime, 100.0, 100.0);
    runtime.handle_event(InputEvent::TouchStart { id: 1, x: 10.0, y: 10.0 });
    runtime.handle_event(InputEvent::TouchEnd { id: 1, x: 10.0, y: 10.0 });
    assert_eq!(runtime.model().got, ["buy:7"]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<html><head><style>
  html { font-size: 10px }
  body { margin: 0 }
  .a { width: 2em; font-size: 20px; height: 1rem }
  .b { font-size: 2em }
  .c { width: 3em; height: 1px }
</style></head><body><div class="a"></div><div class="b"><div class="c"></div></div></body></html>"#)]
struct FontRelativeModel;

impl Model for FontRelativeModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

#[test]
fn em_is_relative_to_the_font_size_and_rem_to_the_root() {
    let mut runtime = Runtime::new(FontRelativeModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let ui = &runtime.ui;
    let html = ui.taffy.children(ui.root).unwrap()[0];
    let body = ui.taffy.children(html).unwrap()[0];
    let [a, b] = ui.taffy.children(body).unwrap()[..] else { panic!() };
    let c = ui.taffy.children(b).unwrap()[0];
    let size = |n| ui.taffy.layout(n).unwrap().size;
    // `font-size` applies before `width: 2em`, though it comes after it.
    assert_eq!(size(a).width, 40.0);
    assert_eq!(size(a).height, 10.0, "1rem is html's 10px");
    // `font-size: 2em` doubles body's 10px; `.c` inherits 20px.
    assert_eq!(size(c).width, 60.0);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div><span>text</span><div style="display: inline-flex"><div style="width: 20px; height: 5px"></div><div style="width: 30px; height: 5px"></div></div></div>"#)]
struct InlineFlexModel;

impl Model for InlineFlexModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

#[test]
fn inline_flex_sits_in_the_line_and_lays_out_a_row() {
    let mut runtime = Runtime::new(InlineFlexModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let ui = &runtime.ui;
    let first = |n| ui.taffy.children(n).unwrap()[0];
    let block = first(first(first(ui.root))); // html > body > div
    let [span, flex] = ui.taffy.children(block).unwrap()[..] else { panic!() };
    let (span, flex_box) = (ui.taffy.layout(span).unwrap(), ui.taffy.layout(flex).unwrap());
    assert_eq!(flex_box.location.y, span.location.y, "same line as the text");
    assert!(flex_box.location.x >= span.location.x + span.size.width);
    assert_eq!(flex_box.size.width, 50.0, "shrink-wraps its row of items");
    let second = ui.taffy.children(flex).unwrap()[1];
    assert_eq!(ui.taffy.layout(second).unwrap().location.x, 20.0);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div style="position: relative; width: 100px; height: 100px">
  <div style="position: absolute; z-index: 2; width: 50px; height: 50px; background: #ff0000" data-on-click="tick"></div>
  <div style="position: absolute; width: 50px; height: 50px; background: #00ff00"></div>
  <div style="position: absolute; z-index: -1; width: 50px; height: 50px; background: #0000ff"></div>
</div>"#)]
struct ZIndexModel;

impl Model for ZIndexModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

#[test]
fn z_index_orders_painting_and_hit_testing() {
    let mut runtime = Runtime::new(ZIndexModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let colors: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 50.0 => Some((c.r, c.g, c.b)),
        _ => None,
    }).collect();
    assert_eq!(colors, [(0, 0, 255), (0, 255, 0), (255, 0, 0)], "z-index -1, auto, 2");
    let (interaction, _) = runtime.ui.hit_test(20.0, 20.0).expect("hit");
    let message = match &interaction {
        Some(ui::Interaction::Message(message)) => message.downcast_ref::<MockMsg>(),
        _ => None,
    };
    assert_eq!(message, Some(&MockMsg::Tick), "the topmost box gets the click: {interaction:?}");
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>body { margin: 0; }</style>
<div style="height: 0">
  <div style="position: absolute; left: 10px; top: 10px; width: 50px; height: 50px; background: #ff0000" data-on-click="tick"></div>
</div>
<div style="position: absolute; left: 100px; top: 0; width: 20px; height: 20px; overflow: hidden">
  <div style="position: absolute; left: 0; top: 30px; width: 50px; height: 50px" data-on-click="tick"></div>
</div>"#)]
struct OverflowHitModel;

impl Model for OverflowHitModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// Elements outside their parent's box are clicked as they are painted:
/// unless the parent clips them.
#[test]
fn overflowing_elements_are_hit_unless_clipped() {
    let mut runtime = Runtime::new(OverflowHitModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let clickable = |x, y| runtime.ui.hit_test(x, y).is_some_and(|(interaction, _)| interaction.is_some());
    assert!(clickable(30.0, 30.0), "outside a parent of no height");
    assert!(!clickable(110.0, 40.0), "clipped by overflow: hidden");
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div style="display: flex">
  <div style="order: 2; width: 10px; height: 10px; background: #ff0000"></div>
  <div style="order: 3; width: 10px; height: 10px; background: #00ff00"></div>
  <div style="order: 1; width: 10px; height: 10px; background: #0000ff"></div>
</div>"#)]
struct FlexOrderModel;

impl Model for FlexOrderModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// Compiled templates place flex items in `order` too.
#[test]
fn templates_place_flex_items_in_order() {
    let mut runtime = Runtime::new(FlexOrderModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let mut boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some((rect.x, (c.r, c.g, c.b))),
        _ => None,
    }).collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let colors: Vec<_> = boxes.iter().map(|b| b.1).collect();
    assert_eq!(colors, [(0, 0, 255), (255, 0, 0), (0, 255, 0)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  .float { float: left; width: 100px; }
  .float > div { width: 10px; height: 10px; }
  .a { background: #ff0000; }
  .b { background: #00ff00; }
</style>
<div class="float"><div class="a"></div><div class="b"></div></div>"#)]
struct FloatChildrenModel;

impl Model for FloatChildrenModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// `float` is not inherited: a float's block children stack as blocks
/// rather than lining up as floats themselves.
#[test]
fn float_children_are_not_floats() {
    let mut runtime = Runtime::new(FloatChildrenModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(_), rect, .. } if rect.width == 10.0 => Some((rect.x, rect.y)),
        _ => None,
    }).collect();
    assert_eq!(boxes, [(8.0, 8.0), (8.0, 18.0)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  .row { display: flex; }
  .row > div { width: 10px; height: 10px; background: rgb(0, 0, 0); }
  .row > div:nth-child(2n) { background: rgb(0, 255, 0); }
  .row > :last-child:not(.a) { background: #0000ff; }
  .loop > div:first-child, .loop > div:nth-child(2) { background: #ff0000; }
</style>
<div class="row"><div class="a"></div><div class="b"></div><div class="c"></div></div>
<div class="row loop">{% for i in 0..2 %}<div></div>{% endfor %}</div>"#)]
struct SelectorModel;

impl Model for SelectorModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// Templates match structural selectors where the structure is in the
/// template, and not inside a loop, whose iterations are only known at
/// runtime.
#[test]
fn templates_match_structural_selectors_outside_loops() {
    let mut runtime = Runtime::new(SelectorModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let mut boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some(((rect.y, rect.x), (c.r, c.g, c.b))),
        _ => None,
    }).collect();
    boxes.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let colors: Vec<_> = boxes.iter().map(|b| b.1).collect();
    assert_eq!(colors, [(0, 0, 0), (0, 255, 0), (0, 0, 255), (0, 0, 0), (0, 0, 0)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  .p .q { width: 10px; height: 10px; background: #ff0000; }
  @media (min-width: 1px) { .q { background: #0000ff; } .p .r { background: #00ff00; } }
  .r { width: 10px; height: 10px; background: #ff0000; }
</style>
<div class="p"><div class="q"></div><div class="r"></div></div>"#)]
struct MediaCascadeModel;

impl Model for MediaCascadeModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// A rule in a matching `@media` block takes part in the cascade like any
/// other: it loses to a more specific rule outside the block and beats a
/// less specific one after it.
#[test]
fn media_rules_cascade_by_specificity() {
    let mut runtime = Runtime::new(MediaCascadeModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let mut boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some((rect.y, (c.r, c.g, c.b))),
        _ => None,
    }).collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let colors: Vec<_> = boxes.iter().map(|b| b.1).collect();
    assert_eq!(colors, [(255, 0, 0), (0, 255, 0)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  div { width: 10px; height: 10px; }
  #a { background: #ff0000 !important; }
  .a { background: #0000ff; }
  .b { background: #ff0000 !important; }
</style>
<div id="a" class="a" style="background: #00ff00"></div>
<div class="b" style="background: #0000ff !important"></div>"#)]
struct ImportantModel;

impl Model for ImportantModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// `!important` declarations beat the `style` attribute, unless it is
/// `!important` too.
#[test]
fn important_declarations_come_last() {
    let mut runtime = Runtime::new(ImportantModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let mut boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
        DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some((rect.y, (c.r, c.g, c.b))),
        _ => None,
    }).collect();
    boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
    let colors: Vec<_> = boxes.iter().map(|b| b.1).collect();
    assert_eq!(colors, [(255, 0, 0), (0, 0, 255)]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div>{% for i in 0..2 %}{% for j in 0..2 %}<p>{{ i }}{{ j }}</p>{% endfor %}{% endfor %}<p>x</p></div>"#)]
struct KeysModel;

impl Model for KeysModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// Every element has a key, distinct within the view (also in nested loops
/// with no element between them) and the same in every rebuild.
#[test]
fn element_keys_are_unique_and_stable() {
    let keys = || {
        let ui = ui::Ui::new_compiled(&KeysModel, &MockMeasurer, ContainerStyle::default(), &|_| true).unwrap();
        let mut keys: Vec<(u64, u64)> = ui.keys.iter().map(|(node, &key)| (u64::from(node), key)).collect();
        keys.sort();
        keys.into_iter().map(|(_, key)| key).collect::<Vec<_>>()
    };
    let first = keys();
    assert_eq!(first.len(), 8, "html, body, the div and five paragraphs");
    let mut unique = first.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), first.len());
    assert_eq!(keys(), first);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<style>
  body { margin: 0; }
  .row { display: flex; width: 100px; height: 10px; }
  .row > div { width: 10px; height: 10px; background: #000000; }
  .row > .a:hover { background: #ff0000; }
  .row:hover > .b { background: #00ff00; }
  .row > .c:active { background: #0000ff; }
</style>
<div class="row"><div class="a"></div><div class="b"></div><div class="c"></div></div>"#)]
struct PointerModel;

impl Model for PointerModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// `:hover` follows the pointer (the element under it and its ancestors),
/// `:active` the pressed element while a touch lasts.
#[test]
fn hover_and_active_follow_the_pointer() {
    let mut runtime = Runtime::new(PointerModel, MockMeasurer);
    crate::set_size(&mut runtime, 800.0, 600.0);
    let colors = |runtime: &mut Runtime<PointerModel, MockMeasurer>| {
        let mut boxes: Vec<_> = runtime.ui.build_commands(&std::collections::HashMap::new(), None).iter().filter_map(|c| match c {
            DrawCommand::DrawRect { color: Some(c), rect, .. } if rect.width == 10.0 => Some((rect.x, (c.r, c.g, c.b))),
            _ => None,
        }).collect();
        boxes.sort_by(|a, b| a.0.total_cmp(&b.0));
        boxes.into_iter().map(|b| b.1).collect::<Vec<_>>()
    };
    const BLACK: (u8, u8, u8) = (0, 0, 0);
    assert_eq!(colors(&mut runtime), [BLACK; 3]);
    assert!(runtime.handle_event(InputEvent::Hover { x: 5.0, y: 5.0 }));
    assert_eq!(colors(&mut runtime), [(255, 0, 0), (0, 255, 0), BLACK]);
    assert!(!runtime.handle_event(InputEvent::Hover { x: 6.0, y: 5.0 }), "same elements, no restyle");
    assert!(runtime.handle_event(InputEvent::Hover { x: 500.0, y: 500.0 }));
    assert_eq!(colors(&mut runtime), [BLACK; 3]);
    assert!(runtime.handle_event(InputEvent::TouchStart { id: 1, x: 25.0, y: 5.0 }));
    assert_eq!(colors(&mut runtime), [BLACK, (0, 255, 0), (0, 0, 255)]);
    runtime.handle_event(InputEvent::TouchEnd { id: 1, x: 25.0, y: 5.0 });
    assert_eq!(colors(&mut runtime), [BLACK, (0, 255, 0), BLACK], "hover stays after a tap");
}

#[derive(Debug, Clone, PartialEq, XeruneMessage)]
enum HoverMsg {
    Enter(usize),
    Leave,
}

#[derive(XeruneTemplate)]
#[template(source = r#"<body style="margin: 0px">
    <div style="height: 20px" data-on-hover="enter:{{ index }}" data-on-hover-out="leave">row</div>
</body>"#)]
struct HoverModel {
    index: usize,
    received: Vec<HoverMsg>,
}

impl Model for HoverModel {
    type Message = HoverMsg;
    fn update(&mut self, msg: HoverMsg, _context: &mut Context<Self::Message>) {
        self.received.push(msg);
    }
}

/// `data-on-hover` / `data-on-hover-out` send their messages when the
/// pointer enters and leaves the element.
#[test]
fn hover_sends_the_elements_hover_messages() {
    let mut runtime = Runtime::new(HoverModel { index: 3, received: Vec::new() }, MockMeasurer);
    crate::set_size(&mut runtime, 200.0, 200.0);
    runtime.handle_event(InputEvent::Hover { x: 10.0, y: 10.0 });
    runtime.handle_event(InputEvent::Hover { x: 10.0, y: 100.0 });
    assert_eq!(runtime.model().received, [HoverMsg::Enter(3), HoverMsg::Leave]);
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div>{% for item in items %}<p data-key="{{ item }}">{{ item }}</p>{% endfor %}</div>"#)]
struct KeyedModel {
    items: Vec<&'static str>,
}

#[derive(XeruneTemplate)]
#[template(source = r#"<div>{% for item in items %}<p>{{ item }}</p>{% endfor %}</div>"#)]
struct IndexedModel {
    items: Vec<&'static str>,
}

impl Model for KeyedModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

impl Model for IndexedModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context<Self::Message>) {}
}

/// The key of each paragraph, by its text.
fn paragraph_keys(ui: &ui::Ui) -> Vec<(String, u64)> {
    let mut keys: Vec<(String, u64)> = ui.keys.iter().filter_map(|(node, &key)| {
        let children = ui.taffy.children(node).ok()?;
        let [child] = children[..] else { return None };
        match &ui.render_data.get(child)?.kind {
            RenderKind::Text(text) => Some((text.clone(), key)),
            _ => None,
        }
    }).collect();
    keys.sort();
    keys
}

/// In a loop, `data-key` keys an element by its value, so items keep their
/// key when one is inserted before them; without it the index decides.
#[test]
fn data_key_keeps_keys_when_items_are_inserted() {
    let build = |keyed: bool, items: Vec<&'static str>| {
        let ui = if keyed {
            ui::Ui::new_compiled(&KeyedModel { items }, &MockMeasurer, ContainerStyle::default(), &|_| true)
        } else {
            ui::Ui::new_compiled(&IndexedModel { items }, &MockMeasurer, ContainerStyle::default(), &|_| true)
        };
        paragraph_keys(&ui.unwrap())
    };
    let (before, after) = (build(true, vec!["a", "b"]), build(true, vec!["z", "a", "b"]));
    assert_eq!(before, after[..2], "a and b keep their keys");
    let (before, after) = (build(false, vec!["a", "b"]), build(false, vec!["z", "a", "b"]));
    assert_ne!(before, after[..2], "by index, a and b shift");
}

mod canvas {
    //! The canvas background: browsers paint `<html>`'s background (else
    //! `<body>`'s, else white) behind every frame.
    use super::MockMeasurer;
    use std::collections::HashMap;
    use xerune::*;

    macro_rules! canvas_model {
        ($name:ident, $path:literal) => {
            #[derive(XeruneTemplate)]
            #[template(path = $path)]
            struct $name;
            impl Model for $name {
                type Message = String;
                fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
            }
        };
    }
    canvas_model!(BodyBg, "tests/canvas/body_bg.html");
    canvas_model!(NoBg, "tests/canvas/no_bg.html");
    canvas_model!(Covered, "tests/canvas/covered.html");
    canvas_model!(Dark, "tests/canvas/dark.html");
    canvas_model!(Translucent, "tests/canvas/translucent.html");

    const VIEWPORT: Rect = Rect { x: 0.0, y: 0.0, width: 320.0, height: 200.0 };

    fn commands<M: Model + ui::TemplateLayout + 'static>(model: M) -> Vec<DrawCommand> {
        let mut runtime = Runtime::new(model, MockMeasurer);
        crate::set_size(&mut runtime, VIEWPORT.width, VIEWPORT.height);
        runtime.ui.build_commands(&HashMap::new(), None)
    }

    /// Colors of full-viewport rects, in paint order.
    fn canvas_fills(commands: &[DrawCommand]) -> Vec<Color> {
        commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawRect { rect, color: Some(color), .. } if *rect == VIEWPORT => Some(*color),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn body_background_moves_to_the_canvas() {
        let cmds = commands(BodyBg);
        assert_eq!(canvas_fills(&cmds), [Color::new(255, 0, 0, 255)]);
        let red_rects = cmds
            .iter()
            .filter(|c| matches!(c, DrawCommand::DrawRect { color: Some(c), .. } if c.r == 255 && c.g == 0))
            .count();
        assert_eq!(red_rects, 1, "body must not paint its background a second time");
    }

    #[test]
    fn no_background_gets_a_white_canvas() {
        assert_eq!(canvas_fills(&commands(NoBg)), [Color::WHITE]);
    }

    #[test]
    fn opaque_full_screen_box_skips_the_canvas() {
        assert_eq!(canvas_fills(&commands(Covered)), [Color::new(0x10, 0x20, 0x30, 255)]);
    }

    #[test]
    fn color_scheme_dark_darkens_the_default_canvas() {
        assert_eq!(canvas_fills(&commands(Dark)), [Color::new(0x12, 0x12, 0x12, 255)]);
    }

    #[test]
    fn translucent_background_is_painted_over_white() {
        let fills = canvas_fills(&commands(Translucent));
        assert_eq!(fills.len(), 2);
        assert_eq!(fills[0], Color::WHITE);
        assert_eq!((fills[1].r, fills[1].a), (255, 127));
    }
}

/// In the portrait layout the todo list must stay inside the window so it
/// scrolls; a column flex child with `min-height: auto` would grow to fit
/// every item instead.
#[test]
fn todo_list_scrolls_in_portrait() {
    let items = (0..30).map(|i| TodoItem::new(&format!("Item {}", i), false)).collect();
    let model = TestTodoModel { items, active_item: 0, new_item_title: String::new(), lang: "en".to_string() };
    let mut runtime = Runtime::new(model, MockMeasurer);
    crate::set_size(&mut runtime, 480.0, 800.0);

    let ui = &runtime.ui;
    let scroller = ui
        .render_data
        .iter()
        .find_map(|(node, data)| match (&data.kind, &data.style) {
            (RenderKind::Container, s) if s.overflow == Overflow::Scroll => Some(node),
            _ => None,
        })
        .expect("todo list is a scroll container");

    // Absolute bottom edge of the scroll container.
    let mut bottom = ui.taffy.layout(scroller).unwrap().size.height;
    let mut node = Some(scroller);
    while let Some(n) = node {
        bottom += ui.taffy.layout(n).unwrap().location.y;
        node = ui.taffy.parent(n);
    }
    assert!(bottom <= 800.5, "scroll container ends at {bottom}px, below the 800px window");
}

/// Lines of text drawn, in paint order, laid out with the bundled Roboto
/// fonts (as in the examples) at `width`x`height`.
fn roboto_lines<M: Model + ui::TemplateLayout + 'static>(model: M, width: f32, height: f32) -> Vec<String> {
    let mut runtime = Runtime::new(model, roboto());
    set_size(&mut runtime, width, height);
    runtime
        .ui
        .build_commands(&std::collections::HashMap::new(), None)
        .into_iter()
        .filter_map(|c| match c {
            // Like tests/text/chromium_lines.js: whitespace collapsed.
            DrawCommand::DrawText { text, .. } => Some(text.split_whitespace().collect::<Vec<_>>().join(" ")),
            _ => None,
        })
        .collect()
}

mod text_layout {
    use std::collections::HashMap;
    use xerune::*;

    /// 8px per character, 16px tall lines.
    struct CharMeasurer;
    impl TextMeasurer for CharMeasurer {
        fn measure_text(&self, text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (text.chars().count() as f32 * 8.0, 16.0)
        }
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/line_height.html")]
    struct LineHeights;
    impl Model for LineHeights {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    fn line_y(commands: &[DrawCommand], text: &str) -> f32 {
        commands
            .iter()
            .find_map(|c| match c {
                DrawCommand::DrawText { text: t, rect, .. } if t == text => Some(rect.y),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no line {text:?}"))
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/reference.html")]
    struct Reference;
    impl Model for Reference {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// Line breaks of a reference page must match Chromium's with the same
    /// fonts. To regenerate the expected lines:
    /// `tools/chromium-lines.sh tests/text/reference.html`.
    #[test]
    fn line_breaks_match_chromium() {
        let expected = [
            "The quick brown fox jumps over",
            "the lazy dog while the",
            "committee debates.",
            "Centered text that wraps onto",
            "more than one line here.",
            "Right aligned text that also",
            "wraps around.",
            "Supercalifragilisticexpialidocious",
            "words overflow narrow boxes.",
            "Small print: embedded displays are",
            "small, so every line of text has to earn",
            "its place.",
            "Line height 1.6 spreads these",
            "wrapped lines further apart.",
            "Thirty-two pixel lines with small thirteen",
            "pixel text inside them.",
            "The font shorthand sets size and",
            "line height together.",
            "Story title that is long enough",
            "to wrap",
            "A body paragraph in a flex column that may",
            "shrink to nothing, so it wraps.",
            "No wrapping here even though the box is narrow.",
            "Pre-line keeps",
            "the newlines but collapses",
            "spaces and still wraps long",
            "lines.",
            "Pre-wrap keeps every space",
            "and wraps when the line is full.",
            "Pre keeps",
            "indentation",
            "A",
            "Supercalifragilist",
            "icexpialidocious",
            "word.",
            "Break all fills eve",
            "ry line to the edg",
            "e.",
            // `anywhere` lets the flex item shrink below the long word;
            // `break-word` doesn't change min-content, so it overflows.
            "Anywhere:",
            "Pneumonoultrami",
            "croscopic",
            "Breakword:",
            "Pneumonoultramicroscopic",
        ];
        assert_eq!(crate::roboto_lines(Reference, 800.0, 480.0), expected);
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/korean.html")]
    struct Korean;
    impl Model for Korean {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// Korean breaks between any two syllables, or with `word-break:
    /// keep-all` only at spaces, as in Chromium with the same fonts:
    /// `tools/chromium-lines.sh tests/text/korean.html`.
    #[test]
    fn korean_line_breaks_match_chromium() {
        let expected = [
            "다람쥐 헌 쳇바퀴에 타고",
            "파. 키스의 고유조건은 입",
            "술끼리 만나야 하고 특별",
            "한 기술은 필요치 않다.",
            "오늘 할 일 목록",
            "새 펌웨어를 Pi에 플래시",
            "하고 ARM에서 원뿔형 그",
            "라데이션을 프로파일링하",
            "기",
            "Mixed 한국어 and",
            "English text, 숫자 1234",
            "도 함께.",
            "다람쥐 헌 쳇바퀴에",
            "타고파. 키스의",
            "고유조건은 입술끼리",
            "만나야 하고 특별한",
            "기술은 필요치 않다.",
            "한 줄",
        ];
        assert_eq!(crate::roboto_lines(Korean, 800.0, 480.0), expected);
    }

    /// A line with Hangul is as tall as the Hangul font needs: its ascent
    /// and descent are the largest of the fonts on it (24px at 16px, not
    /// Roboto's 19px), as in Chromium, whose heights these are.
    #[test]
    fn korean_line_heights_match_chromium() {
        let mut runtime = Runtime::new(Korean, crate::roboto());
        crate::set_size(&mut runtime, 800.0, 480.0);
        let heights: Vec<f32> = runtime
            .ui
            .build_commands(&HashMap::new(), None)
            .into_iter()
            .filter_map(|c| match c {
                DrawCommand::DrawRect { rect, color: Some(c), .. } if c == Color::new(0xe0, 0xf2, 0xfe, 255) => Some(rect.height),
                _ => None,
            })
            .collect();
        assert_eq!(heights, [96.0, 29.0, 96.0, 72.0, 120.0, 24.0]);
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/kerning.html")]
    struct Kerning;
    impl Model for Kerning {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        .row { display: flex; font-size: 64px; font-weight: bold; }
        .pnum { font-variant-numeric: proportional-nums; }
    </style>
    <div class="row"><span>1111</span></div>
    <div class="row pnum"><span>1111</span></div>
    <div class="row pnum"><span style="font-variant-numeric: normal">1111</span></div>"#)]
    struct NumericVariants;
    impl Model for NumericVariants {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// `font-variant-numeric` is inherited and picks the font's
    /// proportional digits: Roboto Bold's "1" is 985 units wide instead of
    /// the tabular 1175.
    #[test]
    fn font_variant_numeric_picks_proportional_digits() {
        let mut runtime = Runtime::new(NumericVariants, crate::roboto());
        crate::set_size(&mut runtime, 800.0, 480.0);
        let widths: Vec<f32> = runtime
            .ui
            .build_commands(&HashMap::new(), None)
            .into_iter()
            .filter_map(|c| match c {
                DrawCommand::DrawText { rect, .. } => Some((rect.width * 10.0).round() / 10.0),
                _ => None,
            })
            .collect();
        assert_eq!(widths, [146.9, 123.1, 146.9]);
    }

    /// Roboto kerns with OpenType GPOS, which the shaper applies: Chromium
    /// fits "min-" on the first line thanks to it.
    #[test]
    fn gpos_kerning_narrows_lines() {
        assert_eq!(
            crate::roboto_lines(Kerning, 800.0, 480.0),
            ["A body paragraph in a flex column with min-", "width zero, which wraps."]
        );
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/mixed_run.html")]
    struct MixedRun {
        name: String,
        count: u32,
    }
    impl Model for MixedRun {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    #[test]
    fn text_and_values_wrap_as_one_paragraph() {
        let mut rt = Runtime::new(MixedRun { name: "Ada".into(), count: 3 }, CharMeasurer);
        crate::set_size(&mut rt, 400.0, 300.0);
        let lines: Vec<String> = rt
            .ui
            .build_commands(&HashMap::new(), None)
            .into_iter()
            .filter_map(|c| match c {
                DrawCommand::DrawText { text, .. } => Some(text),
                _ => None,
            })
            .collect();
        // 15 characters of 8px fit in 120px; lines break across the values.
        assert_eq!(lines, ["Hello Ada, you", "have 3 new", "messages", "waiting."]);
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/white_space.html")]
    struct WhiteSpaces;
    impl Model for WhiteSpaces {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    #[test]
    fn white_space_modes() {
        let mut rt = Runtime::new(WhiteSpaces, CharMeasurer);
        crate::set_size(&mut rt, 400.0, 600.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        let lines: Vec<&str> = commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawText { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(
            lines,
            [
                // nowrap: one line overflowing the 80px box
                "aaaa bbbb cccc",
                // pre: spaces kept, breaks only at the newline
                "x  y",
                "z",
                // pre-line: spaces collapse, the newline stays
                "p q",
                "r",
                // pre-wrap: spaces kept, wraps; spaces at a break hang
                "eeee",
                "ffff gggg",
                "hhhh",
                "blank",
                "line",
                // <pre> keeps whitespace; a tab goes to the next 8-column stop
                "        tab",
            ]
        );
        // The blank line between two newlines takes a line of its own.
        assert_eq!(line_y(&commands, "line") - line_y(&commands, "blank"), 32.0);
    }

    #[derive(XeruneTemplate)]
    #[template(path = "tests/text/ellipsis.html")]
    struct Ellipsis;
    impl Model for Ellipsis {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    #[test]
    fn text_overflow_ellipsis() {
        let mut rt = Runtime::new(Ellipsis, CharMeasurer);
        crate::set_size(&mut rt, 400.0, 300.0);
        let texts: Vec<(String, f32)> = rt
            .ui
            .build_commands(&HashMap::new(), None)
            .into_iter()
            .filter_map(|c| match c {
                DrawCommand::DrawText { text, rect, .. } => Some((text, rect.x)),
                _ => None,
            })
            .collect();
        let texts: Vec<(&str, f32)> = texts.iter().map(|(t, x)| (t.as_str(), *x)).collect();
        assert_eq!(
            texts,
            [
                // 8px characters and ellipsis: 11 characters + "…" fit in 100px.
                ("abcdefghijk", 0.0),
                ("\u{2026}", 88.0),
                // Without overflow: hidden there is nothing to cut.
                ("qrstuvwxyzqrstuv", 0.0),
                // Text directly in a clipping flex container is cut to it,
                // though the anonymous flex item itself is wider.
                ("ABCDEFGHIJK", 0.0),
                ("\u{2026}", 88.0),
                ("short", 0.0),
            ]
        );
    }

    #[test]
    fn line_height_spaces_wrapped_lines() {
        let mut rt = Runtime::new(LineHeights, CharMeasurer);
        crate::set_size(&mut rt, 400.0, 600.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        let gap = |first: &str, second: &str| line_y(&commands, second) - line_y(&commands, first);
        // A number multiplies the font size; normal here is 16px.
        assert_eq!(gap("aaaa bbbb", "cccc"), 32.0);
        // A percentage is inherited as the length it computed to (30px), not
        // re-applied to the child's 20px font.
        assert_eq!(gap("dddd eeee", "ffff"), 30.0);
        // `font: <size>/<line-height>`, from a stylesheet and inline.
        assert_eq!(gap("gggg hhhh", "iiii"), 25.0);
        assert_eq!(gap("jjjj kkkk", "llll"), 15.0);
    }
}

// Example models, for the `damage` tests.
#[path = "../examples/calculator/calculator_impl.rs"]
#[allow(dead_code)]
mod calculator_impl;
#[path = "../examples/music_player/music_player_impl.rs"]
#[allow(dead_code)]
mod music_player_impl;
#[path = "../examples/showcase/showcase_impl.rs"]
#[allow(dead_code)]
mod showcase_impl;
#[path = "../examples/todo/todo_impl.rs"]
#[allow(dead_code)]
mod todo_impl;

#[derive(Debug, Clone, PartialEq, XeruneMessage)]
enum ClickMsg {
    Pick(usize),
    Name(String),
    Digit(char),
    Reset,
}

#[derive(XeruneTemplate)]
#[template(source = r#"<body style="margin: 0px">
    {% for item in items %}<div style="height: 20px" data-on-click="pick:{{ loop.index0 }}">{{ item }}</div>{% endfor %}
    <div style="height: 20px" data-on-click="name:{{ title }}">name</div>
    <div style="height: 20px" data-on-click="digit:7">7</div>
    <div style="height: 20px" data-on-click="reset">reset</div>
</body>"#)]
struct ClickModel {
    items: Vec<String>,
    title: String,
    received: Vec<ClickMsg>,
}

impl Model for ClickModel {
    type Message = ClickMsg;
    fn update(&mut self, msg: ClickMsg, _context: &mut Context<Self::Message>) {
        self.received.push(msg);
    }
}

/// Clicks deliver the messages the template built, typed: an argument
/// from an expression (by value or through a reference), a static
/// argument, and a plain name.
#[test]
fn clicks_send_typed_messages() {
    let model = ClickModel { items: vec!["a".into(), "b".into()], title: "Ada".into(), received: Vec::new() };
    let mut runtime = Runtime::new(model, MockMeasurer);
    crate::set_size(&mut runtime, 200.0, 200.0);
    for y in [30.0, 50.0, 70.0, 90.0] {
        runtime.handle_event(InputEvent::Click { x: 10.0, y });
    }
    assert_eq!(
        runtime.model().received,
        [ClickMsg::Pick(1), ClickMsg::Name("Ada".into()), ClickMsg::Digit('7'), ClickMsg::Reset]
    );
}

/// Moving the todo selection past the visible rows scrolls the list to the
/// selected row, found by its `id="item-N"`.
#[test]
fn scroll_into_view_reveals_the_element_with_the_id() {
    let mut runtime = Runtime::new(todo_impl::TodoList::default(), roboto());
    set_size(&mut runtime, 800.0, 480.0);
    let rows = runtime.model().items.len();
    let list_scroll = |runtime: &Runtime<todo_impl::TodoList, xerune_render::Fonts>| {
        runtime.ui.scroll_offsets.values().map(|(_, y)| *y).fold(0.0f32, f32::max)
    };
    assert_eq!(list_scroll(&runtime), 0.0);
    for _ in 1..rows {
        runtime.handle_event(InputEvent::KeyDown("ArrowDown".into()));
    }
    assert_eq!(runtime.model().active_item, rows - 1);
    assert!(list_scroll(&runtime) > 0.0, "the last row must be scrolled into view");
}

/// Frames drawn only where the commands changed must look exactly like a
/// full repaint, through real interaction sequences of the examples.
mod damage {
    use super::*;
    use xerune::images::{DefaultImageLoader, ImageError, ImageLoader};
    use xerune_render::SoftwareRenderer;

    use super::{calculator_impl, music_player_impl, showcase_impl, todo_impl};

    const W: u32 = 800;
    const H: u32 = 480;

    use super::roboto as fonts;

    /// Local images only: deterministic and offline.
    struct LocalImages(DefaultImageLoader);

    impl ImageLoader for LocalImages {
        fn fetch(&self, src: &str) -> Result<Vec<u8>, ImageError> {
            if xerune::images::is_remote(src) {
                return Err(ImageError::HttpDisabled);
            }
            self.0.fetch(src)
        }
    }

    struct Session<M: Model + ui::TemplateLayout> {
        runtime: Runtime<M, xerune_render::Fonts>,
        buffer: Vec<u32>,
        renderer: SoftwareRenderer,
    }

    impl<M: Model + ui::TemplateLayout + 'static> Session<M> {
        fn new(model: M) -> Self {
            let mut runtime = Runtime::new(model, fonts());
            runtime.set_image_loader(LocalImages(DefaultImageLoader::default()));
            set_size(&mut runtime, W as f32, H as f32);
            let mut session = Self {
                runtime,
                buffer: vec![0; (W * H) as usize],
                renderer: SoftwareRenderer::new(fonts()),
            };
            session.render_and_compare("first frame");
            session
        }

        /// Renders the next frame as the event loop does and compares it
        /// with a full repaint of the same commands.
        fn render_and_compare(&mut self, step: &str) {
            self.runtime.render(&mut self.renderer, &mut Surface::new(Pixels::Xrgb8888(&mut self.buffer), W, H));

            let commands = self.runtime.ui.build_commands(&self.runtime.context().canvases, None);
            let mut full = vec![0; (W * H) as usize];
            let resources = RenderResources::new(&self.runtime.context().canvases, self.runtime.images());
            let mut target = Surface::new(Pixels::Xrgb8888(&mut full), W, H);
            let area = target.area();
            SoftwareRenderer::new(fonts()).render(&mut target, &commands, &resources, area);

            // The same frame drawn in 40-row bands, as with too little memory
            // for a full frame.
            let mut renderer = SoftwareRenderer::new(fonts());
            let mut band = vec![0; (W * 40) as usize];
            for y in (0..H).step_by(40) {
                let rows = 40.min(H - y);
                let mut target = Surface::new(Pixels::Xrgb8888(&mut band), W, rows).with_origin(0, y as i32);
                let area = target.area();
                renderer.render(&mut target, &commands, &resources, area);
                let rows = (y * W) as usize..((y + rows) * W) as usize;
                if let Some(i) = band[..rows.len()].iter().zip(&full[rows.clone()]).position(|(a, b)| a != b) {
                    panic!("after {step}: banded pixel ({}, {}) differs from a full repaint", i as u32 % W, y + i as u32 / W);
                }
            }

            let wrong = self.buffer.iter().zip(&full).position(|(a, b)| a != b);
            if let Some(i) = wrong {
                if std::env::var("DAMAGE_DEBUG").is_ok() {
                    let (x, y) = ((i as u32 % W) as f32, (i as u32 / W) as f32);
                    eprintln!("damaged {:08x} full {:08x}", self.buffer[i], full[i]);
                    for (n, c) in commands.iter().enumerate() {
                        if let Some(b) = c.bounds() {
                            if b.x <= x && x < b.x + b.width && b.y <= y && y < b.y + b.height {
                                eprintln!("#{n} {}", format!("{c:?}").chars().take(260).collect::<String>());
                            }
                        }
                    }
                }
                panic!("after {step}: pixel ({}, {}) differs from a full repaint", i as u32 % W, i as u32 / W);
            }
        }

        fn send(&mut self, messages: &[&str]) {
            for msg in messages {
                self.runtime.handle_messages([msg.to_string()]);
                self.render_and_compare(msg);
            }
        }
    }

    #[test]
    fn todo_frames_match_full_repaints() {
        Session::new(todo_impl::TodoList::default())
            .send(&["toggle:0", "toggle:1", "toggle:0", "remove:0", "todo_input:text:Buy milk", "add", "toggle:2"]);
    }

    #[test]
    fn calculator_frames_match_full_repaints() {
        Session::new(calculator_impl::CalculatorModel::default())
            .send(&["digit:7", "digit:8", "op:*", "digit:9", "equals", "negate", "percent", "clear", "clear"]);
    }

    #[test]
    fn showcase_frames_match_full_repaints() {
        let mut session = Session::new(showcase_impl::ShowcaseModel::default());
        session.send(&["toggle_wifi", "increment_progress", "increment_user_counter", "toggle_engine", "increment_speed"]);
        for tab in 1..5 {
            session.send(&[&format!("select_tab:{tab}"), "increment_progress", "toggle_maintenance"]);
        }
    }

    #[test]
    fn music_player_frames_match_full_repaints() {
        let mut session = Session::new(music_player_impl::MusicPlayerModel::new(None));
        // Covers arrive after the first frame; their commands stay the same.
        session.runtime.wait_for_images(std::time::Duration::from_secs(10));
        session.render_and_compare("images arrived");
        session.send(&["hover_track:2", "hover_track:4", "unhover_track", "select_track:1", "play_pause", "next", "back"]);
    }
}

mod controls {
    //! Form controls are drawn like Chromium draws them (geometry and colors
    //! measured from its screenshots), tinted by `accent-color`.
    use std::collections::HashMap;
    use xerune::style::{BorderSide, Borders};
    use xerune::*;

    struct NoText;
    impl TextMeasurer for NoText {
        fn measure_text(&self, _text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (0.0, 16.0)
        }
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        .row { display: flex; align-items: flex-start; }
        .red { accent-color: #e11d48; }
        .big { width: 30px; height: 30px; }
    </style>
    <body>
        <div class="row"><input type="checkbox"><input type="checkbox" checked></div>
        <div class="row red"><input type="checkbox" checked><input type="range" value="0"><progress value="0.25"></progress></div>
        <div class="row"><input type="checkbox" class="big" checked></div>
    </body>"#)]
    struct Controls;
    impl Model for Controls {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    fn commands<M: Model + ui::TemplateLayout + 'static>(model: M) -> Vec<DrawCommand> {
        let mut rt = Runtime::new(model, NoText);
        crate::set_size(&mut rt, 400.0, 300.0);
        rt.ui.build_commands(&HashMap::new(), None)
    }

    fn rgb(hex: u32) -> Color {
        Color::new((hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255)
    }

    /// The `DrawRect` drawn at `rect`, as (fill, border, radius).
    fn rect_at(commands: &[DrawCommand], rect: Rect) -> (Option<Color>, Borders, f32) {
        commands
            .iter()
            .find_map(|c| match c {
                DrawCommand::DrawRect { rect: r, color, border, border_radius, .. } if *r == rect => Some((*color, *border, *border_radius)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no rect at {rect:?} in {commands:#?}"))
    }

    fn border(width: f32, hex: u32) -> Borders {
        Borders::all(BorderSide::solid(width, rgb(hex)))
    }

    #[test]
    fn unchecked_checkbox_is_a_white_box_with_a_grey_border() {
        // 13x13 with margins 3px 3px 3px 4px.
        let commands = commands(Controls);
        assert_eq!(rect_at(&commands, Rect::new(4.0, 3.0, 13.0, 13.0)), (Some(Color::WHITE), border(1.0, 0x767676), 2.0));
    }

    #[test]
    fn checked_checkbox_is_filled_with_the_accent_and_checked() {
        let commands = commands(Controls);
        let rect = Rect::new(24.0, 3.0, 13.0, 13.0);
        assert_eq!(rect_at(&commands, rect), (Some(rgb(0x0075ff)), Borders::NONE, 2.0));
        assert!(commands.contains(&DrawCommand::DrawCheckMark { rect, color: Color::WHITE }));
    }

    #[test]
    fn accent_color_is_inherited_by_every_control() {
        let commands = commands(Controls);
        let red = Some(rgb(0xe11d48));
        // Second row, 19px down.
        assert_eq!(rect_at(&commands, Rect::new(4.0, 22.0, 13.0, 13.0)).0, red);
        // The range's thumb: 129x16 at (22, 21), the track inset by 1px,
        // the thumb at its start.
        assert_eq!(rect_at(&commands, Rect::new(23.0, 21.0, 16.0, 16.0)), (red, Borders::NONE, 8.0));
        // The progress bar's value is the accent with a border halfway to
        // #757575, clipped to a quarter of the 160x16 bar's 8px track.
        let track = Rect::new(153.0, 23.0, 160.0, 8.0);
        assert_eq!(rect_at(&commands, track), (Some(rgb(0xefefef)), border(1.0, 0xb2b2b2), 4.0));
        let value = commands.iter().position(|c| *c == DrawCommand::Clip { rect: Rect { width: 40.0, ..track } }).unwrap();
        assert_eq!(
            commands[value + 1],
            DrawCommand::DrawRect { rect: track, color: red, gradient: None, border_radius: 4.0, border: border(1.0, 0xab495f) }
        );
    }

    #[test]
    fn css_sizes_the_controls() {
        let commands = commands(Controls);
        assert_eq!(rect_at(&commands, Rect::new(4.0, 42.0, 30.0, 30.0)).0, Some(rgb(0x0075ff)));
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        .column { display: flex; flex-direction: column; align-items: flex-start; }
    </style>
    <body><div class="column">
        <input type="range" min="0" max="10" value="5">
        <input type="range">
        <input type="range" value="30">
        <input type="range" max="10" step="5" value="7">
        <input type="range" min="10" max="0" value="3">
        <input type="range" step="any" value="33.3">
        <input type="range" value="150">
    </div></body>"#)]
    struct Ranges;
    impl Model for Ranges {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// A range's value is placed between `min` and `max` (default 0..100)
    /// after HTML's sanitization: it defaults to the middle, is clamped,
    /// and snaps to `step` (default 1).
    #[test]
    fn range_value_follows_min_max_and_step() {
        let commands = commands(Ranges);
        let thumbs: Vec<f32> = commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawRect { rect, border_radius, .. } if *border_radius == 8.0 => Some(rect.x),
                _ => None,
            })
            .collect();
        // Each 129px range's thumb travels along the 127px track minus itself.
        let at = |fraction: f32| 3.0 + fraction * 111.0;
        assert_eq!(thumbs, [at(0.5), at(0.5), at(0.3), at(0.5), at(0.0), at(0.333), at(1.0)]);
    }

    #[derive(Debug, Clone, PartialEq, XeruneMessage)]
    enum RangeMsg {
        #[xerune(prefix = "set:")]
        Set(f32),
        #[xerune(prefix = "done:")]
        Done(f32),
        Clicked,
    }

    /// A range whose value the model owns, as in the music player's seek bar.
    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        .list { height: 100px; overflow: scroll; display: flex; flex-direction: column; }
        .tall { height: 300px; flex-shrink: 0; }
        input { flex-shrink: 0; align-self: flex-start; }
    </style>
    <body><div class="list" data-on-click="clicked">
        <input type="range" min="0" max="10" value="{{ value }}" data-on-input="set" data-on-change="done">
        <div class="tall"></div>
    </div></body>"#)]
    struct Seek {
        value: f32,
        sent: Vec<RangeMsg>,
    }
    impl Model for Seek {
        type Message = RangeMsg;
        fn update(&mut self, msg: RangeMsg, _context: &mut Context<Self::Message>) {
            if let RangeMsg::Set(value) = msg {
                self.value = value;
            }
            self.sent.push(msg);
        }
    }

    /// Pressing a range moves its thumb's middle under the pointer and
    /// dragging keeps it there, within the ends, sending `data-on-input`
    /// with each new value; letting go sends `data-on-change`. The touch
    /// neither scrolls nor clicks.
    #[test]
    fn dragging_a_range_sends_its_value() {
        let mut rt = Runtime::new(Seek { value: 5.0, sent: Vec::new() }, NoText);
        crate::set_size(&mut rt, 400.0, 300.0);
        // The 129px range's thumb middle travels from 11 to 122 (see
        // `range_value_follows_min_max_and_step`).
        let x = |value: f32| 11.0 + value / 10.0 * 111.0;
        rt.handle_event(InputEvent::TouchStart { id: 1, x: x(3.0), y: 10.0 });
        assert_eq!(rt.model().sent, [RangeMsg::Set(3.0)]);
        rt.handle_event(InputEvent::TouchMove { id: 1, x: x(3.2), y: 60.0 });
        assert_eq!(rt.model().sent, [RangeMsg::Set(3.0)], "the same step");
        rt.handle_event(InputEvent::TouchMove { id: 1, x: 390.0, y: 60.0 });
        rt.handle_event(InputEvent::TouchMove { id: 1, x: 395.0, y: 80.0 });
        rt.handle_event(InputEvent::TouchEnd { id: 1, x: 395.0, y: 80.0 });
        assert_eq!(rt.model().sent, [RangeMsg::Set(3.0), RangeMsg::Set(10.0), RangeMsg::Done(10.0)]);
        assert_eq!(rt.ui.scroll_offsets.iter().next().map(|(_, offset)| *offset), None, "no scrolling");

        // Pressing the thumb where it is changes nothing; a tap off the range clicks.
        rt.model_mut().sent.clear();
        rt.handle_event(InputEvent::TouchStart { id: 2, x: x(10.0), y: 10.0 });
        rt.handle_event(InputEvent::TouchEnd { id: 2, x: x(10.0), y: 10.0 });
        rt.handle_event(InputEvent::TouchStart { id: 3, x: 300.0, y: 50.0 });
        rt.handle_event(InputEvent::TouchEnd { id: 3, x: 300.0, y: 50.0 });
        assert_eq!(rt.model().sent, [RangeMsg::Clicked]);
    }

    /// A range's value as text has the step's decimals at most.
    #[test]
    fn range_values_are_formatted_by_step() {
        let range = |step| style::Range { min: 0.0, max: 1.0, step, value: None };
        assert_eq!(range(Some(0.1)).format(range(Some(0.1)).value_at(0.3)), "0.3");
        assert_eq!(range(Some(1.0)).format(1.0), "1");
        assert_eq!(range(Some(0.25)).format(0.5), "0.5");
        assert_eq!(range(None).format(0.123456), "0.123456");
        let gain = style::Range { min: -12.0, max: 12.0, step: Some(1.0), value: None };
        assert_eq!(gain.format(gain.value_at(0.25)), "-6");
        assert_eq!(gain.format(gain.value_at(0.5)), "0");
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        .switch { appearance: none; width: 40px; height: 20px; margin: 0px; border-radius: 10px;
                  background-color: #cccccc; border: 2px solid #999999; }
        .switch:checked { background-color: #22c55e; }
        .switch.loud:checked { border-color: #000000; }
        progress, input[type=range] { appearance: none; margin: 0px; }
    </style>
    <body>
        <input type="checkbox" class="switch {% if loud %}loud{% endif %}" {% if on %}checked{% endif %}>
        <div><progress value="0.25"></progress></div>
        <div><input type="range" value="0"></div>
    </body>"#)]
    struct Unstyled {
        on: bool,
        loud: bool,
    }
    impl Model for Unstyled {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// `appearance: none` makes a checkbox a plain CSS box, styled per
    /// state with `:checked` (here following `{% if on %}checked{% endif %}`).
    #[test]
    fn checkbox_without_appearance_is_its_css_box() {
        // Checkboxes are border-box, as in Chromium's html.css.
        let switch = Rect::new(0.0, 0.0, 40.0, 20.0);
        let off = commands(Unstyled { on: false, loud: true });
        assert_eq!(rect_at(&off, switch), (Some(rgb(0xcccccc)), border(2.0, 0x999999), 10.0));
        assert!(!off.iter().any(|c| matches!(c, DrawCommand::DrawCheckMark { .. })));
        let on = commands(Unstyled { on: true, loud: false });
        assert_eq!(rect_at(&on, switch), (Some(rgb(0x22c55e)), border(2.0, 0x999999), 10.0));
        let loud = commands(Unstyled { on: true, loud: true });
        assert_eq!(rect_at(&loud, switch), (Some(rgb(0x22c55e)), border(2.0, 0x000000), 10.0));
    }

    /// Chromium's fallbacks: html.css's grey bar with a green value, and a
    /// range with only its thumb.
    #[test]
    fn progress_and_range_without_appearance() {
        let commands = commands(Unstyled { on: false, loud: false });
        let bar = commands.iter().position(|c| matches!(c, DrawCommand::DrawRect { color: Some(c), .. } if *c == rgb(0x808080))).unwrap();
        let DrawCommand::DrawRect { rect, .. } = commands[bar] else { unreachable!() };
        assert_eq!((rect.width, rect.height), (160.0, 16.0));
        assert_eq!(commands[bar + 1], DrawCommand::DrawRect { rect: Rect { width: 40.0, ..rect }, color: Some(rgb(0x008000)), gradient: None, border_radius: 0.0, border: Borders::NONE });
        let rounded: Vec<f32> = commands.iter().filter_map(|c| match c {
            DrawCommand::DrawRect { border_radius, .. } if *border_radius > 0.0 => Some(*border_radius),
            _ => None,
        }).collect();
        // The switch, then the range's thumb alone.
        assert_eq!(rounded, [10.0, 8.0]);
    }

    /// Runtime stylesheets know `:checked` too.
    #[test]
    fn runtime_css_matches_checked() {
        struct Checked(bool);
        impl Model for Checked {
            type Message = String;
            fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
        }
        impl ui::TemplateLayout for Checked {
            fn stylesheet(&self) -> &'static str {
                "body { margin: 0px; } input { appearance: none; width: 20px; height: 20px; margin: 0px; background-color: red; } input:checked { background-color: blue; }"
            }
            fn build_ui(
                &self,
                builder: &mut ui::UiBuilder,
                _measurer: &impl TextMeasurer,
                _default_style: &style::ContainerStyle,
                _message_validator: &impl Fn(&str) -> bool,
            ) -> taffy::NodeId {
                let body = builder.create_element("body", &[]);
                let checkbox = builder.create_checkbox(self.0, &[("type", "checkbox")]);
                builder.append_child(body, checkbox);
                body
            }
        }
        let rect = Rect::new(0.0, 0.0, 20.0, 20.0);
        assert_eq!(rect_at(&commands(Checked(false)), rect).0, Some(rgb(0xff0000)));
        assert_eq!(rect_at(&commands(Checked(true)), rect).0, Some(rgb(0x0000ff)));
    }

    const PART_CSS: &str = "
        body { margin: 0px; }
        div { display: flex; }
        input[type=range] { -webkit-appearance: none; width: 200px; height: 24px; margin: 0px; }
        .s::-webkit-slider-runnable-track { height: 6px; border-radius: 3px; background: #334155; }
        .s::-webkit-slider-thumb { -webkit-appearance: none; width: 18px; height: 18px; border-radius: 50%;
                                   background: #f59e0b; margin-top: -6px; border: 2px solid #ffffff; }
        .red::-webkit-slider-thumb { background: red; }
        progress { appearance: none; width: 200px; height: 10px; }
        progress::-webkit-progress-bar { background: #e2e8f0; border-radius: 5px; }
        progress::-webkit-progress-value { background: linear-gradient(90deg, #22d3ee 0%, #a78bfa 100%); border-radius: 5px; }
        *::-moz-range-progress { background: blue; }
    ";

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        div { display: flex; }
        input[type=range] { -webkit-appearance: none; width: 200px; height: 24px; margin: 0px; }
        .s::-webkit-slider-runnable-track { height: 6px; border-radius: 3px; background: #334155; }
        .s::-webkit-slider-thumb { -webkit-appearance: none; width: 18px; height: 18px; border-radius: 50%;
                                   background: #f59e0b; margin-top: -6px; border: 2px solid #ffffff; }
        .red::-webkit-slider-thumb { background: red; }
        progress { appearance: none; width: 200px; height: 10px; }
        progress::-webkit-progress-bar { background: #e2e8f0; border-radius: 5px; }
        progress::-webkit-progress-value { background: linear-gradient(90deg, #22d3ee 0%, #a78bfa 100%); border-radius: 5px; }
        *::-moz-range-progress { background: blue; }
    </style>
    <body>
        <div><input type="range" class="s" value="30"></div>
        <div><progress value="0.5"></progress></div>
        <div><input type="range" class="red" value="50" style="appearance: auto"></div>
    </body>"#)]
    struct Parts;
    impl Model for Parts {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// Typical web CSS for range and progress parts, laid out as in
    /// Chromium (compared against its screenshots while writing this).
    fn check_parts(commands: &[DrawCommand]) {
        // The track spans the range, centered; the thumb (appearance: none)
        // sits at the track's top plus its margin, 30% along the track minus
        // itself, on whole pixels.
        let fill_and_radius = |rect| { let (fill, _, radius) = rect_at(commands, rect); (fill, radius) };
        assert_eq!(fill_and_radius(Rect::new(0.0, 9.0, 200.0, 6.0)), (Some(rgb(0x334155)), 3.0));
        assert_eq!(rect_at(commands, Rect::new(55.0, 3.0, 18.0, 18.0)), (Some(rgb(0xf59e0b)), border(2.0, 0xffffff), 9999.0));
        // The fill (`::-moz-range-progress`) runs to the thumb's middle.
        assert_eq!(rect_at(commands, Rect::new(0.0, 9.0, 64.0, 6.0)).0, Some(rgb(0x0000ff)));

        let bar = Rect::new(0.0, 24.0, 200.0, 10.0);
        assert_eq!(fill_and_radius(bar), (Some(rgb(0xe2e8f0)), 5.0));
        let value = commands.iter().find_map(|c| match c {
            DrawCommand::DrawRect { rect, gradient: Some(_), border_radius, .. } => Some((*rect, *border_radius)),
            _ => None,
        });
        assert_eq!(value, Some((Rect { width: 100.0, ..bar }, 5.0)));

        // Styling only the thumb's color keeps the native track and shape.
        assert_eq!(rect_at(commands, Rect::new(1.0, 34.0 + 8.0, 198.0, 8.0)).0, Some(rgb(0xefefef)));
        assert_eq!(fill_and_radius(Rect::new(1.0 + 0.5 * 182.0, 34.0 + 4.0, 16.0, 16.0)), (Some(rgb(0xff0000)), 8.0));
    }

    #[test]
    fn parts_are_styled_with_pseudo_elements() {
        check_parts(&commands(Parts));
    }

    #[test]
    fn runtime_css_styles_parts() {
        struct RuntimeParts;
        impl Model for RuntimeParts {
            type Message = String;
            fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
        }
        impl ui::TemplateLayout for RuntimeParts {
            fn stylesheet(&self) -> &'static str {
                PART_CSS
            }
            fn build_ui(
                &self,
                builder: &mut ui::UiBuilder,
                _measurer: &impl TextMeasurer,
                _default_style: &style::ContainerStyle,
                _message_validator: &impl Fn(&str) -> bool,
            ) -> taffy::NodeId {
                let body = builder.create_element("body", &[]);
                for node in [
                    builder.create_slider(30.0, &[("type", "range"), ("class", "s")]),
                    builder.create_progress(0.5, 1.0, &[]),
                    builder.create_slider(50.0, &[("type", "range"), ("class", "red"), ("style", "appearance: auto")]),
                ] {
                    let row = builder.create_element("div", &[]);
                    builder.append_child(row, node);
                    builder.append_child(body, row);
                }
                body
            }
        }
        check_parts(&commands(RuntimeParts));
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        .styled::placeholder { color: #ff0000; font-size: 20px; }
    </style>
    <body>
        <input type="text" value="{{ text }}" placeholder="Type here">
        <input type="text" class="styled" placeholder="Search">
    </body>"#)]
    struct Placeholders {
        text: String,
    }
    impl Model for Placeholders {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    fn text(commands: &[DrawCommand], wanted: &str) -> Option<(Color, f32)> {
        commands.iter().find_map(|c| match c {
            DrawCommand::DrawText { text, color, font_size, .. } if text == wanted => Some((*color, *font_size)),
            _ => None,
        })
    }

    /// An empty input shows its placeholder in Chromium's #757575, styled
    /// by `::placeholder`; typed text replaces it.
    #[test]
    fn empty_input_shows_placeholder() {
        let empty = commands(Placeholders { text: String::new() });
        assert_eq!(text(&empty, "Type here"), Some((rgb(0x757575), 16.0)));
        assert_eq!(text(&empty, "Search"), Some((rgb(0xff0000), 20.0)));
        let typed = commands(Placeholders { text: "hello".into() });
        assert_eq!(text(&typed, "Type here"), None);
        assert_eq!(text(&typed, "hello"), Some((rgb(0x000000), 16.0)));
    }

    #[derive(XeruneTemplate)]
    #[template(source = r#"<input value="{{ text }}" placeholder="Type here">"#)]
    struct UntypedInput {
        text: String,
    }

    impl Model for UntypedInput {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// An `<input>` without a `type` is a text input, as in HTML: compiled
    /// and built at runtime.
    #[test]
    fn untyped_inputs_are_text_inputs() {
        assert_eq!(text(&commands(UntypedInput { text: String::new() }), "Type here"), Some((rgb(0x757575), 16.0)));
        assert_eq!(text(&commands(UntypedInput { text: "hi".into() }), "hi"), Some((rgb(0x000000), 16.0)));

        struct Runtime;
        impl Model for Runtime {
            type Message = String;
            fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
        }
        impl ui::TemplateLayout for Runtime {
            fn stylesheet(&self) -> &'static str {
                ""
            }
            fn build_ui(&self, builder: &mut ui::UiBuilder, _: &impl TextMeasurer, _: &style::ContainerStyle, _: &impl Fn(&str) -> bool) -> taffy::NodeId {
                let body = builder.create_element("body", &[]);
                let input = builder.create_input_text("", &[("placeholder", "Type here")]);
                builder.append_child(body, input);
                body
            }
        }
        assert_eq!(text(&commands(Runtime), "Type here"), Some((rgb(0x757575), 16.0)));
    }

    /// Runtime stylesheets (no compiled template) must not reset an
    /// `<input>`'s size to its type's defaults after CSS set it.
    #[test]
    fn runtime_css_sizes_inputs() {
        struct Runtime;
        impl Model for Runtime {
            type Message = String;
            fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
        }
        impl ui::TemplateLayout for Runtime {
            fn stylesheet(&self) -> &'static str {
                "body { margin: 0px; } input { width: 30px; height: 20px; margin: 0px; }"
            }
            fn build_ui(
                &self,
                builder: &mut ui::UiBuilder,
                _measurer: &impl TextMeasurer,
                _default_style: &style::ContainerStyle,
                _message_validator: &impl Fn(&str) -> bool,
            ) -> taffy::NodeId {
                let body = builder.create_element("body", &[]);
                let checkbox = builder.create_checkbox(false, &[("type", "checkbox")]);
                builder.append_child(body, checkbox);
                body
            }
        }
        let commands = commands(Runtime);
        assert_eq!(rect_at(&commands, Rect::new(0.0, 0.0, 30.0, 20.0)).0, Some(Color::WHITE));
    }

    const SWITCH_CSS: &str = "
        body { margin: 0px; }
        div { display: flex; }
        .switch { appearance: none; position: relative; width: 40px; height: 22px; margin: 0px; background-color: #1e293b; }
        .switch::before { content: ''; position: absolute; left: 3px; top: 3px; width: 16px; height: 16px; background-color: #e2e8f0; }
        .switch:checked::before { left: 21px; }
        p { margin: 0px; }
        p:before { content: \"A\"; color: #ff0000; }
        p::after { content: 'Z'; color: #0000ff; }
    ";

    #[derive(XeruneTemplate)]
    #[template(source = r#"<style>
        body { margin: 0px; }
        div { display: flex; }
        .switch { appearance: none; position: relative; width: 40px; height: 22px; margin: 0px; background-color: #1e293b; }
        .switch::before { content: ''; position: absolute; left: 3px; top: 3px; width: 16px; height: 16px; background-color: #e2e8f0; }
        .switch:checked::before { left: 21px; }
        p { margin: 0px; }
        p:before { content: "A"; color: #ff0000; }
        p::after { content: 'Z'; color: #0000ff; }
    </style>
    <body>
        <div><input type="checkbox" class="switch" {% if on %}checked{% endif %}></div>
        <p>mid</p>
    </body>"#)]
    struct Switch {
        on: bool,
    }
    impl Model for Switch {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
    }

    /// A toggle switch: a checkbox whose knob is its `::before`, moved by
    /// `:checked`; `::before` / `::after` text goes around the element's.
    fn check_switch(off: &[DrawCommand], on: &[DrawCommand]) {
        let knob = Some(rgb(0xe2e8f0));
        assert_eq!(rect_at(off, Rect::new(0.0, 0.0, 40.0, 22.0)).0, Some(rgb(0x1e293b)));
        assert_eq!(rect_at(off, Rect::new(3.0, 3.0, 16.0, 16.0)).0, knob);
        assert_eq!(rect_at(on, Rect::new(21.0, 3.0, 16.0, 16.0)).0, knob);
        let texts: Vec<(&str, Color)> = on.iter().filter_map(|c| match c {
            DrawCommand::DrawText { text, color, .. } => Some((text.as_str(), *color)),
            _ => None,
        }).collect();
        assert_eq!(texts, [("A", rgb(0xff0000)), ("mid", rgb(0x000000)), ("Z", rgb(0x0000ff))]);
    }

    #[test]
    fn before_and_after_generate_boxes() {
        check_switch(&commands(Switch { on: false }), &commands(Switch { on: true }));
    }

    #[test]
    fn runtime_css_generates_before_and_after() {
        struct RuntimeSwitch(bool);
        impl Model for RuntimeSwitch {
            type Message = String;
            fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
        }
        impl ui::TemplateLayout for RuntimeSwitch {
            fn stylesheet(&self) -> &'static str {
                SWITCH_CSS
            }
            fn build_ui(
                &self,
                builder: &mut ui::UiBuilder,
                _measurer: &impl TextMeasurer,
                _default_style: &style::ContainerStyle,
                _message_validator: &impl Fn(&str) -> bool,
            ) -> taffy::NodeId {
                let body = builder.create_element("body", &[]);
                let row = builder.create_element("div", &[]);
                let switch = builder.create_checkbox(self.0, &[("type", "checkbox"), ("class", "switch")]);
                builder.append_child(row, switch);
                builder.append_child(body, row);
                let p = builder.create_element("p", &[]);
                let text = builder.create_text("mid", &[]);
                builder.append_child(p, text);
                builder.append_child(body, p);
                body
            }
        }
        check_switch(&commands(RuntimeSwitch(false)), &commands(RuntimeSwitch(true)));
    }
}

mod known_gaps {
    //! Things the engine gets wrong today. Each test describes the correct
    //! (browser) behavior and is ignored with a FIXME until it is fixed:
    //! `cargo test -- --ignored` lists what is still broken. Remove the
    //! `#[ignore]` together with the fix.
    use std::collections::HashMap;
    use xerune::*;

    /// 8px per character, so long text really is long.
    struct CharMeasurer;
    impl TextMeasurer for CharMeasurer {
        fn measure_text(&self, text: &str, _font_size: f32, _weight: u16) -> (f32, f32) {
            (text.chars().count() as f32 * 8.0, 16.0)
        }
    }

    macro_rules! gap_model {
        ($name:ident, $path:literal) => {
            #[derive(XeruneTemplate)]
            #[template(path = $path)]
            struct $name;
            impl Model for $name {
                type Message = String;
                fn update(&mut self, _msg: String, _context: &mut Context<Self::Message>) {}
            }
        };
    }
    gap_model!(TextWrap, "tests/gaps/text_wrap.html");
    gap_model!(ImgShadow, "tests/gaps/img_shadow.html");
    gap_model!(CanvasResize, "tests/gaps/canvas_resize.html");
    gap_model!(InlineWrap, "tests/gaps/inline_wrap.html");
    gap_model!(ControlMinHeight, "tests/gaps/control_min_height.html");

    fn runtime<M: Model + ui::TemplateLayout + 'static>(model: M, w: f32, h: f32) -> Runtime<M, CharMeasurer> {
        let mut runtime = Runtime::new(model, CharMeasurer);
        crate::set_size(&mut runtime, w, h);
        runtime
    }

    fn text_rect(commands: &[DrawCommand], text: &str) -> Rect {
        commands
            .iter()
            .find_map(|c| match c {
                DrawCommand::DrawText { text: t, rect, .. } if t.contains(text) => Some(*rect),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no text {text:?}"))
    }

    #[test]
    fn text_wraps_to_its_container() {
        let rt = runtime(TextWrap, 400.0, 300.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        let lines: Vec<Rect> = commands
            .iter()
            .filter_map(|c| match c {
                DrawCommand::DrawText { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect();
        assert!(lines.iter().all(|r| r.width <= 100.0), "a line overflows the 100px box: {lines:?}");
        assert!(lines.len() > 1, "expected the sentence on several lines");
    }

    #[test]
    #[ignore = "FIXME: form controls have no automatic minimum size: a range and a checkbox shrink to 0px in an overflowing flex column"]
    fn controls_keep_their_height_in_a_shrinking_column() {
        let rt = runtime(ControlMinHeight, 400.0, 300.0);
        let heights: Vec<f32> = rt.ui.render_data.iter()
            .filter(|(_, data)| matches!(data.kind, style::RenderKind::Slider(..) | style::RenderKind::Progress(..) | style::RenderKind::Checkbox(_)))
            .map(|(node, _)| rt.ui.taffy.layout(node).unwrap().size.height)
            .collect();
        assert_eq!(heights, [16.0, 0.0, 13.0]);
    }

    #[test]
    #[ignore = "FIXME: inline elements are separate flex items, not one inline formatting context: text after <b> starts a new line and the space before it is lost"]
    fn text_flows_around_inline_elements() {
        // With 8px characters, "Some bold words in a" (160px) fits the 240px
        // line, so the text after </b> continues on the line of "bold".
        let rt = runtime(InlineWrap, 400.0, 300.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        let some = text_rect(&commands, "Some");
        let bold = text_rect(&commands, "bold");
        let words = text_rect(&commands, "words");
        assert_eq!(words.y, bold.y, "text after </b> must stay on its line");
        assert!(bold.x >= some.x + some.width + 8.0, "the space before <b> must remain");
    }

    #[test]
    #[ignore = "FIXME: <img> paints only the image, no box-shadow (or background/border)"]
    fn image_box_shadow_is_drawn() {
        let rt = runtime(ImgShadow, 400.0, 300.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        assert!(commands.iter().any(|c| matches!(c, DrawCommand::DrawBoxShadow { .. })));
    }

    #[test]
    #[ignore = "FIXME: a canvas keeps the pixel size of its first layout"]
    fn canvas_follows_its_layout_size() {
        let mut rt = runtime(CanvasResize, 800.0, 480.0);
        assert_eq!(rt.context().canvases["c"].width, 100);
        crate::set_size(&mut rt, 480.0, 800.0);
        assert_eq!(rt.context().canvases["c"].width, 60);
    }
}

/// A recording dispatches `@MS` messages when their time comes, and its
/// frames, each repainting only what changed in the one buffer, end up as
/// a fresh snapshot of the same state.
#[test]
fn snapshot_recording_dispatches_timed_messages() {
    use xerune::backend::SnapshotBackend;
    let fonts = roboto();
    let mut recorder = SnapshotBackend::new("unused.png");
    recorder.size = Some((800, 480));
    recorder.messages = ["toggle:2", "@150", "remove:0", "@250", "toggle:0"].map(String::from).to_vec();
    recorder.frames = 4;
    recorder.fps = 10;
    let mut frames = Vec::new();
    let mut runtime = Runtime::new(todo_impl::TodoList::default(), fonts.clone());
    recorder
        .record(&mut runtime, &mut xerune_render::SoftwareRenderer::new(fonts.clone()), 800, 480, |_, pixels, _, _| {
            frames.push(pixels.to_vec());
            Ok(())
        })
        .unwrap();

    // Frames at 0, 100, 200 and 300 ms: changes at 150 and 250 ms.
    assert_eq!(frames.len(), 4);
    assert_eq!(frames[0], frames[1]);
    assert_ne!(frames[1], frames[2]);
    assert_ne!(frames[2], frames[3]);

    let mut snapshot = SnapshotBackend::new("unused.png");
    snapshot.messages = ["toggle:2", "remove:0", "toggle:0"].map(String::from).to_vec();
    let mut runtime = Runtime::new(todo_impl::TodoList::default(), fonts.clone());
    let (pixels, _, _) = snapshot.render(&mut runtime, &mut xerune_render::SoftwareRenderer::new(fonts), 800, 480).unwrap();
    assert!(frames[3] == pixels, "the last frame differs from a fresh snapshot of the same state");
}
