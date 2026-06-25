use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use glob::glob;
use owo_colors::OwoColorize;
use rayon::prelude::*;
use regex::Regex;
use tiny_skia::Pixmap;

use taffy::prelude::NodeId;
use xerune::{Context, Model, Runtime, Ui, RenderData};
use skia_renderer::{TinySkiaMeasurer, TinySkiaRenderer};

const WIDTH: u32 = 800;
const HEIGHT: u32 = 600;

#[derive(Debug, Clone)]
enum TestRequirement {
    Match(PathBuf),
    Mismatch(PathBuf),
    LayoutAttributes,
}

struct RawHtmlModel {
    html: String,
}

impl Model for RawHtmlModel {
    type Message = String;
    fn view(&self) -> String {
        self.html.clone()
    }
    fn update(&mut self, _msg: Self::Message, _context: &mut Context) {}
}

impl xerune::ui::TemplateLayout for RawHtmlModel {
    fn stylesheet(&self) -> &'static str {
        use html5ever::parse_document;
        use html5ever::tendril::TendrilSink;
        use markup5ever_rcdom::{Handle, NodeData, RcDom};

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut self.html.as_bytes())
            .unwrap();
        
        fn extract_styles_local(handle: &Handle, css_accumulator: &mut String) {
            if let NodeData::Element { name, .. } = &handle.data {
                if name.local.as_ref() == "style" {
                    for child in handle.children.borrow().iter() {
                        if let NodeData::Text { contents } = &child.data {
                            css_accumulator.push_str(&contents.borrow());
                            css_accumulator.push('\n');
                        }
                    }
                }
            }
            for child in handle.children.borrow().iter() {
                extract_styles_local(child, css_accumulator);
            }
        }
        let mut css_str = String::new();
        extract_styles_local(&dom.document, &mut css_str);
        Box::leak(css_str.into_boxed_str())
    }

    fn build_ui(&self, builder: &mut xerune::ui::UiBuilder) -> taffy::NodeId {
        use html5ever::parse_document;
        use html5ever::tendril::TendrilSink;
        use markup5ever_rcdom::{Handle, NodeData, RcDom};

        let dom = parse_document(RcDom::default(), Default::default())
            .from_utf8()
            .read_from(&mut self.html.as_bytes())
            .unwrap();

        fn build_recursive(
            handle: &Handle,
            builder: &mut xerune::ui::UiBuilder,
        ) -> Option<taffy::NodeId> {
            match &handle.data {
                NodeData::Document => {
                    let root = builder.create_element("body", &[]);
                    builder.node_to_handle.insert(root, handle.clone());
                    for child in handle.children.borrow().iter() {
                        if let Some(child_id) = build_recursive(child, builder) {
                            builder.append_child(root, child_id);
                        }
                    }
                    Some(root)
                }
                NodeData::Element { name, attrs, .. } => {
                    let tag = name.local.as_ref();
                    if tag == "style" || tag == "script" || tag == "head" {
                        return None;
                    }
                    
                    let mut tag_attrs = Vec::new();
                    let mut type_attr = None;
                    let mut value_attr = None;
                    let mut checked_attr = None;
                    let mut max_attr = None;
                    let mut src_attr = None;
                    let mut id_attr = None;

                    for attr in attrs.borrow().iter() {
                        let key = attr.name.local.as_ref();
                        let val = attr.value.as_ref();
                        tag_attrs.push((key.to_string(), val.to_string()));
                        match key {
                            "type" => type_attr = Some(val.to_string()),
                            "value" => value_attr = Some(val.to_string()),
                            "checked" => checked_attr = Some(val.to_string()),
                            "max" => max_attr = Some(val.to_string()),
                            "src" => src_attr = Some(val.to_string()),
                            "id" => id_attr = Some(val.to_string()),
                            _ => {}
                        }
                    }

                    let is_input = tag == "input";
                    let node = if is_input && type_attr.as_deref() == Some("checkbox") {
                        let checked = checked_attr.is_some() && checked_attr.as_deref() != Some("false");
                        builder.create_checkbox(checked, &[])
                    } else if is_input && type_attr.as_deref() == Some("range") {
                        let val = value_attr.as_deref().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                        builder.create_slider(val, &[])
                    } else if is_input && (type_attr.as_deref() == Some("text") || type_attr.is_none()) {
                        let val = value_attr.as_deref().unwrap_or("");
                        builder.create_input_text(val, &[])
                    } else if tag == "progress" {
                        let val = value_attr.as_deref().and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
                        let max = max_attr.as_deref().and_then(|m| m.parse::<f32>().ok()).unwrap_or(1.0);
                        builder.create_progress(val, max, &[])
                    } else if tag == "img" {
                        let src = src_attr.as_deref().unwrap_or("");
                        builder.create_image(src, &[])
                    } else if tag == "canvas" {
                        let id_val = id_attr.unwrap_or_default();
                        builder.create_canvas(&id_val, &[])
                    } else {
                        builder.create_element(tag, &[])
                    };

                    builder.node_to_handle.insert(node, handle.clone());
                    if let Some(meta) = builder.node_metadata.get_mut(&node) {
                        meta.attrs = tag_attrs;
                    }

                    for child in handle.children.borrow().iter() {
                        if let Some(child_id) = build_recursive(child, builder) {
                            builder.append_child(node, child_id);
                        }
                    }
                    Some(node)
                }
                NodeData::Text { contents } => {
                    let text = contents.borrow();
                    let trimmed = text.trim();
                    if trimmed.is_empty() {
                        return None;
                    }
                    let node = builder.create_text(&text, &[]);
                    builder.node_to_handle.insert(node, handle.clone());
                    Some(node)
                }
                _ => None,
            }
        }

        build_recursive(&dom.document, builder).unwrap_or_else(|| {
            let root = builder.create_element("body", &[]);
            builder.node_to_handle.insert(root, dom.document.clone());
            root
        })
    }
}

