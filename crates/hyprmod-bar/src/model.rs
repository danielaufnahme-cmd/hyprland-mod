//! Bar design model: validation, presets, save/load and ``.hyprmod-bar`` sharing.
//!
//! A bar design is plain data ([`Bar`]). Everything coming from a file goes
//! through [`validate_bar`] before it is used, and the generator validates again,
//! so colors, fonts and numbers that reach style.css can never smuggle in CSS.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use hyprmod_core::fsutil::atomic_write;
use hyprmod_core::pyjson::{dumps, Dumps};
use hyprmod_core::util::{py_str, strip, truthy};
use regex::Regex;
use serde_json::{json, Map, Value as Json};

use crate::modules::{self, merged, module, option_spec, Kind, Opts};

pub const BAR_FORMAT: &str = "hyprland-mod-bar";
pub const FORMAT_VERSION: i64 = 1;
pub const BAR_EXT: &str = ".hyprmod-bar";
pub const MAX_FILE_BYTES: u64 = 256 * 1024;
pub const ZONES: [&str; 3] = ["left", "center", "right"];
pub const ZONE_LABELS: [&str; 3] = ["Left", "Center", "Right"];
pub const POSITIONS: &[&str] = &["top", "bottom"];
pub const STYLES: &[&str] = &["solid", "islands", "pills"];
pub const WEIGHTS: &[&str] = &["normal", "medium", "bold"];
pub const MAX_MODULES: usize = 24;

static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").expect("regex"));
// A font family name: letters, digits, spaces and a few separators — no quotes,
// semicolons, braces or backslashes, so it can't break out of the CSS string.
static FONT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9][A-Za-z0-9 _.+\-]{0,63}$").expect("regex"));
static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,47}$").expect("regex"));
static SAFE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\x00-\x1f\x7f]*$").expect("regex"));
static SLUG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").expect("regex"));

/// Python ``re.match(r"^...$", s)``: ``$`` also matches before one final newline.
fn py_full_match(re: &Regex, s: &str) -> bool {
    re.is_match(s) || s.strip_suffix('\n').is_some_and(|t| re.is_match(t))
}

/// A bar design or file was rejected; the message is shown to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarError(pub String);

impl fmt::Display for BarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BarError {}

pub type Result<T> = std::result::Result<T, BarError>;

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(BarError(msg.into()))
}

/// Index of a zone name in [`ZONES`].
pub fn zone_index(zone: &str) -> Option<usize> {
    ZONES.iter().position(|z| *z == zone)
}

#[derive(Clone, Copy)]
enum FieldKind {
    Choice(&'static [&'static str]),
    Bool,
    Int(i64, i64),
    Float(f64, f64),
    Color,
    Font,
}

/// name -> kind, in the order of ``Bar.to_dict()``.
const FIELDS: &[(&str, FieldKind)] = &[
    ("position", FieldKind::Choice(POSITIONS)),
    ("style", FieldKind::Choice(STYLES)),
    ("floating", FieldKind::Bool),
    ("height", FieldKind::Int(20, 72)),
    ("margin", FieldKind::Int(0, 48)),
    ("radius", FieldKind::Int(0, 36)),
    ("spacing", FieldKind::Int(0, 32)),
    ("padding", FieldKind::Int(0, 32)),
    ("border", FieldKind::Int(0, 4)),
    ("font", FieldKind::Font),
    ("font_size", FieldKind::Int(8, 28)),
    ("font_weight", FieldKind::Choice(WEIGHTS)),
    ("background", FieldKind::Color),
    ("foreground", FieldKind::Color),
    ("accent", FieldKind::Color),
    ("opacity", FieldKind::Float(0.0, 1.0)),
    ("icons", FieldKind::Bool),
];

/// Names of the design settings, in file order.
pub fn field_names() -> impl Iterator<Item = &'static str> {
    FIELDS.iter().map(|(n, _)| *n)
}

/// One module on the bar.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub kind: String,
    pub options: Opts,
}

impl Entry {
    pub fn new(kind: &str) -> Self {
        Entry { kind: kind.to_string(), options: Opts::new() }
    }
}

