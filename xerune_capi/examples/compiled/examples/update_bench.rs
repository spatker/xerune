//! What a model update costs, interpreted vs compiled: each iteration sets
//! the shop's model (the gold changes), ticks and renders the frame.
//!
//!   cargo run --release -p shop_ui --example update_bench
//!
//! Env: UPDATE_BENCH_KIND (interpreted, compiled; default both),
//! UPDATE_BENCH_ITERS (default 200). For ARM instruction counts under
//! qemu-arm, run one kind with ITERS=0 and ITERS=N and subtract.

use std::ffi::CString;
use std::ptr::null;
use std::time::Instant;

use xerune_capi::*;

const WIDTH: u32 = 800;
const HEIGHT: u32 = 480;

fn model(gold: i64) -> CString {
    let items = r#"{"id": 1, "name": "Iron sword", "price": 40, "owned": false}, {"id": 2, "name": "Health potion", "price": 15, "owned": true}, {"id": 3, "name": "Dragon shield", "price": 400, "owned": false}"#;
    CString::new(format!(r#"{{"gold": {gold}, "items": [{items}], "last_purchase": "Health potion", "owned_share": 0.333}}"#)).unwrap()
}

fn run(kind: &str, iters: i64) {
    let ui = unsafe {
        match kind {
            "compiled" => shop_ui::xr_new_shop(null(), 0),
            _ => {
                let ui = xr_new(null(), 0);
                let path = CString::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../ui/shop.html")).unwrap();
                assert_eq!(xr_load_template(ui, path.as_ptr()), 0);
                ui
            }
        }
    };
    let mut pixels = vec![0u32; (WIDTH * HEIGHT) as usize];
    let frame = |ui, gold: i64, pixels: &mut Vec<u32>| unsafe {
        assert_eq!(xr_set_model(ui, model(gold).as_ptr()), 0);
        xr_tick(ui, gold as u64);
        xr_render(ui, pixels.as_mut_ptr(), WIDTH as usize, XR_PIXELS_XRGB8888, std::ptr::null_mut());
    };
    unsafe { xr_set_size(ui, WIDTH, HEIGHT) };
    frame(ui, 100, &mut pixels);
    let start = Instant::now();
    for i in 0..iters {
        frame(ui, 101 + i, &mut pixels);
    }
    if iters > 0 {
        println!("{kind}: {:.1} us per update", start.elapsed().as_secs_f64() * 1e6 / iters as f64);
    }
    unsafe { xr_free(ui) };
}

fn main() {
    let iters = std::env::var("UPDATE_BENCH_ITERS").ok().and_then(|n| n.parse().ok()).unwrap_or(200);
    match std::env::var("UPDATE_BENCH_KIND") {
        Ok(kind) => run(&kind, iters),
        Err(_) => {
            run("interpreted", iters);
            run("compiled", iters);
        }
    }
}
