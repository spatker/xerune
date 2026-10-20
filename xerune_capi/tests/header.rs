//! `include/xerune.h` is what cbindgen makes of `src/lib.rs`. Run with
//! `XERUNE_UPDATE_HEADER=1` to write it after changing the API.

use std::path::Path;

#[test]
fn header_is_up_to_date() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = cbindgen::Config::from_file(dir.join("cbindgen.toml")).unwrap();
    let mut header = Vec::new();
    cbindgen::Builder::new().with_crate(dir).with_config(config).generate().expect("cbindgen").write(&mut header);
    let path = dir.join("include/xerune.h");
    if std::env::var_os("XERUNE_UPDATE_HEADER").is_some() {
        std::fs::write(&path, &header).unwrap();
        return;
    }
    let current = std::fs::read(&path).unwrap_or_default();
    assert!(current == header, "{} is out of date: run XERUNE_UPDATE_HEADER=1 cargo test -p xerune_capi --test header", path.display());
}