fn load_fonts(wpt_dir: &Path) -> &'static [fontdue::Font] {
    let mut fonts = Vec::new();
    
    // Try to load Ahem font if it exists in WPT (very common for CSS tests)
    let ahem_path = wpt_dir.join("fonts/Ahem.ttf");
    if ahem_path.exists() {
        if let Ok(data) = fs::read(&ahem_path) {
            if let Ok(font) = fontdue::Font::from_bytes(data, fontdue::FontSettings::default()) {
                println!("Loaded Ahem font from WPT");
                // Use Ahem for both regular and bold to guarantee correct layouts in reftests
                fonts.push(font.clone());
                fonts.push(font);
                return Box::leak(fonts.into_boxed_slice());
            }
        }
    }
    
    // Fallback to Roboto
    println!("Ahem font not found, falling back to Roboto");
    let roboto_reg = include_bytes!("../../resources/fonts/Roboto-Regular.ttf");
    if let Ok(font) = fontdue::Font::from_bytes(roboto_reg as &[u8], fontdue::FontSettings::default()) {
        fonts.push(font);
    }
    let roboto_bold = include_bytes!("../../resources/fonts/Roboto-Bold.ttf");
    if let Ok(font) = fontdue::Font::from_bytes(roboto_bold as &[u8], fontdue::FontSettings::default()) {
        fonts.push(font);
    }
    
    Box::leak(fonts.into_boxed_slice())
}

fn resolve_ref_path(test_path: &Path, ref_href: &str) -> PathBuf {
    if ref_href.starts_with('/') {
        PathBuf::from(ref_href.trim_start_matches('/'))
    } else {
        if let Some(parent) = test_path.parent() {
            parent.join(ref_href)
        } else {
            PathBuf::from(ref_href)
        }
    }
}

