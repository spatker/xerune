//! Models read from JSON (the C API's): templates see them as they see a
//! Rust model's data, with `null` for `None`.

use std::path::{Path, PathBuf};

use xerune_template::{File, Value};

fn render(source: &str, json: &str) -> Result<String, String> {
    let file = File { name: "<test>".to_string(), dir: PathBuf::new(), content: source.to_string() };
    let template = xerune_template::parse(file, &mut |path: &str, _: &Path| Err(format!("no include {path}"))).map_err(|e| e.to_string())?;
    let model: Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let html = xerune_template::render_html(&template, &model, &()).map_err(|e| e.to_string())?;
    // Just the text, without the elements' keys.
    let mut text = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => text.push(c),
            _ => {}
        }
    }
    Ok(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[test]
fn numbers_strings_and_lists() {
    let source = "<p>{{ title }}: {% for item in items %}{{ item.name }}={{ item.count * 2 }} {% endfor %}{{ ratio }} {{ items.len() }}</p>";
    let json = r#"{"title": "Inventory", "items": [{"name": "arrows", "count": 12}, {"name": "potions", "count": -1}], "ratio": 0.5}"#;
    assert_eq!(render(source, json).unwrap(), "Inventory: arrows=24 potions=-2 0.5 2");
}

#[test]
fn integers_compare_and_format_like_rust() {
    let source = r#"<p>{% if hp > 0 && hp <= max %}alive{% endif %} {{ "{:03}"|format(hp) }} {{ "{:.1}"|format(speed) }}</p>"#;
    assert_eq!(render(source, r#"{"hp": 7, "max": 100, "speed": 2.25}"#).unwrap(), "alive 007 2.2");
    // Beyond i64: u64.
    assert_eq!(render("<p>{{ big }}</p>", r#"{"big": 18446744073709551615}"#).unwrap(), "18446744073709551615");
}

#[test]
fn null_is_none_and_any_other_value_is_some() {
    let source = "<p>{% if let Some(name) = player %}{{ name }}{% else %}nobody{% endif %} \
                  {% if player.is_some() %}some{% endif %}{% if player.is_none() %}none{% endif %} \
                  {% if player == None %}eq-none{% endif %}</p>";
    assert_eq!(render(source, r#"{"player": "Ada"}"#).unwrap(), "Ada some");
    assert_eq!(render(source, r#"{"player": null}"#).unwrap(), "nobody none eq-none");
    let source = r#"<p>{% if let Some(n) = level %}{{ n + 1 }}{% endif %}{% if level == Some(3) %} three{% endif %}</p>"#;
    assert_eq!(render(source, r#"{"level": 3}"#).unwrap(), "4 three");
    let source = "<p>{% if let None = level %}no level{% endif %}</p>";
    assert_eq!(render(source, r#"{"level": 3}"#).unwrap(), "");
}

#[test]
fn objects_keep_their_order() {
    let source = "<p>{% for (key, value) in stats %}{{ key }}:{{ value }} {% endfor %}</p>";
    assert_eq!(render(source, r#"{"stats": {"str": 3, "dex": 9, "int": 1}}"#).unwrap(), "str:3 dex:9 int:1");
}

#[test]
fn mistakes_are_errors() {
    let err = render("<p>{{ missing }}</p>", "{}").unwrap_err();
    assert!(err.contains("no field `missing`"), "{err}");
    let err = render("<p>{% if count %}x{% endif %}</p>", r#"{"count": 1}"#).unwrap_err();
    assert!(err.contains("must be a bool"), "{err}");
    assert!(render("<p></p>", "{not json").unwrap_err().contains("key must be a string"));
}
