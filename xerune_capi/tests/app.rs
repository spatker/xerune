//! `XrApp`s, run headlessly: with `XERUNE_SNAPSHOT` set, an app renders
//! one frame (after `XERUNE_SNAPSHOT_MESSAGES`) and ends.

use std::ffi::{CStr, CString};
use std::path::PathBuf;
use std::ptr::null;
use std::sync::{Mutex, MutexGuard};

use xerune_capi::*;

const TEMPLATE: &str = r#"<html><body style="margin: 0">
<button id="b" style="width: 100px; height: 40px; background: #ff0000" data-on-click="buy:{{ id }}">{{ name }}</button>
</body></html>"#;

/// The snapshot settings are environment variables, and the viewport is
/// global; each test holds this while it runs.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn text(s: *const std::ffi::c_char) -> Option<String> {
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_string())
}

fn last_error() -> String {
    text(xr_last_error()).unwrap()
}

/// Where a test's snapshot goes, with the messages sent before it.
fn snapshot_to(name: &str, messages: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("xr-app-{name}.png"));
    let _ = std::fs::remove_file(&path);
    // SAFETY: the tests that read the environment hold `one_at_a_time`.
    unsafe {
        std::env::set_var("XERUNE_SNAPSHOT", &path);
        std::env::set_var("XERUNE_SNAPSHOT_MESSAGES", messages);
    }
    path
}

fn app(model: &str) -> *mut XrApp {
    let app = unsafe { xr_app_new(null(), 0) };
    assert!(!app.is_null(), "{}", last_error());
    unsafe {
        assert_eq!(xr_app_load_template_source(app, c(TEMPLATE).as_ptr(), null()), 0, "{}", last_error());
        assert_eq!(xr_app_set_model(app, c(model).as_ptr()), 0, "{}", last_error());
    }
    app
}

#[cfg(target_os = "linux")]
fn readable(fd: i32) -> bool {
    let mut pollfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    unsafe { libc::poll(&mut pollfd, 1, 0) == 1 }
}

#[test]
fn app_renders_and_hands_out_messages() {
    let _lock = one_at_a_time();
    let path = snapshot_to("messages", "buy:7;buy:8");
    let app = app(r#"{"id": 7, "name": "Buy"}"#);
    unsafe {
        assert_eq!(xr_app_start(app, null(), 200, 100), 0, "{}", last_error());
        // The snapshot is taken: the app ended.
        assert!(!xr_app_running(app));
        assert!(path.exists(), "no snapshot at {}", path.display());
        let image = std::fs::read(&path).unwrap();
        assert!(image.starts_with(b"\x89PNG"));

        #[cfg(target_os = "linux")]
        let fd = xr_app_message_fd(app);
        #[cfg(target_os = "linux")]
        assert!(readable(fd), "messages wait, so the fd is readable");
        assert_eq!(text(xr_app_next_message(app)).as_deref(), Some("buy:7"));
        assert_eq!(text(xr_app_next_message(app)).as_deref(), Some("buy:8"));
        assert_eq!(text(xr_app_next_message(app)), None);
        assert_eq!(text(xr_app_next_error(app)), None);
        #[cfg(target_os = "linux")]
        assert!(!readable(fd), "everything was taken");

        // Too late now.
        assert_eq!(xr_app_set_model(app, c(r#"{"id": 1, "name": "x"}"#).as_ptr()), -1);
        assert!(last_error().contains("isn't running"), "{}", last_error());
        assert_eq!(xr_app_start(app, null(), 200, 100), -1);
        xr_app_free(app);
    }
}

#[test]
fn a_model_that_doesnt_fit_stops_the_start() {
    let _lock = one_at_a_time();
    snapshot_to("misfit", "");
    // The template reads `id`.
    let app = app(r#"{"name": "Buy"}"#);
    unsafe {
        assert_eq!(xr_app_start(app, null(), 200, 100), -1);
        assert!(last_error().contains("id"), "{}", last_error());
        assert!(!xr_app_running(app));
        xr_app_free(app);
    }
}

#[test]
fn the_latest_model_counts() {
    let _lock = one_at_a_time();
    let path = snapshot_to("latest", "");
    let app = app(r#"{"name": "Buy"}"#);
    unsafe {
        // Only the last model before the start is shown, so the first
        // (which lacks `id`) isn't an error.
        assert_eq!(xr_app_set_model(app, c(r#"{"id": 1, "name": "Buy"}"#).as_ptr()), 0);
        assert_eq!(xr_app_start(app, c("shop").as_ptr(), 200, 100), 0, "{}", last_error());
        assert!(path.exists());
        assert_eq!(text(xr_app_next_error(app)), None);
        xr_app_free(app);
    }
}

#[test]
fn a_null_app_is_an_error() {
    unsafe {
        assert_eq!(xr_app_set_model(std::ptr::null_mut(), c("{}").as_ptr()), -1);
        assert!(text(xr_app_next_message(std::ptr::null_mut())).is_none());
        assert!(!xr_app_running(std::ptr::null_mut()));
        assert_eq!(xr_app_message_fd(std::ptr::null_mut()), -1);
        xr_app_free(std::ptr::null_mut());
    }
}