fn render_html_to_pixmap(html: &str, fonts: &'static [fontdue::Font]) -> Result<Pixmap, String> {
    let mut pixmap = Pixmap::new(WIDTH, HEIGHT).ok_or_else(|| "Failed to create Pixmap".to_string())?;
    pixmap.fill(tiny_skia::Color::WHITE);

    let measurer = TinySkiaMeasurer { fonts };
    let model = RawHtmlModel { html: html.to_string() };
    
    // Use a catch_unwind to handle any potential layout engine panics gracefully
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut runtime = Runtime::new(model, measurer);
        runtime.set_size(WIDTH as f32, HEIGHT as f32);
        
        let mut image_cache = HashMap::new();
        let mut gradient_cache = HashMap::new();
        let mut glyph_cache = HashMap::new();
        
        let mut renderer = TinySkiaRenderer::new(
            pixmap.as_mut(),
            fonts,
            &mut image_cache,
            &mut gradient_cache,
            &mut glyph_cache,
        );
        
        runtime.render(&mut renderer);

        // Debug printing of the layout tree
        println!("=== Layout Tree ===");
        fn print_tree(node_id: NodeId, ui: &Ui, depth: usize) {
            if let Ok(layout) = ui.taffy.layout(node_id) {
                let handle = ui.node_to_handle.get(&node_id);
                let tag = handle.map(|h| {
                    if let markup5ever_rcdom::NodeData::Element { ref name, .. } = h.data {
                        name.local.as_ref().to_string()
                    } else {
                        "text/unknown".to_string()
                    }
                }).unwrap_or_else(|| "none".to_string());
                println!(
                    "{:indent$}- [{}] location={:?} size={:?}",
                    "",
                    tag,
                    layout.location,
                    layout.size,
                    indent = depth * 2
                );
            }
            if let Ok(children) = ui.taffy.children(node_id) {
                for child in children {
                    print_tree(child, ui, depth + 1);
                }
            }
        }
        print_tree(runtime.ui.root, &runtime.ui, 0);
        println!("===================");
    }));

    match res {
        Ok(_) => Ok(pixmap),
        Err(_) => Err("Layout engine panicked".to_string()),
    }
}

fn compare_pixmaps(p1: &Pixmap, p2: &Pixmap, tolerance: f32) -> bool {
    if p1.width() != p2.width() || p1.height() != p2.height() {
        return false;
    }
    let mut diff_pixels = 0;
    let d1 = p1.data();
    let d2 = p2.data();
    for i in (0..d1.len()).step_by(4) {
        let r_diff = (d1[i] as i32 - d2[i] as i32).abs();
        let g_diff = (d1[i+1] as i32 - d2[i+1] as i32).abs();
        let b_diff = (d1[i+2] as i32 - d2[i+2] as i32).abs();
        let a_diff = (d1[i+3] as i32 - d2[i+3] as i32).abs();
        if r_diff + g_diff + b_diff + a_diff > 20 {
            diff_pixels += 1;
        }
    }
    let total_pixels = p1.width() * p1.height();
    let diff_ratio = diff_pixels as f32 / total_pixels as f32;
    diff_ratio <= tolerance
}

fn create_diff_pixmap(p1: &Pixmap, p2: &Pixmap) -> Pixmap {
    let mut diff = Pixmap::new(p1.width(), p1.height()).unwrap();
    let d1 = p1.data();
    let d2 = p2.data();
    let out = diff.data_mut();
    for i in (0..d1.len()).step_by(4) {
        let r_diff = (d1[i] as i32 - d2[i] as i32).abs();
        let g_diff = (d1[i+1] as i32 - d2[i+1] as i32).abs();
        let b_diff = (d1[i+2] as i32 - d2[i+2] as i32).abs();
        let a_diff = (d1[i+3] as i32 - d2[i+3] as i32).abs();
        if r_diff + g_diff + b_diff + a_diff > 20 {
            // Bright red for mismatch
            out[i] = 255;
            out[i+1] = 0;
            out[i+2] = 0;
            out[i+3] = 255;
        } else {
            // Dimmed version of original for matching parts
            out[i] = d1[i] / 5 + 200;
            out[i+1] = d1[i+1] / 5 + 200;
            out[i+2] = d1[i+2] / 5 + 200;
            out[i+3] = 255;
        }
    }
    diff
}

