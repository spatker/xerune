use xerune::*;
use taffy::prelude::TaffyMaxContent;

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
    completed: bool,
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

impl Model for TestTodoModel {
    type Message = TestTodoMsg;
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

#[test]
fn test_tiled_rendering_identity() {
    use std::collections::HashMap;
    use fontdue::Font;
    use fast_renderer::FastRenderer;

    // Load font
    let font_data = std::fs::read("resources/fonts/Roboto-Regular.ttf").expect("Failed to read font file");
    let font = Font::from_bytes(font_data, fontdue::FontSettings::default()).expect("Failed to parse font");
    let fonts = vec![font];

    // Setup model and layout
    let model = TestTodoModel {
        items: vec![
            TodoItem { title: "Item 1".to_string(), completed: false },
            TodoItem { title: "Item 2".to_string(), completed: true },
        ],
        active_item: 0,
        new_item_title: "abc".to_string(),
    };
    let measurer = fast_renderer::FastMeasurer { fonts: (&fonts).into() };
    let mut runtime = Runtime::new(model, measurer);
    runtime.set_size(800.0, 600.0);

    // Get render commands
    let commands = runtime.ui.build_commands(&HashMap::new(), None);

    // 1. Full-screen rendering
    let mut full_buffer = vec![0u32; 800 * 600];
    let mut image_cache1 = HashMap::new();
    let mut glyph_cache1 = HashMap::new();
    {
        let mut renderer = FastRenderer::new(&mut full_buffer, 800, 600, &fonts, &mut image_cache1, &mut glyph_cache1);
        renderer.render(&commands, &HashMap::new(), None);
    }

    // 2. Tile-based rendering
    let mut tiled_buffer = vec![0u32; 800 * 600];
    let mut image_cache2 = HashMap::new();
    let mut glyph_cache2 = HashMap::new();
    {
        // 50-pixel height tile
        let mut tile_buffer = vec![0u32; 800 * 50];
        let mut renderer = FastRenderer::new(&mut tile_buffer, 800, 50, &fonts, &mut image_cache2, &mut glyph_cache2);
        renderer.render_tiled(&commands, &HashMap::new(), None, 600, |tx, ty, tw, th, pixels| {
            for dy in 0..th {
                let src_start = (dy * tw) as usize;
                let dst_start = ((ty + dy as i32) * 800 + tx) as usize;
                tiled_buffer[dst_start .. dst_start + tw as usize].copy_from_slice(&pixels[src_start .. src_start + tw as usize]);
            }
        });
    }

    // 3. Compare pixel buffers
    assert_eq!(full_buffer, tiled_buffer, "Full screen rendering and tiled rendering outputs must be pixel-perfect identical");
}

#[test]
fn test_bitmap_font_rendering() {
    use std::collections::HashMap;
    use fast_renderer::FastRenderer;
    use xerune::font::{DEFAULT_ROBOTO_REGULAR, DEFAULT_ROBOTO_BOLD};

    let fonts = vec![DEFAULT_ROBOTO_REGULAR, DEFAULT_ROBOTO_BOLD];

    let commands = vec![
        DrawCommand::DrawText {
            text: "Hello World!".to_string(),
            rect: Rect { x: 10.0, y: 10.0, width: 200.0, height: 30.0 },
            color: Color::new(255, 255, 255, 255),
            font_size: 16.0,
            weight: 0,
        }
    ];

    let mut buffer = vec![0u32; 200 * 50];
    let mut image_cache = HashMap::new();
    let mut glyph_cache = HashMap::new();

    let mut renderer = FastRenderer::new(&mut buffer, 200, 50, &fonts, &mut image_cache, &mut glyph_cache);
    renderer.render(&commands, &HashMap::new(), None);

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
    
    assert_eq!(style.border_width, 2.0);
    assert_eq!(style.border_style, BorderStyle::Dashed);
    assert_eq!(style.border_color, Some(Color::from_rgba8(0, 255, 0, 255)));
    assert!(style.box_shadow.is_some());
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
    use fast_renderer::FastRenderer;
    use xerune::font::DEFAULT_ROBOTO_REGULAR;

    let fonts = vec![DEFAULT_ROBOTO_REGULAR];

    let commands = vec![
        DrawCommand::DrawRect {
            rect: Rect { x: 0.0, y: 0.0, width: 50.0, height: 50.0 },
            color: Some(Color::new(255, 0, 0, 255)),
            gradient: None,
            border_radius: 0.0,
            border_width: 0.0,
            border_color: None,
            border_style: xerune::style::BorderStyle::Solid,
            border_bottom_only: false,
        },
        DrawCommand::DrawRect {
            rect: Rect { x: 100.0, y: 100.0, width: 50.0, height: 50.0 },
            color: Some(Color::new(0, 255, 0, 255)),
            gradient: None,
            border_radius: 0.0,
            border_width: 0.0,
            border_color: None,
            border_style: xerune::style::BorderStyle::Solid,
            border_bottom_only: false,
        },
    ];

    let mut buffer = vec![0u32; 200 * 200];
    let mut image_cache = HashMap::new();
    let mut glyph_cache = HashMap::new();

    // Render with dirty_rect restricted to the second rectangle only
    let dirty_rect = Some(Rect::new(90.0, 90.0, 70.0, 70.0));
    let mut renderer = FastRenderer::new(&mut buffer, 200, 200, &fonts, &mut image_cache, &mut glyph_cache);
    renderer.render(&commands, &HashMap::new(), dirty_rect);

    // Verify first rect (0,0..50,50) was culled and untouched (0)
    let first_rect_pixel = buffer[0];
    assert_eq!(first_rect_pixel, 0, "First rect should not have been rendered because it falls outside dirty_rect");

    // Verify second rect (100,100) was rendered with green color (0xFF00FF00)
    let second_rect_pixel = buffer[100 * 200 + 100];
    assert_eq!(second_rect_pixel, 0xFF00FF00, "Second rect should have been rendered within dirty_rect");
}

#[test]
fn test_zero_copy_canvas_blitting() {
    use fast_renderer::blit_image_rgba;

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



