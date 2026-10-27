//! Settings schemas loader and definitions.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Choice {
    pub value: serde_json::Value,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OptionItem {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_type")]
    pub r#type: String,
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default)]
    pub choices: Option<Vec<Choice>>,
}

fn default_type() -> String {
    "string".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Group {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub options: Vec<OptionItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Page {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_order")]
    pub order: f64,
    #[serde(default)]
    pub groups: Vec<Group>,
}

fn default_icon() -> String {
    "preferences-system-symbolic".to_string()
}

fn default_order() -> f64 {
    1000.0
}

fn normalize_option(mut opt: OptionItem) -> OptionItem {
    if opt.label.is_empty() {
        let leaf = opt.key.rsplit(':').next().unwrap_or(&opt.key);
        let mut capitalized = leaf.replace('_', " ");
        if let Some(first) = capitalized.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        opt.label = capitalized;
    }
    opt
}

fn normalize_page(mut page: Page, fallback_id: &str) -> Page {
    if page.id.is_empty() {
        page.id = fallback_id.to_string();
    }
    if page.title.is_empty() {
        let mut t = page.id.replace('_', " ");
        if let Some(first) = t.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        page.title = t;
    }
    for g in &mut page.groups {
        g.options = g.options.drain(..).map(normalize_option).collect();
    }
    page
}

pub fn load_schema<P: AsRef<Path>>(schema_dir: Option<P>) -> Vec<Page> {
    let mut pages = Vec::new();

    if let Some(dir) = schema_dir {
        let p = dir.as_ref();
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(p) {
                let mut paths: Vec<PathBuf> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
                    .collect();
                paths.sort();

                for path in paths {
                    if let Ok(text) = fs::read_to_string(&path) {
                        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
                        if let Ok(page) = serde_json::from_str::<Page>(&text) {
                            pages.push(normalize_page(page, &stem));
                        } else if let Ok(mut page_list) = serde_json::from_str::<Vec<Page>>(&text) {
                            for (i, page) in page_list.drain(..).enumerate() {
                                pages.push(normalize_page(page, &format!("{}-{}", stem, i)));
                            }
                        }
                    }
                }
            }
        }
    }

    if pages.is_empty() {
        // Fallback to embedded schema definitions
        pages = load_embedded_schemas();
    }

    pages.sort_by(|a, b| {
        a.order
            .partial_cmp(&b.order)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.title.cmp(&b.title))
    });
    pages
}

fn load_embedded_schemas() -> Vec<Page> {
    let schemas: &[(&str, &str)] = &[
        ("general", include_str!("../../data/schema/general.json")),
        ("decoration", include_str!("../../data/schema/decoration.json")),
        ("animations", include_str!("../../data/schema/animations.json")),
        ("input", include_str!("../../data/schema/input.json")),
        ("gestures", include_str!("../../data/schema/gestures.json")),
        ("cursor", include_str!("../../data/schema/cursor.json")),
        ("layouts", include_str!("../../data/schema/layouts.json")),
        ("groups", include_str!("../../data/schema/groups.json")),
        ("misc", include_str!("../../data/schema/misc.json")),
        ("binds", include_str!("../../data/schema/binds.json")),
        ("system", include_str!("../../data/schema/system.json")),
        ("developer", include_str!("../../data/schema/developer.json")),
    ];

    let mut pages = Vec::new();
    for (name, content) in schemas {
        if let Ok(page) = serde_json::from_str::<Page>(content) {
            pages.push(normalize_page(page, name));
        } else if let Ok(mut page_list) = serde_json::from_str::<Vec<Page>>(content) {
            for (i, page) in page_list.drain(..).enumerate() {
                pages.push(normalize_page(page, &format!("{}-{}", name, i)));
            }
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedded_schema_pages() {
        let pages = load_schema::<&str>(None);
        assert_eq!(pages.len(), 12);
        let ids: Vec<&str> = pages.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"general"));
        assert!(ids.contains(&"decoration"));
        assert!(ids.contains(&"animations"));
    }
}