fn is_offset_parent(ui: &Ui, node_id: NodeId) -> bool {
    let handle = match ui.node_to_handle.get(&node_id) {
        Some(h) => h,
        None => return false,
    };
    if let markup5ever_rcdom::NodeData::Element { ref name, .. } = handle.data {
        let tag = name.local.as_ref();
        if tag == "body" || tag == "html" || tag == "table" || tag == "td" || tag == "th" {
            return true;
        }
    }
    if let Some(RenderData::Container(style)) = ui.render_data.get(&node_id) {
        if style.position == xerune::style::Position::Relative || style.position == xerune::style::Position::Absolute {
            return true;
        }
    }
    false
}

fn get_offset_left(ui: &Ui, node_id: NodeId) -> f32 {
    let mut offset = 0.0;
    let mut current = node_id;
    loop {
        let layout = match ui.taffy.layout(current) {
            Ok(l) => l,
            Err(_) => break,
        };
        offset += layout.location.x;
        let parent = match ui.taffy.parent(current) {
            Some(p) => p,
            None => break,
        };
        if is_offset_parent(ui, parent) {
            let handle = ui.node_to_handle.get(&parent);
            let tag = handle.and_then(|h| {
                if let markup5ever_rcdom::NodeData::Element { ref name, .. } = h.data {
                    Some(name.local.as_ref())
                } else {
                    None
                }
            });
            if tag == Some("body") || tag == Some("html") {
                current = parent;
                continue;
            }
            if let Ok(parent_layout) = ui.taffy.layout(parent) {
                offset -= parent_layout.border.left;
            }
            break;
        }
        current = parent;
    }
    offset
}

fn get_offset_top(ui: &Ui, node_id: NodeId) -> f32 {
    let mut offset = 0.0;
    let mut current = node_id;
    loop {
        let layout = match ui.taffy.layout(current) {
            Ok(l) => l,
            Err(_) => break,
        };
        offset += layout.location.y;
        let parent = match ui.taffy.parent(current) {
            Some(p) => p,
            None => break,
        };
        if is_offset_parent(ui, parent) {
            let handle = ui.node_to_handle.get(&parent);
            let tag = handle.and_then(|h| {
                if let markup5ever_rcdom::NodeData::Element { ref name, .. } = h.data {
                    Some(name.local.as_ref())
                } else {
                    None
                }
            });
            if tag == Some("body") || tag == Some("html") {
                current = parent;
                continue;
            }
            if let Ok(parent_layout) = ui.taffy.layout(parent) {
                offset -= parent_layout.border.top;
            }
            break;
        }
        current = parent;
    }
    offset
}

fn check_node_layout(node_id: NodeId, ui: &Ui, errors: &mut Vec<String>) {
    let layout = match ui.taffy.layout(node_id) {
        Ok(l) => l,
        Err(_) => return,
    };
    let handle = match ui.node_to_handle.get(&node_id) {
        Some(h) => h,
        None => return,
    };
    
    if let markup5ever_rcdom::NodeData::Element { ref attrs, .. } = handle.data {
        for attr in attrs.borrow().iter() {
            let name = attr.name.local.as_ref();
            let value = &attr.value;
            
            let result = match name {
                "data-expected-width" => check_attr(name, value, layout.size.width),
                "data-expected-height" => check_attr(name, value, layout.size.height),
                "data-expected-padding-top" => check_attr(name, value, layout.padding.top),
                "data-expected-padding-bottom" => check_attr(name, value, layout.padding.bottom),
                "data-expected-padding-left" => check_attr(name, value, layout.padding.left),
                "data-expected-padding-right" => check_attr(name, value, layout.padding.right),
                "data-expected-margin-top" => check_attr(name, value, layout.margin.top),
                "data-expected-margin-bottom" => check_attr(name, value, layout.margin.bottom),
                "data-expected-margin-left" => check_attr(name, value, layout.margin.left),
                "data-expected-margin-right" => check_attr(name, value, layout.margin.right),
                "data-offset-x" => check_attr(name, value, get_offset_left(ui, node_id)),
                "data-offset-y" => check_attr(name, value, get_offset_top(ui, node_id)),
                _ => Ok(()),
            };
            if let Err(e) = result {
                errors.push(format!("Node (tag: {}): {}", get_tag_name(handle), e));
            }
        }
    }
}