#[derive(Clone, Debug)]
pub struct Bar {
    pub position: String,
    pub style: String,
    pub floating: bool,
    pub height: i64,
    pub margin: i64,
    pub radius: i64,
    pub spacing: i64,
    pub padding: i64,
    pub border: i64,
    pub font: String,
    pub font_size: i64,
    pub font_weight: String,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub opacity: f64,
    pub icons: bool,
    /// Modules per zone, indexed like [`ZONES`].
    pub modules: [Vec<Entry>; 3],
}

fn default_modules() -> [Vec<Entry>; 3] {
    [
        vec![Entry::new("workspaces")],
        vec![Entry::new("clock")],
        vec![Entry::new("pulseaudio"), Entry::new("network"), Entry::new("battery"), Entry::new("tray")],
    ]
}

impl Default for Bar {
    fn default() -> Self {
        Bar {
            position: "top".into(),
            style: "solid".into(),
            floating: false,
            height: 32,
            margin: 8,
            radius: 12,
            spacing: 4,
            padding: 10,
            border: 0,
            font: "Adwaita Sans".into(),
            font_size: 13,
            font_weight: "medium".into(),
            background: "#16181d".into(),
            foreground: "#e8e8ea".into(),
            accent: "#3584e4".into(),
            opacity: 0.9,
            icons: true,
            modules: default_modules(),
        }
    }
}

impl PartialEq for Bar {
    fn eq(&self, other: &Self) -> bool {
        self.to_dict() == other.to_dict()
    }
}

impl Bar {
    /// An empty bar (no modules) with default settings.
    pub fn empty() -> Self {
        Bar { modules: Default::default(), ..Default::default() }
    }

    /// The value of one design setting as JSON.
    pub fn get(&self, name: &str) -> Option<Json> {
        Some(match name {
            "position" => json!(self.position),
            "style" => json!(self.style),
            "floating" => json!(self.floating),
            "height" => json!(self.height),
            "margin" => json!(self.margin),
            "radius" => json!(self.radius),
            "spacing" => json!(self.spacing),
            "padding" => json!(self.padding),
            "border" => json!(self.border),
            "font" => json!(self.font),
            "font_size" => json!(self.font_size),
            "font_weight" => json!(self.font_weight),
            "background" => json!(self.background),
            "foreground" => json!(self.foreground),
            "accent" => json!(self.accent),
            "opacity" => json!(self.opacity),
            "icons" => json!(self.icons),
            _ => return None,
        })
    }

    fn put(&mut self, name: &str, v: Json) {
        let s = || v.as_str().unwrap_or_default().to_string();
        let i = || v.as_i64().unwrap_or_default();
        match name {
            "position" => self.position = s(),
            "style" => self.style = s(),
            "floating" => self.floating = v.as_bool().unwrap_or_default(),
            "height" => self.height = i(),
            "margin" => self.margin = i(),
            "radius" => self.radius = i(),
            "spacing" => self.spacing = i(),
            "padding" => self.padding = i(),
            "border" => self.border = i(),
            "font" => self.font = s(),
            "font_size" => self.font_size = i(),
            "font_weight" => self.font_weight = s(),
            "background" => self.background = s(),
            "foreground" => self.foreground = s(),
            "accent" => self.accent = s(),
            "opacity" => self.opacity = v.as_f64().unwrap_or_default(),
            "icons" => self.icons = v.as_bool().unwrap_or_default(),
            _ => {}
        }
    }

    pub fn to_dict(&self) -> Json {
        let mut d = Map::new();
        for (name, _) in FIELDS {
            d.insert(name.to_string(), self.get(name).unwrap_or(Json::Null));
        }
        let mut mods = Map::new();
        for (z, zone) in ZONES.iter().enumerate() {
            let list: Vec<Json> =
                self.modules[z].iter().map(|m| json!({"type": m.kind, "options": m.options.clone()})).collect();
            mods.insert(zone.to_string(), Json::Array(list));
        }
        d.insert("modules".into(), Json::Object(mods));
        Json::Object(d)
    }

    // -- module editing (BarError on nonsense, never leaves a half edit) --

    pub fn types(&self) -> Vec<&str> {
        self.modules.iter().flatten().map(|m| m.kind.as_str()).collect()
    }

