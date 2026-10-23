//! The C API as a C program uses it: through the `extern "C"` functions
//! and raw pointers only.

use std::ffi::{CStr, CString, c_char};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, MutexGuard};

use xerune_capi::*;

/// FIXME: xerune's viewport is global (`xerune::screen`), so UIs of
/// different sizes must not be used at once, and tests run in parallel.
/// Each test holds this while it runs.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static VIEWPORT: Mutex<()> = Mutex::new(());
    VIEWPORT.lock().unwrap_or_else(|e| e.into_inner())
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(xr_last_error()) }.to_str().unwrap().to_string()
}

fn message(ui: *mut XrUi) -> Option<String> {
    let m = unsafe { xr_next_message(ui) };
    (!m.is_null()).then(|| unsafe { CStr::from_ptr(m) }.to_str().unwrap().to_string())
}

/// A UI of `width` x `height` showing `template` for `model`.
fn ui(template: &str, model: &str, width: u32, height: u32) -> *mut XrUi {
    let ui = unsafe { xr_new(null(), 0) };
    assert!(!ui.is_null(), "{}", last_error());
    unsafe {
        assert_eq!(xr_load_template_source(ui, c(template).as_ptr(), null()), 0, "{}", last_error());
        assert_eq!(xr_set_model(ui, c(model).as_ptr()), 0, "{}", last_error());
        xr_set_size(ui, width, height);
    }
    ui
}

struct Frame {
    pixels: Vec<u32>,
    width: usize,
}

impl Frame {
    fn new(width: u32, height: u32) -> Self {
        Self { pixels: vec![0; (width * height) as usize], width: width as usize }
    }

    /// Renders what changed; the area drawn, if any.
    fn render(&mut self, ui: *mut XrUi) -> Option<XrRect> {
        let mut damage = XrRect::default();
        let drawn = unsafe { xr_render(ui, self.pixels.as_mut_ptr(), self.width, XR_PIXELS_XRGB8888, &mut damage) };
        drawn.then_some(damage)
    }

    fn at(&self, x: usize, y: usize) -> u32 {
        self.pixels[y * self.width + x] & 0xff_ffff
    }
}