fn get_tag_name(handle: &markup5ever_rcdom::Handle) -> String {
    if let markup5ever_rcdom::NodeData::Element { ref name, .. } = handle.data {
        name.local.as_ref().to_string()
    } else {
        "unknown".to_string()
    }
}

fn check_attr(name: &str, value: &str, actual: f32) -> Result<(), String> {
    let expected: f32 = value.parse().map_err(|_| format!("invalid float: {}", value))?;
    if (actual - expected).abs() < 1.0 {
        Ok(())
    } else {
        Err(format!("{} expected {} got {}", name, expected, actual))
    }
}

fn check_all_nodes(node_id: NodeId, ui: &Ui, errors: &mut Vec<String>) {
    check_node_layout(node_id, ui, errors);
    if let Ok(children) = ui.taffy.children(node_id) {
        for child in children {
            check_all_nodes(child, ui, errors);
        }
    }
}

fn run_attribute_test(html: &str, fonts: &'static [fontdue::Font]) -> Result<Vec<String>, String> {
    let measurer = TinySkiaMeasurer { fonts };
    let model = RawHtmlModel { html: html.to_string() };
    
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut runtime = Runtime::new(model, measurer);
        runtime.set_size(WIDTH as f32, HEIGHT as f32);
        
        println!("=== Layout Tree (Attribute Test) ===");
        fn print_tree(node_id: NodeId, ui: &Ui, depth: usize) {
            if let Ok(layout) = ui.taffy.layout(node_id) {
                let handle = ui.node_to_handle.get(&node_id);
                let tag = handle.map(|h| {
                    if let markup5ever_rcdom::NodeData::Element { ref name, .. } = h.data {
                        name.local.as_ref().to_string()
                    } else {
                        "text/unknown".to_string()
                    }
                }).unwrap_or_else(|| "none".to_string());
                println!(
                    "{:indent$}- [{}] location={:?} size={:?}",
                    "",
                    tag,
                    layout.location,
                    layout.size,
                    indent = depth * 2
                );
            }
            if let Ok(children) = ui.taffy.children(node_id) {
                for child in children {
                    print_tree(child, ui, depth + 1);
                }
            }
        }
        print_tree(runtime.ui.root, &runtime.ui, 0);
        println!("===================");

        let mut errors = Vec::new();
        check_all_nodes(runtime.ui.root, &runtime.ui, &mut errors);
        errors
    }));

    match res {
        Ok(errors) => Ok(errors),
        Err(_) => Err("Layout engine panicked".to_string()),
    }
}