    /// ``(zone index, position)`` of a module on the bar.
    pub fn find(&self, mtype: &str) -> Option<(usize, usize)> {
        for (z, list) in self.modules.iter().enumerate() {
            if let Some(i) = list.iter().position(|m| m.kind == mtype) {
                return Some((z, i));
            }
        }
        None
    }

    fn label(mtype: &str) -> &str {
        module(mtype).map(|m| m.label).unwrap_or(mtype)
    }

    pub fn add_module(&mut self, mtype: &str, zone: &str, index: Option<usize>, options: Option<&Opts>) -> Result<()> {
        let Some(spec) = module(mtype) else {
            return err(format!("Unknown module “{mtype}”."));
        };
        let Some(z) = zone_index(zone) else {
            return err(format!("Unknown place “{zone}”."));
        };
        if self.find(mtype).is_some() {
            return err(format!("{} is already on the bar.", spec.label));
        }
        if self.types().len() >= MAX_MODULES {
            return err("That's a lot of modules — remove one first.");
        }
        let opts = validate_options(mtype, &Json::Object(options.cloned().unwrap_or_default()))?;
        let list = &mut self.modules[z];
        let index = index.unwrap_or(list.len()).min(list.len());
        list.insert(index, Entry { kind: mtype.to_string(), options: opts });
        Ok(())
    }

    pub fn remove_module(&mut self, mtype: &str) -> Result<Entry> {
        let Some((z, i)) = self.find(mtype) else {
            return err(format!("{} isn't on the bar.", Self::label(mtype)));
        };
        Ok(self.modules[z].remove(i))
    }

    /// Move to ``zone`` so that it ends up at ``index`` there (after removal).
    pub fn move_module(&mut self, mtype: &str, zone: &str, index: usize) -> Result<()> {
        let Some(z) = zone_index(zone) else {
            return err(format!("Unknown place “{zone}”."));
        };
        let entry = self.remove_module(mtype)?;
        let list = &mut self.modules[z];
        list.insert(index.min(list.len()), entry);
        Ok(())
    }

    /// Move one step left/right inside its zone; false at the edge.
    pub fn shift_module(&mut self, mtype: &str, delta: i64) -> bool {
        let Some((z, i)) = self.find(mtype) else {
            return false;
        };
        let j = i as i64 + delta;
        let list = &mut self.modules[z];
        if j < 0 || j >= list.len() as i64 {
            return false;
        }
        list.swap(i, j as usize);
        true
    }

    pub fn set_option(&mut self, mtype: &str, key: &str, value: Json) -> Result<()> {
        let Some((z, i)) = self.find(mtype) else {
            return err(format!("{} isn't on the bar.", Self::label(mtype)));
        };
        let entry = &mut self.modules[z][i];
        let mut merged_opts = entry.options.clone();
        merged_opts.insert(key.to_string(), value);
        entry.options = validate_options(mtype, &Json::Object(merged_opts))?;
        Ok(())
    }

    /// Options of a module with defaults filled in.
    pub fn options(&self, mtype: &str) -> Opts {
        let stored = self.find(mtype).map(|(z, i)| self.modules[z][i].options.clone()).unwrap_or_default();
        merged(mtype, &stored)
    }

    /// Change one design setting (validated).
    pub fn set(&mut self, name: &str, value: &Json) -> Result<()> {
        let v = check_field(name, value)?;
        self.put(name, v);
        Ok(())
    }
}

// ---------------------------------------------------------------- validation

/// A whole number from JSON (``5`` or ``5.0``, never ``true``).
fn whole(value: &Json) -> Option<f64> {
    match value {
        Json::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(i as f64)
            } else {
                n.as_f64().filter(|f| f.is_finite() && f.fract() == 0.0)
            }
        }
        _ => None,
    }
}

/// Python ``round(x, 2)`` (correctly rounded, ties to even).
fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().unwrap_or(x)
}