const SHOP: &str = r#"<html><head><style>
    body { margin: 0; background: #000000; }
    .item { position: absolute; left: 10px; width: 100px; height: 40px; background: #0000ff; }
    .item.sold { background: #ff0000; }
    p { position: absolute; left: 200px; top: 10px; margin: 0; color: #ffffff; }
</style></head><body>
    {% for item in items %}
    <div class="item{% if item.sold %} sold{% endif %}" style="top: {{ loop.index0 * 50 + 10 }}px" data-on-click="buy:{{ item.id }}"></div>
    {% endfor %}
    <p>{{ gold }} gold</p>
</body></html>"#;

#[test]
fn clicks_send_messages_and_the_model_changes_the_view() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 30, "items": [{"id": 7, "sold": false}, {"id": 9, "sold": false}]}"#, 320, 200);
    let mut frame = Frame::new(320, 200);
    assert_eq!(frame.render(ui), Some(XrRect { x: 0, y: 0, width: 320, height: 200 }), "the first frame draws all");
    assert_eq!(frame.at(50, 80), 0x0000ff);
    assert_eq!(frame.at(150, 80), 0x000000);

    unsafe {
        xr_pointer_down(ui, 50.0, 80.0);
        xr_pointer_up(ui, 50.0, 80.0);
    }
    assert_eq!(message(ui).as_deref(), Some("buy:9"));
    assert_eq!(message(ui), None);
    // The message doesn't change the view; the model does.
    assert_eq!(frame.render(ui), None);
    assert_eq!(unsafe { xr_set_model(ui, c(r#"{"gold": 10, "items": [{"id": 7, "sold": false}, {"id": 9, "sold": true}]}"#).as_ptr()) }, 0);
    let damage = frame.render(ui).expect("the view changed");
    assert_eq!(frame.at(50, 80), 0xff0000);
    assert!(damage.width < 320 || damage.height < 200, "only what changed is drawn: {damage:?}");
    unsafe { xr_free(ui) };
}

#[test]
fn scroll_into_view_shows_the_row() {
    let _viewport = one_at_a_time();
    let list = r#"<html><head><style>
        body { margin: 0; }
        .list { height: 40px; overflow: scroll; }
        .row { height: 20px; }
    </style></head><body><div class="list">
        {% for row in rows %}<div class="row" id="row-{{ row }}" data-on-click="pick:{{ row }}"></div>{% endfor %}
    </div></body></html>"#;
    let ui = ui(list, r#"{"rows": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]}"#, 100, 100);
    let pick = |y: f32| unsafe {
        xr_pointer_down(ui, 10.0, y);
        xr_pointer_up(ui, 10.0, y);
        message(ui)
    };
    assert_eq!(pick(30.0).as_deref(), Some("pick:1"));
    // The row ends at the list's bottom edge.
    assert!(unsafe { xr_scroll_into_view(ui, c("row-9").as_ptr()) });
    assert_eq!(pick(30.0).as_deref(), Some("pick:9"));
    // Up again: the row starts at the top.
    assert!(unsafe { xr_scroll_into_view(ui, c("row-2").as_ptr()) });
    assert_eq!(pick(10.0).as_deref(), Some("pick:2"));
    // No such row: nothing moves.
    unsafe { xr_scroll_into_view(ui, c("row-42").as_ptr()) };
    assert_eq!(pick(10.0).as_deref(), Some("pick:2"));
    unsafe { xr_free(ui) };
}

#[test]
fn the_keyboard_edits_text() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 0, "items": []}"#, 480, 320);
    let mut frame = Frame::new(480, 320);
    frame.render(ui);
    let before = frame.pixels.clone();
    let (title, text) = (c("Name"), c("abc"));
    let request = XrTextRequest { title: title.as_ptr(), text: text.as_ptr(), password: false, min_len: 0, max_len: 0, lang: null() };
    let key = |name: &str| unsafe { xr_key_down(ui, c(name).as_ptr()) };

    assert!(unsafe { xr_edit_text(ui, c("name").as_ptr(), &request) });
    frame.render(ui);
    assert_ne!(frame.pixels, before, "the keyboard shows");
    // Enter is OK while no key has the focus.
    key("Enter");
    assert_eq!(message(ui).as_deref(), Some("name:done:abc"));
    frame.render(ui);
    assert_eq!(frame.pixels, before, "the keyboard is gone");

    // Too short to confirm: Enter does nothing, Escape cancels.
    let request = XrTextRequest { min_len: 8, ..request };
    unsafe { xr_edit_text(ui, c("wifi").as_ptr(), &request) };
    key("Enter");
    assert_eq!(message(ui), None);
    key("Escape");
    assert_eq!(message(ui).as_deref(), Some("wifi:cancel"));
    assert!(!unsafe { xr_edit_text(ui, c("x").as_ptr(), null()) });
    assert!(last_error().contains("NULL"), "{}", last_error());
    unsafe { xr_free(ui) };
}

#[test]
fn a_dragged_pointer_is_no_click() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 0, "items": [{"id": 1, "sold": false}]}"#, 320, 200);
    unsafe {
        xr_pointer_down(ui, 50.0, 30.0);
        xr_pointer_move(ui, 90.0, 30.0);
        xr_pointer_up(ui, 90.0, 30.0);
    }
    assert_eq!(message(ui), None);
    unsafe { xr_free(ui) };
}

#[test]
fn bad_models_are_errors_and_keep_the_view() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 30, "items": []}"#, 320, 200);
    let mut frame = Frame::new(320, 200);
    frame.render(ui);
    unsafe {
        assert_eq!(xr_set_model(ui, c(r#"{"items": []}"#).as_ptr()), -1);
        assert!(last_error().contains("no field `gold`"), "{}", last_error());
        assert_eq!(xr_set_model(ui, c("{\"gold\": ").as_ptr()), -1);
        assert!(last_error().contains("isn't JSON"), "{}", last_error());
        assert_eq!(xr_set_model(ui, c("[1, 2]").as_ptr()), -1);
        assert!(last_error().contains("must be a JSON object"), "{}", last_error());
        assert_eq!(xr_set_model(ui, null()), -1);
        assert!(last_error().contains("NULL"), "{}", last_error());
    }
    assert_eq!(frame.render(ui), None, "still the old view");
    unsafe { xr_free(ui) };
}

#[test]
fn bad_templates_are_errors_and_keep_the_template() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 30, "items": []}"#, 320, 200);
    unsafe {
        assert_eq!(xr_load_template_source(ui, c("<p>{% if %}</p>").as_ptr(), null()), -1);
        assert!(last_error().contains("syntax"), "{}", last_error());
        assert_eq!(xr_load_template_source(ui, c("<p>{{ score }}</p>").as_ptr(), null()), -1);
        assert!(last_error().contains("no field `score`"), "{}", last_error());
        assert_eq!(xr_load_template(ui, c("/no/such/template.html").as_ptr()), -1);
        assert!(last_error().contains("/no/such/template.html"), "{}", last_error());
        assert_eq!(xr_set_model(ui, c(r#"{"gold": 5, "items": []}"#).as_ptr()), 0, "the shop is still shown");
        xr_free(ui);
    }
}

#[test]
fn misuse_is_an_error_not_a_crash() {
    let _viewport = one_at_a_time();
    let mut pixels = vec![0u32; 16];
    unsafe {
        assert!(!xr_render(null_mut(), pixels.as_mut_ptr(), 4, XR_PIXELS_XRGB8888, null_mut()));
        assert!(last_error().contains("NULL"));
        let ui = xr_new(null(), 0);
        assert!(!xr_render(ui, pixels.as_mut_ptr(), 4, XR_PIXELS_XRGB8888, null_mut()));
        assert!(last_error().contains("xr_set_size"), "{}", last_error());
        xr_set_size(ui, 4, 4);
        assert!(!xr_render(ui, pixels.as_mut_ptr(), 2, XR_PIXELS_XRGB8888, null_mut()));
        assert!(last_error().contains("stride"), "{}", last_error());
        assert!(!xr_render(ui, pixels.as_mut_ptr(), 4, 99, null_mut()));
        assert!(last_error().contains("pixel format"), "{}", last_error());
        assert!(xr_render(ui, pixels.as_mut_ptr(), 4, XR_PIXELS_XRGB8888, null_mut()), "no template: an empty page");
        assert!(xr_next_message(ui).is_null());
        assert!(!xr_key_down(ui, null()));
        xr_free(ui);
        xr_free(null_mut());
        let font = XrFont { regular: b"not a font".as_ptr(), regular_len: 10, bold: null(), bold_len: 0 };
        assert!(xr_new(&font, 1).is_null());
        assert!(last_error().contains("font 0"), "{}", last_error());
        assert!(!CStr::from_ptr(xr_version()).to_bytes().is_empty());
    }
}

#[test]
fn both_pixel_formats() {
    let _viewport = one_at_a_time();
    let ui = ui("<style>body { margin: 0; background: #102030; }</style>", "{}", 8, 8);
    let mut pixels = vec![0u32; 64];
    unsafe {
        xr_render(ui, pixels.as_mut_ptr(), 8, XR_PIXELS_XBGR8888, null_mut());
        assert_eq!(pixels[0] & 0xff_ffff, 0x302010);
        xr_repaint_all(ui);
        xr_render(ui, pixels.as_mut_ptr(), 8, XR_PIXELS_XRGB8888, null_mut());
        assert_eq!(pixels[0] & 0xff_ffff, 0x102030);
        xr_free(ui);
    }
}

#[test]
fn rows_can_be_longer_than_the_ui() {
    let _viewport = one_at_a_time();
    let ui = ui("<style>body { margin: 0; background: #ffffff; }</style>", "{}", 4, 2);
    let mut pixels = vec![0u32; 6 * 2];
    unsafe { xr_render(ui, pixels.as_mut_ptr(), 6, XR_PIXELS_XRGB8888, null_mut()) };
    assert_eq!(pixels.iter().map(|p| p & 0xff_ffff).collect::<Vec<_>>(), [0xffffff, 0xffffff, 0xffffff, 0xffffff, 0, 0, 0xffffff, 0xffffff, 0xffffff, 0xffffff, 0, 0]);
    unsafe { xr_free(ui) };
}

#[test]
fn ticks_run_transitions_on_the_callers_clock() {
    let _viewport = one_at_a_time();
    let template = r#"<style>
        body { margin: 0; background: #000000; }
        .light { width: 10px; height: 10px; background: #000000; transition: background-color 1s linear; }
        .light.on { background: #ffffff; }
    </style><div class="light{% if on %} on{% endif %}"></div>"#;
    let ui = ui(template, r#"{"on": false}"#, 10, 10);
    let mut frame = Frame::new(10, 10);
    unsafe { xr_tick(ui, 5_000) };
    frame.render(ui);
    unsafe { xr_set_model(ui, c(r#"{"on": true}"#).as_ptr()) };
    unsafe { xr_tick(ui, 5_500) };
    frame.render(ui);
    let half = frame.at(5, 5) & 0xff;
    assert!((0x60..0xa0).contains(&half), "half way: {half:#x}");
    assert!(unsafe { xr_tick(ui, 6_100) });
    frame.render(ui);
    assert_eq!(frame.at(5, 5), 0xffffff);
    unsafe { xr_free(ui) };
}

#[test]
fn text_inputs_and_keys_send_messages() {
    let _viewport = one_at_a_time();
    let template = r#"<style>body { margin: 0; } input { width: 100px; height: 30px; }</style>
        <input id="name" value="{{ name }}">"#;
    let ui = ui(template, r#"{"name": ""}"#, 200, 100);
    unsafe {
        xr_pointer_down(ui, 20.0, 10.0);
        xr_pointer_up(ui, 20.0, 10.0);
        xr_text_input(ui, c("Ada").as_ptr());
        xr_key_down(ui, c("Enter").as_ptr());
    }
    assert_eq!(message(ui).as_deref(), Some("name:text:Ada"));
    assert_eq!(message(ui).as_deref(), Some("keydown:Enter"));
    unsafe { xr_free(ui) };
}

#[test]
fn hit_tells_the_ui_from_the_bare_page() {
    let _viewport = one_at_a_time();
    let ui = ui(SHOP, r#"{"gold": 0, "items": [{"id": 1, "sold": false}]}"#, 320, 200);
    let mut frame = Frame::new(320, 200);
    frame.render(ui);
    unsafe {
        assert!(xr_hit(ui, 50.0, 30.0), "an item");
        assert!(!xr_hit(ui, 150.0, 150.0), "the page");
        xr_free(ui);
    }
}

#[test]
fn several_uis_have_their_own_viewports() {
    let _viewport = one_at_a_time();
    let template = "<style>body { margin: 0; background: #000000; } @media (min-width: 100px) { body { background: #ffffff; } }</style>";
    let (small, large) = (ui(template, "{}", 50, 50), ui(template, "{}", 200, 50));
    let (mut a, mut b) = (Frame::new(50, 50), Frame::new(200, 50));
    unsafe { xr_set_model(small, c("{}").as_ptr()) };
    a.render(small);
    b.render(large);
    assert_eq!((a.at(0, 0), b.at(0, 0)), (0x000000, 0xffffff));
    unsafe {
        xr_free(small);
        xr_free(large);
    }
}

#[test]
fn templates_from_files_find_includes_and_images_next_to_them() {
    let _viewport = one_at_a_time();
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("capi_files");
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    std::fs::write(dir.join("main.html"), r#"<style>body { margin: 0; } img { width: 4px; height: 4px; }</style>{% include "parts/logo.html" %}"#).unwrap();
    std::fs::write(dir.join("parts/logo.html"), r#"<img src="{{ logo }}">"#).unwrap();
    // A 1x1 red PNG.
    let png = [
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
        0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8, 0xcf,
        0xc0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xdd, 0x8d, 0xb0, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    std::fs::write(dir.join("red.png"), png).unwrap();

    let ui = unsafe { xr_new(null(), 0) };
    let path = c(dir.join("main.html").to_str().unwrap());
    unsafe {
        assert_eq!(xr_set_model(ui, c(r#"{"logo": "red.png"}"#).as_ptr()), 0, "a model before the template");
        assert_eq!(xr_load_template(ui, path.as_ptr()), 0, "{}", last_error());
        xr_set_size(ui, 8, 8);
        assert!(xr_wait_for_images(ui, 5_000));
        xr_tick(ui, 0);
    }
    let mut frame = Frame::new(8, 8);
    frame.render(ui);
    assert_eq!(frame.at(1, 1), 0xff0000);
    unsafe { xr_free(ui) };
}

/// Strings from C are borrowed only during the call.
#[test]
fn arguments_needn_t_outlive_the_call() {
    let _viewport = one_at_a_time();
    let ui = unsafe { xr_new(null(), 0) };
    {
        let source: Vec<c_char> = b"<p>{{ a }}</p>\0".iter().map(|&b| b as c_char).collect();
        let model = c(r#"{"a": 1}"#);
        unsafe {
            assert_eq!(xr_set_model(ui, model.as_ptr()), 0);
            assert_eq!(xr_load_template_source(ui, source.as_ptr(), null()), 0);
        }
    }
    unsafe {
        xr_set_size(ui, 20, 20);
        assert_eq!(xr_set_model(ui, c(r#"{"a": 2}"#).as_ptr()), 0);
        xr_free(ui);
    }
}

#[test]
fn programs_draw_into_canvases() {
    let _viewport = one_at_a_time();
    let template = r#"<style>body { margin: 0; background: #000000; } canvas { position: absolute; left: 4px; top: 4px; }</style>
        <canvas id="map" width="8" height="4"></canvas>"#;
    let ui = ui(template, "{}", 20, 20);
    let mut frame = Frame::new(20, 20);
    frame.render(ui);
    let (mut width, mut height) = (0, 0);
    let pixels = unsafe { xr_canvas(ui, c("map").as_ptr(), &mut width, &mut height) };
    assert!(!pixels.is_null(), "{}", last_error());
    assert_eq!((width, height), (8, 4));
    let pixels = unsafe { std::slice::from_raw_parts_mut(pixels, (width * height * 4) as usize) };
    for (i, pixel) in pixels.chunks_mut(4).enumerate() {
        // The left half red, the right half transparent.
        let alpha = if i % 8 < 4 { 255 } else { 0 };
        pixel.copy_from_slice(&[255, 0, 0, alpha]);
    }
    assert_eq!(frame.render(ui), Some(XrRect { x: 4, y: 4, width: 8, height: 4 }), "the canvas is drawn");
    assert_eq!((frame.at(5, 5), frame.at(10, 5), frame.at(5, 10)), (0xff0000, 0x000000, 0x000000));
    assert_eq!(frame.render(ui), None, "until it is drawn into again");
    unsafe {
        assert!(xr_canvas(ui, c("minimap").as_ptr(), null_mut(), null_mut()).is_null());
        assert!(last_error().contains("minimap"), "{}", last_error());
        xr_free(ui);
    }
}