fn inline_stylesheets(html: &str, test_file_path: &Path, wpt_dir: &Path) -> String {
    let link_re = Regex::new(r#"(?i)<link\s+([^>]+)>"#).unwrap();
    let rel_re = Regex::new(r#"(?i)rel\s*=\s*['"]stylesheet['"]"#).unwrap();
    let href_re = Regex::new(r#"(?i)href\s*=\s*['"]([^'"]+)['"]"#).unwrap();
    
    let mut inlined_css = String::new();
    
    for cap in link_re.captures_iter(html) {
        let attrs = &cap[1];
        if rel_re.is_match(attrs) {
            if let Some(href_cap) = href_re.captures(attrs) {
                let href = href_cap.get(1).unwrap().as_str();
                let css_path = if href.starts_with('/') {
                    wpt_dir.join(href.trim_start_matches('/'))
                } else {
                    if let Some(parent) = test_file_path.parent() {
                        parent.join(href)
                    } else {
                        wpt_dir.join(href)
                    }
                };
                if let Ok(css_content) = fs::read_to_string(&css_path) {
                    inlined_css.push_str(&css_content);
                    inlined_css.push('\n');
                }
            }
        }
    }
    
    if !inlined_css.is_empty() {
        format!("{}\n<style>\n{}\n</style>\n", html, inlined_css)
    } else {
        html.to_string()
    }
}

fn filter_path(p: &Path) -> bool {
    let path_str = p.to_string_lossy();
    let is_ref = path_str.ends_with("-ref.html")
        || path_str.ends_with("-ref.htm")
        || path_str.ends_with("-ref.xhtml")
        || path_str.ends_with("-ref.xht")
        || path_str.contains("/reference/");
    let is_support = path_str.contains("/support/");
    let is_dir = p.is_dir();
    !(is_ref || is_support || is_dir)
}

fn main() {
    let wpt_dir_env = env::var("WPT_DIR").unwrap_or_else(|_| "".to_string());
    if wpt_dir_env.is_empty() {
        eprintln!("Error: WPT_DIR environment variable is not set.");
        eprintln!("Please set it to the path of a local copy of WPT (e.g. WPT_DIR=/path/to/wpt)");
        std::process::exit(1);
    }
    let wpt_dir = Path::new(&wpt_dir_env);
    if !wpt_dir.exists() {
        eprintln!("Error: WPT_DIR path '{}' does not exist.", wpt_dir.display());
        std::process::exit(1);
    }

    let mut suite = "css/css-flexbox".to_string();
    let mut filter = String::new();
    
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 {
        suite = args[1].clone();
    }
    if args.len() > 2 {
        filter = args[2].clone();
    }

    println!("Scanning suite: {} in {}", suite, wpt_dir.display());
    let pattern = format!("{}/{}/**/*.htm*", wpt_dir.display(), suite);
    let paths: Vec<PathBuf> = glob(&pattern)
        .expect("Failed to parse glob pattern")
        .filter_map(Result::ok)
        .filter(|p| filter_path(p))
        .filter(|p| filter.is_empty() || p.to_string_lossy().contains(&filter))
        .collect();

    println!("Found {} test files to run.", paths.len());
    
    let out_dir = Path::new("wpt_output");
    if out_dir.exists() {
        let _ = fs::remove_dir_all(out_dir);
    }
    let _ = fs::create_dir_all(out_dir);

    let fonts = load_fonts(wpt_dir);

    let reftest_re = Regex::new(r#"<link\s+rel=['"]?match['"]?\s+href=['"]([^'"]+)['"]"#).unwrap();
    let mismatch_re = Regex::new(r#"<link\s+rel=['"]?mismatch['"]?\s+href=['"]([^'"]+)['"]"#).unwrap();

    let passed = AtomicU32::new(0);
    let failed = AtomicU32::new(0);
    let skipped = AtomicU32::new(0);
    let crashed = AtomicU32::new(0);

    let start_time = Instant::now();

    paths.par_iter().for_each(|path| {
        let relative_path = path.strip_prefix(wpt_dir).unwrap_or(path);
        let mut html_content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };
        html_content = inline_stylesheets(&html_content, path, wpt_dir);

        // Determine test type
        let requirement = if let Some(caps) = reftest_re.captures(&html_content) {
            let ref_href = caps.get(1).unwrap().as_str();
            Some(TestRequirement::Match(resolve_ref_path(relative_path, ref_href)))
        } else if let Some(caps) = mismatch_re.captures(&html_content) {
            let ref_href = caps.get(1).unwrap().as_str();
            Some(TestRequirement::Mismatch(resolve_ref_path(relative_path, ref_href)))
        } else if html_content.contains("data-expected-") || html_content.contains("data-offset-") {
            Some(TestRequirement::LayoutAttributes)
        } else {
            None
        };

        let req = match requirement {
            Some(r) => r,
            None => {
                skipped.fetch_add(1, Ordering::Relaxed);
                return;
            }
        };

        match req {
            TestRequirement::Match(ref_rel_path) => {
                let ref_abs_path = wpt_dir.join(&ref_rel_path);
                let ref_html = match fs::read_to_string(&ref_abs_path) {
                    Ok(c) => c,
                    Err(_) => {
                        println!("{} {} (reference not found: {})", "SKIP".yellow(), relative_path.display(), ref_rel_path.display());
                        skipped.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };
                let ref_html = inline_stylesheets(&ref_html, &ref_abs_path, wpt_dir);

                let test_pixmap = match render_html_to_pixmap(&html_content, fonts) {
                    Ok(p) => p,
                    Err(_) => {
                        println!("{} {}", "CRASH".red(), relative_path.display());
                        crashed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                let ref_pixmap = match render_html_to_pixmap(&ref_html, fonts) {
                    Ok(p) => p,
                    Err(_) => {
                        println!("{} {} (ref crashed)", "CRASH".red(), relative_path.display());
                        crashed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                if compare_pixmaps(&test_pixmap, &ref_pixmap, 0.05) {
                    println!("{} {}", "PASS".green(), relative_path.display());
                    passed.fetch_add(1, Ordering::Relaxed);
                } else {
                    println!("{} {}", "FAIL".red(), relative_path.display());
                    failed.fetch_add(1, Ordering::Relaxed);
                    
                    // Save test, ref, and diff images for visual review
                    let test_name = relative_path.file_stem().unwrap().to_string_lossy();
                    let test_dir = out_dir.join(relative_path.parent().unwrap());
                    let _ = fs::create_dir_all(&test_dir);
                    
                    let _ = test_pixmap.save_png(test_dir.join(format!("{}-test.png", test_name)));
                    let _ = ref_pixmap.save_png(test_dir.join(format!("{}-ref.png", test_name)));
                    let diff_pixmap = create_diff_pixmap(&test_pixmap, &ref_pixmap);
                    let _ = diff_pixmap.save_png(test_dir.join(format!("{}-diff.png", test_name)));
                }
            }
            TestRequirement::Mismatch(ref_rel_path) => {
                let ref_abs_path = wpt_dir.join(&ref_rel_path);
                let ref_html = match fs::read_to_string(&ref_abs_path) {
                    Ok(c) => c,
                    Err(_) => {
                        println!("{} {} (reference not found: {})", "SKIP".yellow(), relative_path.display(), ref_rel_path.display());
                        skipped.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };
                let ref_html = inline_stylesheets(&ref_html, &ref_abs_path, wpt_dir);

                let test_pixmap = match render_html_to_pixmap(&html_content, fonts) {
                    Ok(p) => p,
                    Err(_) => {
                        println!("{} {}", "CRASH".red(), relative_path.display());
                        crashed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                let ref_pixmap = match render_html_to_pixmap(&ref_html, fonts) {
                    Ok(p) => p,
                    Err(_) => {
                        println!("{} {} (ref crashed)", "CRASH".red(), relative_path.display());
                        crashed.fetch_add(1, Ordering::Relaxed);
                        return;
                    }
                };

                // For mismatch, they MUST NOT match
                if !compare_pixmaps(&test_pixmap, &ref_pixmap, 0.05) {
                    println!("{} {}", "PASS".green(), relative_path.display());
                    passed.fetch_add(1, Ordering::Relaxed);
                } else {
                    println!("{} {} (images matched but expected mismatch)", "FAIL".red(), relative_path.display());
                    failed.fetch_add(1, Ordering::Relaxed);
                }
            }
            TestRequirement::LayoutAttributes => {
                match run_attribute_test(&html_content, fonts) {
                    Ok(errors) => {
                        if errors.is_empty() {
                            println!("{} {}", "PASS".green(), relative_path.display());
                            passed.fetch_add(1, Ordering::Relaxed);
                        } else {
                            println!("{} {} - Assertions failed:", "FAIL".red(), relative_path.display());
                            for err in &errors {
                                println!("  - {}", err);
                            }
                            failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    Err(_) => {
                        println!("{} {}", "CRASH".red(), relative_path.display());
                        crashed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
    });

    let elapsed = start_time.elapsed();
    let p = passed.load(Ordering::SeqCst);
    let f = failed.load(Ordering::SeqCst);
    let s = skipped.load(Ordering::SeqCst);
    let c = crashed.load(Ordering::SeqCst);

    println!("\n=== WPT Runner Summary ===");
    println!("Suite: {}", suite);
    println!("Elapsed time: {:.2?}", elapsed);
    println!("Total run: {}", p + f + c);
    println!("  Passed:  {}", p.green());
    println!("  Failed:  {}", f.red());
    println!("  Crashed: {}", c.magenta());
    println!("  Skipped: {}", s.yellow());
}