fn check_field(name: &str, value: &Json) -> Result<Json> {
    let Some((_, kind)) = FIELDS.iter().find(|(n, _)| *n == name) else {
        return err(format!("Unknown setting “{name}”."));
    };
    match *kind {
        FieldKind::Bool => match value {
            Json::Bool(_) => Ok(value.clone()),
            _ => err(format!("“{name}” must be on or off.")),
        },
        FieldKind::Int(lo, hi) => {
            let Some(v) = whole(value) else {
                return err(format!("“{name}” must be a whole number."));
            };
            if !(lo as f64 <= v && v <= hi as f64) {
                return err(format!("“{name}” must be between {lo} and {hi}."));
            }
            Ok(json!(v as i64))
        }
        FieldKind::Float(lo, hi) => {
            let Some(v) = value.as_f64().filter(|_| value.is_number()) else {
                return err(format!("“{name}” must be a number."));
            };
            if !(lo <= v && v <= hi) {
                return err(format!("“{name}” must be between {lo:?} and {hi:?}."));
            }
            Ok(json!(round2(v)))
        }
        FieldKind::Choice(choices) => match value.as_str() {
            Some(s) if choices.contains(&s) => Ok(value.clone()),
            _ => err(format!("“{name}” must be one of: {}.", choices.join(", "))),
        },
        FieldKind::Color => match value.as_str() {
            Some(s) if HEX_RE.is_match(s) => Ok(json!(s.to_lowercase())),
            _ => err(format!("“{name}” must be a color like #3584e4.")),
        },
        FieldKind::Font => match value.as_str() {
            Some(s) if FONT_RE.is_match(strip(s)) => Ok(json!(s.split_whitespace().collect::<Vec<_>>().join(" "))),
            _ => err("The font name can only use letters, numbers, spaces and - _ . +"),
        },
    }
}

/// Check a module's options; returns them with whole numbers normalized.
pub fn validate_options(mtype: &str, options: &Json) -> Result<Opts> {
    let label = module(mtype).map(|m| m.label).unwrap_or(mtype);
    let Json::Object(options) = options else {
        return err(format!("Options for {label} are damaged."));
    };
    let mut out = Opts::new();
    for (key, value) in options {
        let Some(spec) = option_spec(mtype, key) else {
            return err(format!("{label} has no option “{key}”."));
        };
        let value = match spec.kind {
            Kind::Bool => {
                if !value.is_boolean() {
                    return err(format!("{label}: “{}” must be on or off.", spec.label));
                }
                value.clone()
            }
            Kind::Int { min, max } => {
                let Some(v) = whole(value) else {
                    return err(format!("{label}: “{}” must be a whole number.", spec.label));
                };
                let v = v as i64;
                if !(min <= v && v <= max) {
                    return err(format!("{label}: “{}” must be between {min} and {max}.", spec.label));
                }
                json!(v)
            }
            Kind::Choice(choices) => {
                if !value.as_str().is_some_and(|s| choices.iter().any(|(c, _)| *c == s)) {
                    return err(format!("{label}: “{}” has an unknown value.", spec.label));
                }
                value.clone()
            }
        };
        out.insert(key.clone(), value);
    }
    if mtype == "battery" {
        let o = merged(mtype, &out);
        if modules::opt_int(&o, "critical") >= modules::opt_int(&o, "warning") {
            return err("Battery: “Critical below” must be lower than “Warning below”.");
        }
    }
    Ok(out)
}

/// A checked [`Bar`] from plain data; missing settings get defaults.
pub fn validate_bar(data: &Json) -> Result<Bar> {
    let Json::Object(data) = data else {
        return err("The bar design is damaged.");
    };
    let mut unknown: Vec<&String> =
        data.keys().filter(|k| k.as_str() != "modules" && !FIELDS.iter().any(|(n, _)| n == k)).collect();
    unknown.sort();
    if let Some(first) = unknown.first() {
        return err(format!("Unknown bar setting “{first}”."));
    }
    let mut bar = Bar::empty();
    for (name, _) in FIELDS {
        let default = Bar::default().get(name).unwrap_or(Json::Null);
        let v = check_field(name, data.get(*name).unwrap_or(&default))?;
        bar.put(name, v);
    }
    let raw = data.get("modules").cloned().unwrap_or_else(|| Bar::default().to_dict()["modules"].clone());
    let Json::Object(raw) = raw else {
        return err("The module layout is damaged.");
    };
    if raw.keys().any(|k| zone_index(k).is_none()) {
        return err("The module layout is damaged.");
    }
    let mut seen: Vec<String> = Vec::new();
    for (z, zone) in ZONES.iter().enumerate() {
        let empty = Json::Array(vec![]);
        let Json::Array(list) = raw.get(*zone).unwrap_or(&empty) else {
            return err("The module layout is damaged.");
        };
        for entry in list {
            let entry = match entry {
                Json::String(s) => json!({ "type": s }),
                other => other.clone(),
            };
            let Json::Object(entry) = entry else {
                return err("A module entry is damaged.");
            };
            if entry.keys().any(|k| k != "type" && k != "options") {
                return err("A module entry is damaged.");
            }
            let mtype = entry.get("type").cloned().unwrap_or(Json::Null);
            let Some(spec) = mtype.as_str().and_then(module) else {
                return err(format!("Unknown module “{}”.", py_str(&mtype)));
            };
            if seen.iter().any(|s| s == spec.id) {
                return err(format!("{} appears twice.", spec.label));
            }
            seen.push(spec.id.to_string());
            let options = validate_options(spec.id, entry.get("options").unwrap_or(&json!({})))?;
            bar.modules[z].push(Entry { kind: spec.id.to_string(), options });
        }
    }
    if seen.len() > MAX_MODULES {
        return err("The bar has too many modules.");
    }
    Ok(bar)
}

