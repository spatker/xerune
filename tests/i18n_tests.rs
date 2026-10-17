//! The examples' translations: a key missing in a language falls back to
//! English at run time (and logs on every rebuild), so check them here.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use xerune::i18n::Catalog;

/// The catalogs in `examples/<name>/i18n/`, English first.
fn catalogs(dir: &Path) -> Vec<Catalog> {
    let mut files: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort_by_key(|f| (f.file_stem().unwrap() != "en", f.clone()));
    files
        .iter()
        .map(|f| {
            let lang = f.file_stem().unwrap().to_str().unwrap();
            Catalog::parse(lang, &fs::read_to_string(f).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", f.display()))
        })
        .collect()
}

/// The keys of the `self.t("key")` calls in `template`.
fn template_keys(template: &str) -> BTreeSet<String> {
    template.split("self.t(\"").skip(1).map(|rest| rest[..rest.find('"').unwrap()].to_string()).collect()
}

#[test]
fn every_language_has_every_key() {
    let mut checked = 0;
    for example in fs::read_dir("examples").unwrap().map(|e| e.unwrap().path()) {
        let dir = example.join("i18n");
        if !dir.is_dir() {
            continue;
        }
        let catalogs = catalogs(&dir);
        assert_eq!(catalogs[0].lang(), "en", "{}: no en.txt", dir.display());
        let english: BTreeSet<_> = catalogs[0].keys().collect();
        for catalog in &catalogs[1..] {
            let keys: BTreeSet<_> = catalog.keys().collect();
            assert_eq!(
                english.difference(&keys).collect::<Vec<_>>(),
                Vec::<&&str>::new(),
                "{}: missing in {}",
                dir.display(),
                catalog.lang()
            );
            assert_eq!(
                keys.difference(&english).collect::<Vec<_>>(),
                Vec::<&&str>::new(),
                "{}: in {} but not in English",
                dir.display(),
                catalog.lang()
            );
        }
        for template in fs::read_dir(example.join("templates")).unwrap().map(|e| e.unwrap().path()) {
            for key in template_keys(&fs::read_to_string(&template).unwrap()) {
                assert!(english.contains(key.as_str()), "{}: no `{key}` in en.txt", template.display());
            }
        }
        checked += 1;
    }
    assert!(checked >= 2, "the todo and showcase examples have translations");
}
