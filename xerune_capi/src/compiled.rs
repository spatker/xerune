//! Templates compiled into the program: a crate derives a template for a
//! struct of the model's fields, and [`compiled_ui!`] exports constructors
//! for its `XrUi`s and `XrApp`s. The model still comes as JSON (serde reads
//! it into the struct) and the messages still go out as strings, so the
//! program uses the C API as with an interpreted template; only nothing is
//! parsed while it runs.

use std::path::Path;

use serde::de::DeserializeOwned;
use xerune::style::ContainerStyle;
use xerune::ui::{TemplateLayout, UiBuilder};
use xerune::{Context, Model, TextMeasurer};
use xerune_template::Template;

use crate::engine::{JsonModel, Outbox};

/// The model of a compiled template `T`: `T`'s fields from the JSON the
/// program sets, and the view's messages for the program.
pub struct Compiled<T> {
    /// Until the program sets a model, the view is empty.
    data: Option<T>,
    /// Changes with every model, for [`Model::view_fingerprint`].
    generation: u64,
    outbox: Outbox,
}

impl<T> Default for Compiled<T> {
    fn default() -> Self {
        Self { data: None, generation: 0, outbox: Outbox::default() }
    }
}

impl<T: Model<Message = String> + TemplateLayout> Model for Compiled<T> {
    type Message = String;

    fn update(&mut self, msg: String, _context: &mut Context<String>) {
        self.outbox.push(msg);
    }

    /// The view changes only when the program sets the model.
    fn view_fingerprint(&self) -> Option<u64> {
        Some(self.generation)
    }
}

impl<T: Model<Message = String> + TemplateLayout> TemplateLayout for Compiled<T> {
    fn stylesheet(&self) -> &'static str {
        self.data.as_ref().map_or("", T::stylesheet)
    }

    fn build_ui(&self, builder: &mut UiBuilder, measurer: &impl TextMeasurer, default_style: &ContainerStyle, message_validator: &impl Fn(&str) -> bool) -> taffy::NodeId {
        match &self.data {
            Some(data) => data.build_ui(builder, measurer, default_style, message_validator),
            None => builder.create_element("html", &[]),
        }
    }

    fn uses_pointer(&self) -> (bool, bool) {
        self.data.as_ref().map_or((false, false), T::uses_pointer)
    }
}

impl<T: Model<Message = String> + TemplateLayout + DeserializeOwned> JsonModel for Compiled<T> {
    fn set_json(&mut self, json: &str) -> Result<(), String> {
        let data = serde_json::from_str(json).map_err(|e| match e.classify() {
            serde_json::error::Category::Data => format!("the model doesn't fit the template: {e}"),
            _ => format!("the model isn't JSON: {e}"),
        })?;
        self.data = Some(data);
        self.generation += 1;
        Ok(())
    }

    fn outbox(&mut self) -> &mut Outbox {
        &mut self.outbox
    }

    fn set_template(&mut self, _template: Template, _base_dir: &Path) -> Result<(), String> {
        Err("this UI's template is compiled into it".to_string())
    }
}

/// Exports C constructors for a compiled template's UIs: `$ui` makes an
/// `XrUi` (the program draws it, as `xr_new`) and `$app` an `XrApp` (it
/// shows itself, as `xr_app_new`), both with the template loaded. `$model`
/// is a struct with `#[derive(XeruneTemplate, serde::Deserialize)]` whose
/// fields are what the template reads; its messages are strings (this
/// implements `xerune::Model` for it).
///
/// ```ignore
/// #[derive(XeruneTemplate, Deserialize)]
/// #[template(path = "ui/main.html")]
/// pub struct Player { title: String, position: i64, cover: Option<String> }
///
/// xerune_capi::compiled_ui!(Player, ui: xr_new_player, app: xr_app_new_player);
/// ```
///
/// The program declares them next to `xerune.h`:
///
/// ```c
/// XrUi *xr_new_player(const XrFont *fonts, size_t count);
/// XrApp *xr_app_new_player(const XrFont *fonts, size_t count);
/// ```
#[macro_export]
macro_rules! compiled_ui {
    ($model:ty, ui: $ui:ident, app: $app:ident) => {
        impl ::xerune::Model for $model {
            type Message = ::std::string::String;

            // `Compiled` takes the messages; this one never gets any.
            fn update(&mut self, _: ::std::string::String, _: &mut ::xerune::Context<::std::string::String>) {}
        }

        /// A UI showing the compiled template (see `xr_new`). NULL on an
        /// error.
        ///
        /// # Safety
        /// `fonts` must be NULL or point to `count` valid `XrFont`s.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $ui(fonts: *const $crate::XrFont, count: usize) -> *mut $crate::XrUi {
            // SAFETY: as the caller promised.
            unsafe { $crate::new_ui($crate::Compiled::<$model>::default(), fonts, count) }
        }

        /// An app showing the compiled template (see `xr_app_new`). NULL on
        /// an error.
        ///
        /// # Safety
        /// `fonts` must be NULL or point to `count` valid `XrFont`s.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn $app(fonts: *const $crate::XrFont, count: usize) -> *mut $crate::XrApp {
            // SAFETY: as the caller promised.
            unsafe { $crate::new_app($crate::Compiled::<$model>::default, fonts, count) }
        }
    };
}
