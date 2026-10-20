//! The compiled shop draws what the interpreted `../ui/shop.html` draws,
//! pixel for pixel, and sends the same messages, through the C API.

use std::ffi::{CStr, CString};
use std::ptr::null;

use shop_ui::xr_new_shop;
use xerune_capi::*;

const WIDTH: u32 = 640;
const HEIGHT: u32 = 400;

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn last_error() -> String {
    unsafe { CStr::from_ptr(xr_last_error()) }.to_str().unwrap().to_string()
}

fn messages(ui: *mut XrUi) -> Vec<String> {
    std::iter::from_fn(|| {
        let m = unsafe { xr_next_message(ui) };
        (!m.is_null()).then(|| unsafe { CStr::from_ptr(m) }.to_str().unwrap().to_string())
    })
    .collect()
}

fn model(gold: i64, owned: [bool; 3], last_purchase: Option<&str>) -> String {
    let items = [(1, "Iron sword", 40), (2, "Health potion", 15), (3, "Dragon shield", 400)]
        .iter()
        .zip(owned)
        .map(|((id, name, price), owned)| format!(r#"{{"id": {id}, "name": "{name}", "price": {price}, "owned": {owned}}}"#))
        .collect::<Vec<_>>()
        .join(", ");
    let last = last_purchase.map_or("null".to_string(), |p| format!("\"{p}\""));
    let share = owned.iter().filter(|o| **o).count() as f64 / 3.0;
    format!(r#"{{"gold": {gold}, "items": [{items}], "last_purchase": {last}, "owned_share": {share}}}"#)
}

struct Shop {
    ui: *mut XrUi,
    pixels: Vec<u32>,
}

impl Shop {
    fn new(ui: *mut XrUi) -> Self {
        assert!(!ui.is_null(), "{}", last_error());
        unsafe { xr_set_size(ui, WIDTH, HEIGHT) };
        Self { ui, pixels: vec![0; (WIDTH * HEIGHT) as usize] }
    }

    fn set_model(&mut self, json: &str) {
        assert_eq!(unsafe { xr_set_model(self.ui, c(json).as_ptr()) }, 0, "{}", last_error());
    }

    fn frame(&mut self, now_ms: u64) {
        unsafe {
            xr_tick(self.ui, now_ms);
            xr_render(self.ui, self.pixels.as_mut_ptr(), WIDTH as usize, XR_PIXELS_XRGB8888, std::ptr::null_mut());
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        unsafe {
            xr_pointer_move(self.ui, x, y);
            xr_pointer_down(self.ui, x, y);
            xr_pointer_up(self.ui, x, y);
        }
    }
}

impl Drop for Shop {
    fn drop(&mut self) {
        unsafe { xr_free(self.ui) };
    }
}

/// The first pixel where `a` and `b` differ, as (x, y).
fn first_difference(a: &[u32], b: &[u32]) -> Option<(u32, u32)> {
    a.iter().zip(b).position(|(a, b)| a != b).map(|i| (i as u32 % WIDTH, i as u32 / WIDTH))
}

// One test: the viewport is global in xerune, so the two UIs take turns
// on one thread.
#[test]
fn the_compiled_shop_is_the_interpreted_one() {
    let template = c(concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/shop.html"));
    let interpreted = unsafe { xr_new(null(), 0) };
    assert_eq!(unsafe { xr_load_template(interpreted, template.as_ptr()) }, 0, "{}", last_error());
    let mut shops = [Shop::new(interpreted), Shop::new(unsafe { xr_new_shop(null(), 0) })];

    let steps: [(&str, Box<dyn Fn(&mut Shop)>); 4] = [
        ("first frame", Box::new(|shop| shop.set_model(&model(100, [false; 3], None)))),
        // The potion's Buy button; hovering it starts a transition.
        ("hover", Box::new(|shop| unsafe { xr_pointer_move(shop.ui, 573.0, 146.0); })),
        ("click", Box::new(|shop| shop.click(573.0, 146.0))),
        ("bought", Box::new(|shop| shop.set_model(&model(85, [false, true, false], Some("Health potion"))))),
    ];
    let mut now_ms = 0;
    for (name, step) in &steps {
        let mut sent = Vec::new();
        for shop in &mut shops {
            step(shop);
            shop.frame(now_ms);
            sent.push(messages(shop.ui));
        }
        assert_eq!(sent[0], sent[1], "{name}: the messages differ");
        let pixels = &shops[1].pixels;
        assert!(pixels.iter().any(|p| *p != pixels[0]), "{name}: the frame is blank");
        if let Some((x, y)) = first_difference(&shops[0].pixels, pixels) {
            panic!("{name}: the frames differ first at ({x}, {y})");
        }
        if *name == "click" {
            assert_eq!(sent[1], ["buy:2"]);
        }
        now_ms += 80;
    }
}

#[test]
fn a_model_that_doesnt_fit_is_an_error() {
    let ui = unsafe { xr_new_shop(null(), 0) };
    unsafe {
        assert_eq!(xr_set_model(ui, c(r#"{"gold": 1}"#).as_ptr()), -1);
        assert!(last_error().contains("doesn't fit"), "{}", last_error());
        assert_eq!(xr_set_model(ui, c("{").as_ptr()), -1);
        assert!(last_error().contains("isn't JSON"), "{}", last_error());
        assert_eq!(xr_load_template(ui, c("x.html").as_ptr()), -1);
        xr_free(ui);
    }
}
