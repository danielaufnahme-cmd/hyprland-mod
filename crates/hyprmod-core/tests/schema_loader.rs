mod common;

use std::fs;

use hyprmod_core::{load_schema, OptionType};
use serde_json::json;

fn write(dir: &std::path::Path, name: &str, data: &str) {
    fs::write(dir.join(name), data).unwrap();
}

#[test]
fn sorted_and_normalized() {
    let d = tempfile::tempdir().unwrap();
    let b = json!({"id": "decoration", "title": "Decoration", "icon": "x", "order": 20, "description": "d",
        "groups": [{"title": "Blur", "options": [
            {"key": "decoration:blur:enabled", "label": "Blur", "type": "bool", "default": true}]}]});
    let a = json!({"id": "general", "title": "General", "order": 10, "groups": [
        {"title": "Gaps", "options": [{"key": "general:gaps_in", "type": "int", "default": 5, "min": 0, "max": 50},
                                      {"label": "no key, dropped"}]}]});
    write(d.path(), "b.json", &b.to_string());
    write(d.path(), "a.json", &a.to_string());
    write(d.path(), "broken.json", "{not json");
    write(d.path(), "notes.txt", "ignored");
    let pages = load_schema(Some(d.path()));
    assert_eq!(pages.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["general", "decoration"]);
    let general = &pages[0];
    assert_eq!(general.icon, "preferences-system-symbolic");
    assert_eq!(general.description, "");
    let opts = &general.groups[0].options;
    assert_eq!(opts.len(), 1);
    assert_eq!(opts[0].label, "Gaps in");
    assert!(!opts[0].advanced);
    assert_eq!(opts[0].max, Some(50.0));
    assert_eq!(opts[0].kind, OptionType::Int);
    assert_eq!(pages[1].groups[0].options[0].default, json!(true));
}

#[test]
fn missing_dir_and_fields() {
    let d = tempfile::tempdir().unwrap();
    assert!(load_schema(Some(&d.path().join("nope"))).is_empty());
    write(d.path(), "x.json", "{}");
    let pages = load_schema(Some(d.path()));
    assert_eq!(pages[0].id, "x");
    assert_eq!(pages[0].title, "X");
    assert!(pages[0].groups.is_empty());
    assert_eq!(pages[0].order, 1000.0);
}

#[test]
fn list_files_and_titles() {
    let d = tempfile::tempdir().unwrap();
    write(d.path(), "multi.json", r#"[{"order": 2}, {"id": "my_page", "order": 1}]"#);
    let pages = load_schema(Some(d.path()));
    assert_eq!(pages.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["my_page", "multi-0"]);
    assert_eq!(pages[0].title, "My Page");
}

#[test]
fn choices_normalized() {
    let d = tempfile::tempdir().unwrap();
    let e = json!({"id": "e", "groups": [{"options": [
        {"key": "general:layout", "type": "enum", "choices": ["dwindle", {"value": "master", "label": "M"}]}]}]});
    write(d.path(), "e.json", &e.to_string());
    let opt = &load_schema(Some(d.path()))[0].groups[0].options[0];
    let choices = opt.choices.as_ref().unwrap();
    assert_eq!(
        choices.iter().map(|c| (c.value.clone(), c.label.as_str())).collect::<Vec<_>>(),
        [(json!("dwindle"), "dwindle"), (json!("master"), "M")]
    );
}

#[test]
fn real_schema_loads() {
    let pages = load_schema(Some(&common::app_data().join("schema")));
    assert!(pages.len() >= 10);
    let total: usize = pages.iter().flat_map(|p| &p.groups).map(|g| g.options.len()).sum();
    assert!(total >= 300, "{total}");
    assert!(pages.windows(2).all(|w| w[0].order <= w[1].order));
    // default location (source tree) works too
    assert_eq!(load_schema(None).len(), pages.len());
}
