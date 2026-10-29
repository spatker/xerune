//! Rate-limited diagnostics.
//!
//! The UI tree is rebuilt on every view-relevant update, so anything logged
//! while building it would otherwise repeat on every frame.

/// Logs `"{what}: {name}"` at warn level the first time the pair is seen.
pub(crate) fn warn_once(what: &str, name: &str) {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};

    static SEEN: OnceLock<Mutex<HashSet<(String, String)>>> = OnceLock::new();
    let mut seen = SEEN.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    if !seen.insert((what.to_string(), name.to_string())) {
        return;
    }
    log::warn!("{}: {}", what, name);
}