// ---------------------------------------------------------------- documents / presets

#[derive(Clone, Debug)]
pub struct BarPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub bar: Bar,
    pub tags: Vec<String>,
    pub path: Option<PathBuf>,
}

fn text_field(data: &Map<String, Json>, name: &str, limit: usize, required: bool) -> Result<String> {
    let value = match data.get(name) {
        None | Some(Json::Null) => "",
        Some(Json::String(s)) if py_full_match(&SAFE_RE, s) => s.as_str(),
        Some(_) => return err(format!("The {name} must be plain text.")),
    };
    let value = strip(value);
    if required && value.is_empty() {
        return err(format!("The bar needs a {name}."));
    }
    if value.chars().count() > limit {
        return err(format!("The {name} is too long (at most {limit} characters)."));
    }
    Ok(value.to_string())
}

/// ``re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:48].strip("-") or "bar"``.
pub fn slugify(text: &str) -> String {
    let s = SLUG_RE.replace_all(&text.to_lowercase(), "-").trim_matches('-').to_string();
    let s: String = s.chars().take(48).collect();
    let s = s.trim_matches('-');
    if s.is_empty() {
        "bar".into()
    } else {
        s.to_string()
    }
}

pub fn validate_document(data: &Json) -> Result<BarPreset> {
    let Json::Object(d) = data else {
        return err("This isn't a Hyprland Mod bar file.");
    };
    if d.get("format").and_then(Json::as_str) != Some(BAR_FORMAT) {
        return err("This isn't a Hyprland Mod bar file.");
    }
    let version = match d.get("version") {
        Some(Json::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64().unwrap_or(i64::MAX),
        _ => return err("The bar file has no valid version."),
    };
    if version < 1 {
        return err("The bar file has no valid version.");
    }
    if version > FORMAT_VERSION {
        return err("This bar was made by a newer Hyprland Mod — update the app to open it.");
    }
    let name = text_field(d, "name", 60, true)?;
    let description = text_field(d, "description", 200, false)?;
    let id = match d.get("id").filter(|v| truthy(v)) {
        None => slugify(&name),
        Some(Json::String(s)) if py_full_match(&ID_RE, s) => s.clone(),
        Some(_) => return err("The bar file has an invalid id."),
    };
    let tags = match d.get("tags") {
        None => vec![],
        Some(Json::Array(list)) => {
            let mut tags = Vec::new();
            for t in list {
                match t.as_str() {
                    Some(s) if py_full_match(&ID_RE, s) => tags.push(s.to_string()),
                    _ => return err("The bar file has invalid tags."),
                }
            }
            tags.truncate(6);
            tags
        }
        Some(_) => return err("The bar file has invalid tags."),
    };
    let bar = validate_bar(d.get("bar").unwrap_or(&Json::Null))?;
    Ok(BarPreset { id, name, description, bar, tags, path: None })
}

/// The ``.hyprmod-bar`` document for a design.
pub fn document(bar: &Bar, name: &str, description: &str, tags: &[&str]) -> Result<Json> {
    validate_bar(&bar.to_dict())?;
    Ok(json!({
        "format": BAR_FORMAT,
        "version": FORMAT_VERSION,
        "id": slugify(name),
        "name": name,
        "description": description,
        "tags": tags,
        "bar": bar.to_dict(),
    }))
}

/// Python's ``exc.strerror``: the io error text without Rust's " (os error N)".
pub fn io_message(e: &io::Error) -> String {
    let text = e.to_string();
    match text.find(" (os error ") {
        Some(i) => text[..i].to_string(),
        None => text,
    }
}

fn read_json(path: &Path) -> Result<Json> {
    let meta = fs::metadata(path).map_err(|e| BarError(format!("Couldn't read the file: {}.", io_message(&e))))?;
    if meta.len() > MAX_FILE_BYTES {
        return err("The file is too large to be a bar design.");
    }
    let bytes = fs::read(path).map_err(|e| BarError(format!("Couldn't read the file: {}.", io_message(&e))))?;
    let text = String::from_utf8(bytes).map_err(|_| BarError("The file is damaged or isn't a bar design.".into()))?;
    serde_json::from_str(&text).map_err(|_| BarError("The file is damaged or isn't a bar design.".into()))
}

/// The shipped presets (``data/bars``) next to the installed app or source tree.
pub fn builtin_dir() -> PathBuf {
    hyprmod_core::app_data_dir().join("bars")
}

/// All ``*.json`` presets of a folder, sorted by their ``order`` (then file name).
pub fn load_presets(folder: &Path) -> Result<Vec<BarPreset>> {
    let mut files: Vec<PathBuf> = match fs::read_dir(folder) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json") && !p.file_name().is_some_and(|n| n == ".json"))
            .collect(),
        Err(_) => vec![],
    };
    files.sort();
    let mut out: Vec<(f64, BarPreset)> = Vec::new();
    for path in files {
        let data = read_json(&path)?;
        let mut preset = validate_document(&data)?;
        preset.path = Some(path);
        let order = data.get("order").and_then(Json::as_f64).unwrap_or(999.0);
        out.push((order, preset));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out.into_iter().map(|(_, p)| p).collect())
}

