//! The shop of `../ui/shop.html`, compiled into the library: the template
//! becomes Rust code when this crate builds, so a mistake in it (a field
//! the model lacks) fails the build, and nothing is parsed while the
//! program runs. The program uses the C API as with the interpreted
//! template, with `xr_new_shop` / `xr_app_new_shop` (`include/shop.h`)
//! in place of `xr_new` / `xr_app_new` and `xr_load_template`.
//!
//! The model's JSON is read into these structs: their fields are what the
//! template reads (`null` is `None`, a missing `Option` too).

use serde::Deserialize;
use xerune::XeruneTemplate;

#[derive(XeruneTemplate, Deserialize)]
#[template(path = "../ui/shop.html")]
pub struct Shop {
    gold: i64,
    items: Vec<Item>,
    last_purchase: Option<String>,
    owned_share: f64,
}

#[derive(Deserialize)]
pub struct Item {
    id: i64,
    name: String,
    price: i64,
    owned: bool,
}

xerune_capi::compiled_ui!(Shop, ui: xr_new_shop, app: xr_app_new_shop);
