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
        })
        .clone()
}

struct MockModel;
#[derive(Debug, PartialEq, XeruneMessage)]
enum MockMsg {
    Tick,
}

impl Model for MockModel {
    type Message = MockMsg;
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
#[template(path = "tests/test_template.html")]
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
    tag: String,
    completed: bool,
}

impl TodoItem {
    fn new(title: &str, completed: bool) -> Self {
        Self { title: title.to_string(), tag: "work".to_string(), completed }
    }
}

#[derive(Debug, PartialEq, XeruneMessage)]
enum TestTodoMsg {
    Toggle(usize),
    Remove(usize),
    Add,
    #[xerune(prefix = "todo_input:text:")]
    TodoInput(String),
    #[xerune(prefix = "keydown:")]
    KeyDown(String),
}

#[derive(XeruneTemplate)]
#[template(path = "todo_list.html")]
struct TestTodoModel {
    items: Vec<TodoItem>,
    active_item: usize,
    new_item_title: String,
}

impl TestTodoModel {
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
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
    #[derive(Debug, PartialEq, XeruneMessage)]
    enum TouchMockMsg {
        #[xerune(rename = "click_action")]
        ClickMsg,
    }
    impl Model for TouchMockModel {
        type Message = TouchMockMsg;
        fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
fn test_timer_cancellation() {
    let model = MockModel;
    let measurer = MockMeasurer;
    let mut runtime = Runtime::new(model, measurer);

    runtime.set_interval("tick".to_string(), 100);
    assert_eq!(runtime.timers().len(), 1, "Should have 1 registered timer");

    runtime.clear_interval("tick");
    assert_eq!(runtime.timers().len(), 0, "Timer should be cleared by clear_interval");

    // Test cancellation via context command
    let mut context = Context::new();
    context.set_interval("tick".to_string(), 100);
    context.clear_interval("tick");
    
    runtime.model_mut().update(MockMsg::Tick, &mut context);
    runtime.sync_view();

    assert_eq!(runtime.timers().len(), 0, "Timer should be cleared when processing context commands");
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
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
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
    assert!(format!("{interaction:?}").contains("tick"), "the topmost box gets the click: {interaction:?}");
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
                fn update(&mut self, _msg: String, _context: &mut Context) {}
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
    let model = TestTodoModel { items, active_item: 0, new_item_title: String::new() };
    let mut runtime = Runtime::new(model, MockMeasurer);
    crate::set_size(&mut runtime, 480.0, 800.0);

    let ui = &runtime.ui;
    let scroller = ui
        .render_data
        .iter()
        .find_map(|(node, data)| match data {
            xerune::style::RenderData::Container(s) if s.overflow == Overflow::Scroll => Some(node),
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
        fn update(&mut self, _msg: String, _context: &mut Context) {}
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
        fn update(&mut self, _msg: String, _context: &mut Context) {}
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
    #[template(path = "tests/text/mixed_run.html")]
    struct MixedRun {
        name: String,
        count: u32,
    }
    impl Model for MixedRun {
        type Message = String;
        fn update(&mut self, _msg: String, _context: &mut Context) {}
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
        fn update(&mut self, _msg: String, _context: &mut Context) {}
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
        fn update(&mut self, _msg: String, _context: &mut Context) {}
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
                fn update(&mut self, _msg: String, _context: &mut Context) {}
            }
        };
    }
    gap_model!(TextWrap, "tests/gaps/text_wrap.html");
    gap_model!(Placeholder, "tests/gaps/placeholder.html");
    gap_model!(ImgShadow, "tests/gaps/img_shadow.html");
    gap_model!(CanvasResize, "tests/gaps/canvas_resize.html");
    gap_model!(Order, "tests/gaps/order.html");
    gap_model!(InlineWrap, "tests/gaps/inline_wrap.html");
    gap_model!(GposKerning, "tests/gaps/gpos_kerning.html");

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
    #[ignore = "FIXME: Roboto kerns with OpenType GPOS, which fontdue can't read; needs a shaper (harfrust)"]
    fn gpos_kerning_narrows_lines() {
        // Chromium fits "min-" on the first line thanks to kerning.
        assert_eq!(
            crate::roboto_lines(GposKerning, 800.0, 480.0),
            ["A body paragraph in a flex column with min-", "width zero, which wraps."]
        );
    }

    #[test]
    #[ignore = "FIXME: <input placeholder> is not supported"]
    fn empty_input_shows_placeholder() {
        let rt = runtime(Placeholder, 400.0, 300.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        text_rect(&commands, "Type here");
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

    #[test]
    #[ignore = "FIXME: `order` is parsed but not applied in compiled templates"]
    fn flex_order_reorders_items() {
        let rt = runtime(Order, 400.0, 300.0);
        let commands = rt.ui.build_commands(&HashMap::new(), None);
        assert!(text_rect(&commands, "first").x < text_rect(&commands, "second").x);
    }
}
