//! Load settings pages from ``data/schema/*.json``.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value as Json};

use crate::util::{capitalize, py_str, title, truthy};

/// Option ``type`` from the schema; unknown names behave like ``String``.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptionType {
    Bool,
    Int,
    Float,
    Enum,
    Color,
    Gradient,
    String,
    Vec2,
    Other(String),
}

impl OptionType {
    pub fn from_name(name: &str) -> Self {
        match name {
            "bool" => Self::Bool,
            "int" => Self::Int,
            "float" => Self::Float,
            "enum" => Self::Enum,
            "color" => Self::Color,
            "gradient" => Self::Gradient,
            "string" => Self::String,
            "vec2" => Self::Vec2,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Bool => "bool",
            Self::Int => "int",
            Self::Float => "float",
            Self::Enum => "enum",
            Self::Color => "color",
            Self::Gradient => "gradient",
            Self::String => "string",
            Self::Vec2 => "vec2",
            Self::Other(n) => n,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// String, number or bool exactly as in the schema.
    pub value: Json,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OptionSpec {
    pub key: String,
    pub label: String,
    pub description: String,
    pub kind: OptionType,
    /// ``null`` when the schema gives none.
    pub default: Json,
    pub advanced: bool,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub digits: Option<usize>,
    pub unit: Option<String>,
    pub choices: Option<Vec<Choice>>,
    /// The option exactly as in the schema file (plus normalized fields).
    pub raw: Map<String, Json>,
}

impl OptionSpec {
    /// A bare option (label derived from the key), handy for tests and ad-hoc values.
    pub fn new(key: &str, kind: OptionType) -> Self {
        OptionSpec {
            key: key.to_string(),
            label: default_label(key),
            description: String::new(),
            kind,
            default: Json::Null,
            advanced: false,
            min: None,
            max: None,
            step: None,
            digits: None,
            unit: None,
            choices: None,
            raw: Map::new(),
        }
    }

    /// Build from a JSON option object (None when it has no string ``key``).
    pub fn from_json(opt: &Json) -> Option<Self> {
        let obj = opt.as_object()?;
        let key = obj.get("key")?.as_str()?;
        let text = |name: &str, fallback: String| match obj.get(name) {
            Some(Json::String(s)) => s.clone(),
            Some(Json::Null) | None => fallback,
            Some(other) => py_str(other),
        };
        let kind = match obj.get("type") {
            Some(Json::String(s)) => OptionType::from_name(s),
            _ => OptionType::String,
        };
        let num = |name: &str| obj.get(name).and_then(Json::as_f64);
        let choices = obj.get("choices").map(|c| {
            c.as_array()
                .map(|items| {
                    items
                        .iter()
                        .map(|c| match c {
                            Json::Object(m) => Choice {
                                value: m.get("value").cloned().unwrap_or(Json::Null),
                                label: match m.get("label") {
                                    Some(Json::String(s)) => s.clone(),
                                    _ => py_str(m.get("value").unwrap_or(&Json::Null)),
                                },
                            },
                            other => Choice { value: other.clone(), label: py_str(other) },
                        })
                        .collect::<Vec<Choice>>()
                })
                .unwrap_or_default()
        });
        let mut raw = obj.clone();
        let label = text("label", default_label(key));
        let description = text("description", String::new());
        let advanced = obj.get("advanced").is_some_and(truthy);
        raw.insert("label".into(), Json::String(label.clone()));
        raw.insert("description".into(), Json::String(description.clone()));
        raw.entry("type").or_insert(Json::String("string".into()));
        raw.entry("default").or_insert(Json::Null);
        raw.insert("advanced".into(), Json::Bool(advanced));
        if let Some(ch) = &choices {
            raw.insert(
                "choices".into(),
                Json::Array(
                    ch.iter()
                        .map(|c| serde_json::json!({"value": c.value.clone(), "label": c.label.clone()}))
                        .collect(),
                ),
            );
        }
        Some(OptionSpec {
            key: key.to_string(),
            label,
            description,
            kind,
            default: obj.get("default").cloned().unwrap_or(Json::Null),
            advanced,
            min: num("min"),
            max: num("max"),
            step: num("step"),
            digits: obj.get("digits").and_then(Json::as_u64).map(|d| d as usize),
            unit: obj.get("unit").and_then(Json::as_str).map(str::to_string),
            choices,
            raw,
        })
    }
}

fn default_label(key: &str) -> String {
    capitalize(&key.rsplit(':').next().unwrap_or(key).replace('_', " "))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: String,
    pub description: String,
    pub options: Vec<OptionSpec>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub description: String,
    pub order: f64,
    pub groups: Vec<Group>,
}

fn normalize_page(page: &Json, fallback_id: &str) -> Option<Page> {
    let obj = page.as_object()?;
    let id = match obj.get("id") {
        Some(v) if truthy(v) => py_str(v),
        _ => fallback_id.to_string(),
    };
    let text = |name: &str, fallback: String| match obj.get(name) {
        Some(Json::String(s)) => s.clone(),
        None | Some(Json::Null) => fallback,
        Some(other) => py_str(other),
    };
    let order = match obj.get("order") {
        None => 1000.0,
        Some(Json::Number(n)) => n.as_f64().unwrap_or(1000.0),
        Some(Json::String(s)) => crate::util::parse_float(s).unwrap_or(1000.0),
        Some(Json::Bool(b)) => *b as i64 as f64,
        Some(_) => 1000.0,
    };
    let groups = obj
        .get("groups")
        .and_then(Json::as_array)
        .map(|gs| {
            gs.iter()
                .filter_map(Json::as_object)
                .map(|g| Group {
                    title: match g.get("title") {
                        Some(Json::String(s)) => s.clone(),
                        _ => String::new(),
                    },
                    description: g.get("description").and_then(Json::as_str).unwrap_or("").to_string(),
                    options: g
                        .get("options")
                        .and_then(Json::as_array)
                        .map(|os| os.iter().filter_map(OptionSpec::from_json).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Page {
        title: text("title", title(&id.replace('_', " "))),
        icon: text("icon", "preferences-system-symbolic".into()),
        description: text("description", String::new()),
        id,
        order,
        groups,
    })
}

/// ``data/`` of the installed app or source tree (schema/, presets/).
///
/// ``HYPRLAND_MOD_APP_DATA`` overrides it; otherwise the folders next to the
/// executable are searched, then the source tree this crate was built from.
pub fn app_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HYPRLAND_MOD_APP_DATA") {
        return PathBuf::from(dir);
    }
    if let Ok(exe) = std::env::current_exe() {
        for dir in exe.ancestors().skip(1) {
            for candidate in [dir.join("share/hyprland-mod/data"), dir.join("share/hyprland-mod"), dir.join("data")] {
                if candidate.join("schema").is_dir() {
                    return candidate;
                }
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

pub fn default_schema_dir() -> PathBuf {
    app_data_dir().join("schema")
}

/// All pages sorted by ``order`` (then title). Bad files are skipped.
pub fn load_schema(schema_dir: Option<&Path>) -> Vec<Page> {
    let folder = schema_dir.map(Path::to_path_buf).unwrap_or_else(default_schema_dir);
    let mut pages = Vec::new();
    if !folder.is_dir() {
        return pages;
    }
    for path in crate::parser::glob_sorted(&folder.join("*.json")) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let data: Json = match std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|t| serde_json::from_str(&t).map_err(anyhow::Error::from))
        {
            Ok(d) => d,
            Err(e) => {
                eprintln!("hyprland-mod: skipping schema {name}: {e}");
                continue;
            }
        };
        let stem = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let items = match data {
            Json::Array(items) => items,
            other => vec![other],
        };
        let single = items.len() == 1;
        for (i, item) in items.iter().enumerate() {
            let fallback = if single { stem.clone() } else { format!("{stem}-{i}") };
            pages.extend(normalize_page(item, &fallback));
        }
    }
    pages.sort_by(|a, b| a.order.total_cmp(&b.order).then_with(|| a.title.cmp(&b.title)));
    pages
}

/// Every option by key.
pub fn options_by_key(pages: &[Page]) -> indexmap::IndexMap<String, OptionSpec> {
    let mut out = indexmap::IndexMap::new();
    for p in pages {
        for g in &p.groups {
            for o in &g.options {
                out.insert(o.key.clone(), o.clone());
            }
        }
    }
    out
}