/// Text written by [`export_bar`] (JSON, indent 2, trailing newline).
pub fn export_text(bar: &Bar, name: &str, description: &str) -> Result<String> {
    Ok(dumps(&document(bar, name, description, &[])?, Dumps { indent: Some(2), sort_keys: false, ensure_ascii: false })
        + "\n")
}

pub fn export_bar(bar: &Bar, path: &Path, name: &str, description: &str) -> Result<()> {
    let text = export_text(bar, name, description)?;
    atomic_write(path, &text).map_err(|e| BarError(io_message(&e)))
}

pub fn import_bar(path: &Path) -> Result<BarPreset> {
    let mut preset = validate_document(&read_json(path)?)?;
    preset.path = Some(path.to_path_buf());
    Ok(preset)
}

// ---------------------------------------------------------------- the design being edited

/// ``<data dir>/bar``: the manager's data dir, else ``$HYPRLAND_MOD_DATA``, else
/// ``~/.local/share/hyprland-mod``.
pub fn data_dir(manager_data_dir: Option<&Path>) -> PathBuf {
    let base = match manager_data_dir {
        Some(p) => p.to_path_buf(),
        None => match std::env::var_os("HYPRLAND_MOD_DATA").filter(|v| !v.is_empty()) {
            Some(v) => PathBuf::from(v),
            None => home().join(".local/share/hyprland-mod"),
        },
    };
    base.join("bar")
}

pub(crate) fn home() -> PathBuf {
    std::env::var_os("HOME").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Save the design being edited to ``<folder>/current.json``.
pub fn save_current(bar: &Bar, folder: &Path) -> io::Result<PathBuf> {
    let path = folder.join("current.json");
    let text = dumps(&bar.to_dict(), Dumps { indent: Some(2), sort_keys: false, ensure_ascii: false }) + "\n";
    atomic_write(&path, &text)?;
    Ok(path)
}

/// The design saved last time, or None (missing or damaged file).
pub fn load_current(folder: &Path) -> Option<Bar> {
    let path = folder.join("current.json");
    if !path.exists() {
        return None;
    }
    read_json(&path).and_then(|d| validate_bar(&d)).ok()
}
