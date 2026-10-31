//! Ricing presets and whole-setup files: model, validation, load/save, import/export.
//!
//! A preset is a small JSON document (``.hyprmod-preset`` when shared) holding look
//! settings plus optional ``bezier``/``animation`` lines. Everything coming from a
//! file is validated against the schema before it can reach the ConfigManager:
//! unknown keys, commands (``exec``...) and malformed values are rejected with a
//! message meant for people, not developers.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Map, Value as Json};

use crate::fsutil::atomic_write;
use crate::managed::validate_value;
use crate::manager::ConfigManager;
use crate::pyjson::{dumps, Dumps};
use crate::schema::{app_data_dir, load_schema, options_by_key, OptionSpec, OptionType};
use crate::util::{format_g, parse_float, py_str, realpath, repr_str, split_ws, squash_ws, strip, take_chars, truthy};
use crate::values::{format_value, parse_color, parse_value};

pub const PRESET_FORMAT: &str = "hyprland-mod-preset";
pub const SETUP_FORMAT: &str = "hyprland-mod-setup";
pub const FORMAT_VERSION: i64 = 1;
pub const PRESET_EXT: &str = ".hyprmod-preset";
pub const SETUP_EXT: &str = ".hyprmod";
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Every built-in preset sets all of these, so trying one after another never
/// leaves a stale value behind. "Save current setup" captures the same keys.
pub const LOOK_KEYS: &[&str] = &[
    "general:gaps_in",
    "general:gaps_out",
    "general:border_size",
    "general:col.active_border",
    "general:col.inactive_border",
    "decoration:rounding",
    "decoration:rounding_power",
    "decoration:active_opacity",
    "decoration:inactive_opacity",
    "decoration:blur:enabled",
    "decoration:blur:size",
    "decoration:blur:passes",
    "decoration:blur:vibrancy",
    "decoration:blur:noise",
    "decoration:shadow:enabled",
    "decoration:shadow:range",
    "decoration:shadow:render_power",
    "decoration:shadow:color",
    "decoration:shadow:offset",
    "decoration:shadow:sharp",
    "decoration:dim_inactive",
    "decoration:dim_strength",
    "animations:enabled",
];
/// Presets may only touch the look of things.
pub const PRESET_SECTIONS: &[&str] = &["general:", "decoration:", "animations:", "group:", "dwindle:", "master:"];
pub const PRESET_DENY: &[&str] = &["decoration:screen_shader", "general:locale", "general:layout"];
pub const LIST_KEYWORDS: &[&str] = &["bezier", "animation"];
/// The protected snapshot of the user's own look, taken once before Hyprland Mod changes anything.
pub const ORIGINAL_ID: &str = "my-original-look";
pub const ORIGINAL_NAME: &str = "My original look";
pub const ORIGINAL_DESC: &str = "Your setup from before Hyprland Mod";
pub const ORIGINAL_MARKER: &str = "original-look-captured";

pub const ANIMATION_LEAVES: &[&str] = &[
    "global",
    "windows",
    "windowsIn",
    "windowsOut",
    "windowsMove",
    "layers",
    "layersIn",
    "layersOut",
    "fade",
    "fadeIn",
    "fadeOut",
    "fadeSwitch",
    "fadeShadow",
    "fadeDim",
    "fadeLayers",
    "fadeLayersIn",
    "fadeLayersOut",
    "fadePopups",
    "fadePopupsIn",
    "fadePopupsOut",
    "fadeDpms",
    "border",
    "borderangle",
    "workspaces",
    "workspacesIn",
    "workspacesOut",
    "specialWorkspace",
    "specialWorkspaceIn",
    "specialWorkspaceOut",
    "zoomFactor",
    "monitorAdded",
];

static STYLE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(popin( \d{1,3}%)?|slide( (left|right|top|bottom))?|slidevert|slidefade( \d{1,3}%)?|slidefadevert( \d{1,3}%)?|fade|once|loop|gnomed)$",
    )
    .unwrap()
});
static CURVE_NAME_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z][A-Za-z0-9_]{0,31}$").unwrap());
static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+(\.\d+)?$").unwrap());
static ANGLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d{1,3}(\.\d+)?deg$").unwrap());
static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^#[0-9a-fA-F]{6}$").unwrap());
static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9-]{0,47}$").unwrap());
static SAFE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\x00-\x1f\x7f]*$").unwrap());
static GRADIENT_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"rgba?\([^)]*\)|\S+").unwrap());
static SLUG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").unwrap());

/// A preset or setup file was rejected; the message is shown to the user.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct PresetError(pub String);

type PResult<T> = std::result::Result<T, PresetError>;

fn err<T>(msg: impl Into<String>) -> PResult<T> {
    Err(PresetError(msg.into()))
}

pub type Options = IndexMap<String, OptionSpec>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    pub settings: IndexMap<String, String>,
    pub lists: IndexMap<String, Vec<String>>,
    /// ``{"wallpaper": ["#rrggbb", ...], "window": "#rrggbb"}`` (both optional).
    pub preview: Map<String, Json>,
    pub builtin: bool,
    pub path: Option<PathBuf>,
}

impl Preset {
    /// True for the protected "My original look" snapshot (can't be deleted).
    pub fn original(&self) -> bool {
        !self.builtin && self.id == ORIGINAL_ID
    }

    pub fn to_dict(&self) -> Json {
        let mut data = Map::new();
        data.insert("format".into(), json!(PRESET_FORMAT));
        data.insert("version".into(), json!(FORMAT_VERSION));
        data.insert("id".into(), json!(self.id));
        data.insert("name".into(), json!(self.name));
        data.insert("author".into(), json!(self.author));
        data.insert("description".into(), json!(self.description));
        data.insert("tags".into(), json!(self.tags));
        data.insert("settings".into(), json!(self.settings));
        data.insert("lists".into(), json!(self.lists));
        if !self.preview.is_empty() {
            data.insert("preview".into(), Json::Object(self.preview.clone()));
        }
        Json::Object(data)
    }

    /// True when the ConfigManager currently shows (or has staged) this exact look.
    pub fn matches(&self, cm: &ConfigManager, options: &Options) -> bool {
        for (key, value) in &self.settings {
            let Some(opt) = options.get(key) else { return false };
            if canon(opt, cm.get(key).as_deref()) != canon(opt, Some(value)) {
                return false;
            }
        }
        for (kw, values) in &self.lists {
            let have: Vec<String> = cm.get_list(kw).iter().map(|v| squash_ws(v)).collect();
            let want: Vec<String> = values.iter().map(|v| squash_ws(v)).collect();
            if have != want {
                return false;
            }
        }
        true
    }
}

/// ``format_value(parse_value(text))``, or the text itself when that fails.
pub fn canon(opt: &OptionSpec, text: Option<&str>) -> String {
    format_value(opt, &parse_value(opt, text)).unwrap_or_else(|_| text.unwrap_or("None").to_string())
}

pub fn label_for(key: &str, options: &Options) -> String {
    match options.get(key) {
        Some(o) if !o.label.is_empty() => o.label.clone(),
        _ => crate::util::capitalize(&key.rsplit(':').next().unwrap_or(key).replace('_', " ")),
    }
}

// ---------------------------------------------------------------- validation

fn text_field(data: &Map<String, Json>, name: &str, limit: usize, required: bool) -> PResult<String> {
    let value = match data.get(name) {
        None | Some(Json::Null) => String::new(),
        Some(Json::String(s)) => s.clone(),
        Some(_) => return err(format!("The preset's {name} must be text.")),
    };
    let value = squash_ws(&value);
    if value.chars().count() > limit {
        return err(format!("The preset's {name} is too long (at most {limit} characters)."));
    }
    if !SAFE_RE.is_match(&value) {
        return err(format!("The preset's {name} contains control characters."));
    }
    if required && value.is_empty() {
        return err(format!("The preset has no {name}."));
    }
    Ok(value)
}

pub fn slugify(text: &str) -> String {
    let lowered = text.to_lowercase();
    let slug = SLUG_RE.replace_all(&lowered, "-");
    let slug = take_chars(slug.trim_matches('-'), 40).trim_matches('-').to_string();
    if slug.is_empty() {
        "preset".into()
    } else {
        slug
    }
}

fn as_text(key: &str, raw: &Json) -> PResult<String> {
    match raw {
        Json::Bool(b) => Ok(if *b { "true" } else { "false" }.into()),
        Json::Number(_) | Json::String(_) => Ok(py_str(raw)),
        _ => err(format!("“{key}” has a value Hyprland Mod can't understand.")),
    }
}

fn num_text(v: Option<f64>, raw: Option<&Json>) -> String {
    match (v, raw) {
        (Some(_), Some(raw)) => py_str(raw),
        _ => "None".into(),
    }
}

fn range_check(key: &str, opt: &OptionSpec, num: f64, options: &Options) -> PResult<()> {
    let (lo, hi) = (opt.min, opt.max);
    if lo.is_some_and(|lo| num < lo) || hi.is_some_and(|hi| num > hi) {
        return err(format!(
            "“{}” must be between {} and {} (got {}).",
            label_for(key, options),
            num_text(lo, opt.raw.get("min")),
            num_text(hi, opt.raw.get("max")),
            format_g(num)
        ));
    }
    Ok(())
}

/// Validate one setting by its schema type; returns the canonical config text.
pub fn normalize_value(key: &str, raw: &Json, opt: &OptionSpec, options: &Options) -> PResult<String> {
    let label = label_for(key, options);
    let text = strip(&as_text(key, raw)?).to_string();
    if text.chars().count() > 300 {
        return err(format!("“{label}” has a value that's far too long."));
    }
    if let Err(e) = validate_value(&text) {
        return err(format!("“{label}”: {e}."));
    }
    let bad = || err(format!("“{label}” has an invalid value: {}.", repr_str(&text)));
    match &opt.kind {
        OptionType::Bool => {
            if !["true", "false", "yes", "no", "on", "off", "1", "0"].contains(&text.to_lowercase().as_str()) {
                return bad();
            }
        }
        OptionType::Int | OptionType::Float => {
            if !NUM_RE.is_match(&text) || (opt.kind == OptionType::Int && text.contains('.')) {
                return bad();
            }
            range_check(key, opt, parse_float(&text).unwrap_or(0.0), options)?;
        }
        OptionType::Color => {
            if parse_color(&text).is_none() {
                return bad();
            }
        }
        OptionType::Gradient => {
            let mut tokens: Vec<&str> = GRADIENT_TOKEN_RE.find_iter(&text).map(|m| m.as_str()).collect();
            if tokens.is_empty() || tokens.len() > 11 {
                return bad();
            }
            if tokens.last().is_some_and(|t| ANGLE_RE.is_match(t)) {
                tokens.pop();
            }
            if tokens.is_empty() || tokens.iter().any(|t| parse_color(t).is_none()) {
                return bad();
            }
        }
        OptionType::Vec2 => {
            let replaced = text.replace(',', " ");
            let parts = split_ws(&replaced);
            if parts.len() != 2 || !parts.iter().all(|p| NUM_RE.is_match(p)) {
                return bad();
            }
        }
        OptionType::Enum => {
            let allowed: BTreeSet<String> =
                opt.choices.iter().flatten().map(|c| py_str(&c.value).to_lowercase()).collect();
            if !allowed.is_empty() && !allowed.contains(&text.to_lowercase()) {
                return bad();
            }
        }
        _ => {
            if !SAFE_RE.is_match(&text) {
                return bad();
            }
        }
    }
    Ok(if opt.kind == OptionType::String { text } else { canon(opt, Some(&text)) })
}

pub fn validate_settings(
    raw: Option<&Json>,
    options: &Options,
    preset_only: bool,
) -> PResult<IndexMap<String, String>> {
    let raw = match raw {
        None | Some(Json::Null) => return Ok(IndexMap::new()),
        Some(Json::Object(m)) => m,
        Some(_) => return err("The “settings” part of the file is malformed."),
    };
    if raw.len() > 400 {
        return err("The file has too many settings.");
    }
    let mut out = IndexMap::new();
    for (key, value) in raw {
        let Some(opt) = options.get(key) else {
            if key.split(':').next().unwrap_or("").starts_with("exec") {
                return err("This file tries to run commands, which isn't allowed.");
            }
            return err(format!("“{}” isn't a setting Hyprland Mod knows.", take_chars(key, 60)));
        };
        if preset_only && (PRESET_DENY.contains(&key.as_str()) || !PRESET_SECTIONS.iter().any(|s| key.starts_with(s))) {
            return err(format!("Presets can only change the look; “{}” isn't allowed.", label_for(key, options)));
        }
        out.insert(key.clone(), normalize_value(key, value, opt, options)?);
    }
    Ok(out)
}

fn bezier(entry: &str) -> PResult<(String, String)> {
    let parts: Vec<&str> = entry.split(',').map(strip).collect();
    if parts.len() != 5 || !CURVE_NAME_RE.is_match(parts[0]) || !parts[1..].iter().all(|p| NUM_RE.is_match(p)) {
        return err(format!("Invalid animation curve: {}.", repr_str(take_chars(entry, 60))));
    }
    let n: Vec<f64> = parts[1..].iter().map(|p| parse_float(p).unwrap_or(0.0)).collect();
    let (x0, y0, x1, y1) = (n[0], n[1], n[2], n[3]);
    if !((0.0..=1.0).contains(&x0)
        && (0.0..=1.0).contains(&x1)
        && (-3.0..=3.0).contains(&y0)
        && (-3.0..=3.0).contains(&y1))
    {
        return err(format!("Animation curve “{}” is out of range.", parts[0]));
    }
    Ok((parts[0].to_string(), parts.join(", ")))
}

fn animation(entry: &str, curves: Option<&BTreeSet<String>>) -> PResult<String> {
    let parts: Vec<&str> = entry.splitn(5, ',').map(strip).collect();
    let bad = || err(format!("Invalid animation line: {}.", repr_str(take_chars(entry, 60))));
    if parts.len() < 2 || !ANIMATION_LEAVES.contains(&parts[0]) || !["0", "1"].contains(&parts[1]) {
        return bad();
    }
    if parts.len() > 2 {
        let speed_ok = parts.len() >= 4
            && NUM_RE.is_match(parts[2])
            && parse_float(parts[2]).is_some_and(|s| 0.0 < s && s <= 100.0);
        if !speed_ok {
            return bad();
        }
        let curve = parts[3];
        if !CURVE_NAME_RE.is_match(curve) {
            return bad();
        }
        if let Some(curves) = curves {
            if curve != "default" && !curves.contains(curve) {
                return err(format!(
                    "Animation “{}” uses the curve “{curve}”, which the file doesn't define.",
                    parts[0]
                ));
            }
        }
        if parts.len() == 5 && !STYLE_RE.is_match(&squash_ws(parts[4])) {
            return bad();
        }
    }
    Ok(parts.iter().map(|p| squash_ws(p)).collect::<Vec<_>>().join(", "))
}

pub fn validate_lists(raw: Option<&Json>, strict_curves: bool) -> PResult<IndexMap<String, Vec<String>>> {
    let raw = match raw {
        None | Some(Json::Null) => return Ok(IndexMap::new()),
        Some(Json::Object(m)) => m,
        Some(_) => return err("The “lists” part of the file is malformed."),
    };
    let mut lists: IndexMap<&str, Vec<&str>> = IndexMap::new();
    for (kw, values) in raw {
        if !LIST_KEYWORDS.contains(&kw.as_str()) {
            if ["exec", "bind", "env", "plugin", "source"].iter().any(|p| kw.starts_with(p)) {
                return err("This file tries to run commands or change key bindings, which isn't allowed.");
            }
            return err(format!("“{}” lines aren't allowed in this file.", take_chars(kw, 40)));
        }
        let malformed = || err(format!("The “{kw}” lines are malformed."));
        let Some(items) = values.as_array().filter(|a| a.len() <= 64) else { return malformed() };
        let mut strings = Vec::new();
        for v in items {
            let Some(s) = v.as_str() else { return malformed() };
            strings.push(s);
        }
        for s in &strings {
            if !SAFE_RE.is_match(s) || s.chars().count() > 200 {
                return malformed();
            }
        }
        lists.insert(kw.as_str(), strings);
    }
    let mut out = IndexMap::new();
    let mut names = BTreeSet::new();
    if let Some(entries) = lists.get("bezier") {
        let mut lines = Vec::new();
        for e in entries {
            let (name, line) = bezier(e)?;
            names.insert(name);
            lines.push(line);
        }
        out.insert("bezier".to_string(), lines);
    }
    if let Some(entries) = lists.get("animation") {
        let curves = strict_curves.then_some(&names);
        let lines: PResult<Vec<String>> = entries.iter().map(|e| animation(e, curves)).collect();
        out.insert("animation".to_string(), lines?);
    }
    Ok(out)
}

fn preview_meta(raw: Option<&Json>) -> Map<String, Json> {
    let mut out = Map::new();
    let Some(raw) = raw.and_then(Json::as_object) else { return out };
    if let Some(Json::Array(wall)) = raw.get("wallpaper") {
        if (1..=3).contains(&wall.len()) && wall.iter().all(|c| c.as_str().is_some_and(|s| HEX_RE.is_match(s))) {
            out.insert("wallpaper".into(), Json::Array(wall.clone()));
        }
    }
    if let Some(Json::String(win)) = raw.get("window") {
        if HEX_RE.is_match(win) {
            out.insert("window".into(), Json::String(win.clone()));
        }
    }
    out
}

fn int_version(data: &Map<String, Json>) -> Option<i64> {
    match data.get("version")? {
        Json::Number(n) if !n.is_f64() => n.as_i64(),
        Json::Bool(b) => Some(*b as i64),
        _ => None,
    }
}

/// Check a decoded preset document; returns a clean [`Preset`].
pub fn validate_preset(data: &Json, options: &Options, builtin: bool) -> PResult<Preset> {
    let Some(data) = data.as_object() else { return err("This isn't a Hyprland Mod preset.") };
    let format = data.get("format").and_then(Json::as_str);
    if format == Some(SETUP_FORMAT) {
        return err("This is a whole-setup file. Open it from Import & Export instead.");
    }
    if format != Some(PRESET_FORMAT) {
        return err("This isn't a Hyprland Mod preset.");
    }
    if int_version(data).is_none_or(|v| v > FORMAT_VERSION) {
        return err("This preset was made by a newer version of Hyprland Mod.");
    }
    let name = text_field(data, "name", 60, true)?;
    let id = match data.get("id") {
        Some(Json::String(s)) if ID_RE.is_match(s) => s.clone(),
        _ => slugify(&name),
    };
    let tags = match data.get("tags") {
        Some(t) if truthy(t) => t.clone(),
        _ => Json::Array(Vec::new()),
    };
    let Some(tags) = tags.as_array().filter(|t| t.len() <= 8) else {
        return err("The preset's tags are malformed.");
    };
    let mut clean_tags: Vec<String> = Vec::new();
    for t in tags {
        let ok = t.as_str().filter(|s| !strip(s).is_empty() && s.chars().count() <= 24 && SAFE_RE.is_match(s));
        let Some(t) = ok else { return err("The preset's tags are malformed.") };
        let low = strip(t).to_lowercase();
        if !clean_tags.contains(&low) {
            clean_tags.push(low);
        }
    }
    let settings = validate_settings(data.get("settings"), options, true)?;
    let lists = validate_lists(data.get("lists"), true)?;
    if settings.is_empty() && lists.is_empty() {
        return err("This preset doesn't change anything.");
    }
    Ok(Preset {
        id,
        name,
        description: text_field(data, "description", 300, false)?,
        author: text_field(data, "author", 60, false)?,
        tags: clean_tags,
        settings,
        lists,
        preview: preview_meta(data.get("preview")),
        builtin,
        path: None,
    })
}

fn read_json(path: &Path) -> PResult<Json> {
    let meta = std::fs::metadata(path).map_err(|e| PresetError(format!("Couldn't read the file: {e}.")))?;
    if meta.len() > MAX_FILE_BYTES {
        return err("The file is too large to be a Hyprland Mod file.");
    }
    let bytes = std::fs::read(path).map_err(|e| PresetError(format!("Couldn't read the file: {e}.")))?;
    let text = String::from_utf8(bytes).map_err(|e| PresetError(format!("Couldn't read the file: {e}.")))?;
    serde_json::from_str(&text).map_err(|_| PresetError("The file is damaged or isn't a Hyprland Mod file.".into()))
}

fn stems(folder: &Path) -> BTreeSet<String> {
    crate::parser::glob_sorted(&folder.join("*.json"))
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect()
}

fn unique_id(base: &str, taken: &BTreeSet<String>) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while taken.contains(&id) {
        id = format!("{}-{n}", take_chars(base, 44));
        n += 1;
    }
    id
}

fn write_doc(path: &Path, data: &Json) -> std::io::Result<()> {
    let opts = Dumps { indent: Some(2), sort_keys: false, ensure_ascii: false };
    atomic_write(path, &(dumps(data, opts) + "\n"))
}

/// What importing a setup would change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub key: String,
    pub label: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// Built-in presets, user presets and setup files for one data directory.
pub struct PresetStore {
    pub options: Options,
    pub builtin_dir: PathBuf,
    /// ``<data_dir>/presets``
    pub user_dir: PathBuf,
}

impl PresetStore {
    /// ``app_data``: the folder holding ``schema/`` and ``presets/``; ``data_dir``: the user's data folder.
    pub fn new(app_data: &Path, data_dir: &Path) -> Self {
        PresetStore {
            options: options_by_key(&load_schema(Some(&app_data.join("schema")))),
            builtin_dir: app_data.join("presets"),
            user_dir: data_dir.join("presets"),
        }
    }

    /// Store for the manager's data directory and the default app data.
    pub fn for_manager(cm: &ConfigManager) -> Self {
        Self::new(&app_data_dir(), cm.data_dir())
    }

    pub fn validate_preset(&self, data: &Json) -> PResult<Preset> {
        validate_preset(data, &self.options, false)
    }

    pub fn load_builtin(&self) -> PResult<Vec<Preset>> {
        let mut out: Vec<(f64, Preset)> = Vec::new();
        for path in crate::parser::glob_sorted(&self.builtin_dir.join("*.json")) {
            let data = read_json(&path)?;
            let mut preset = validate_preset(&data, &self.options, true)?;
            preset.path = Some(path);
            let order = data.get("order").and_then(Json::as_f64).unwrap_or(999.0);
            out.push((order, preset));
        }
        out.sort_by(|a, b| a.0.total_cmp(&b.0));
        Ok(out.into_iter().map(|(_, p)| p).collect())
    }

    /// User presets (sorted by name) plus messages about files that were skipped.
    pub fn load_user(&self) -> (Vec<Preset>, Vec<String>) {
        let (mut presets, mut problems) = (Vec::new(), Vec::new());
        if !self.user_dir.is_dir() {
            return (presets, problems);
        }
        for path in crate::parser::glob_sorted(&self.user_dir.join("*.json")) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            match read_json(&path).and_then(|d| self.validate_preset(&d)) {
                Ok(mut p) => {
                    p.path = Some(path);
                    presets.push(p);
                }
                Err(e) => problems.push(format!("{name}: {e}")),
            }
        }
        presets.sort_by_key(|p| p.name.to_lowercase());
        (presets, problems)
    }

    /// Write ``preset`` into the user folder (new presets get a unique id).
    pub fn save_user(&self, preset: &mut Preset) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.user_dir)?;
        if preset.path.is_none() {
            let mut taken = stems(&self.user_dir);
            taken.extend(stems(&self.builtin_dir));
            taken.insert(ORIGINAL_ID.to_string());
            let base = if preset.id.is_empty() { &preset.name } else { &preset.id };
            preset.id = unique_id(&slugify(base), &taken);
            preset.path = Some(self.user_dir.join(format!("{}.json", preset.id)));
        }
        preset.builtin = false;
        write_doc(preset.path.as_deref().expect("set above"), &preset.to_dict())?;
        Ok(())
    }

    /// Remove a user preset file; refuses anything outside the user preset folder.
    pub fn delete_user(&self, preset: &Preset) -> PResult<()> {
        let Some(path) = preset.path.as_deref().filter(|_| !preset.builtin) else {
            return err("Built-in presets can't be deleted.");
        };
        if preset.original() {
            return err("“My original look” is your safety net and can't be deleted.");
        }
        let path = realpath(path);
        if path.parent() != Some(realpath(&self.user_dir).as_path()) {
            return err("Only your own presets can be deleted.");
        }
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => err(format!("Couldn't delete the preset: {e}.")),
            _ => Ok(()),
        }
    }

    pub fn rename_user(&self, preset: &mut Preset, name: &str) -> anyhow::Result<()> {
        if preset.builtin {
            return Err(PresetError("Built-in presets can't be renamed.".into()).into());
        }
        let mut doc = Map::new();
        doc.insert("name".into(), json!(name));
        preset.name = text_field(&doc, "name", 60, true)?;
        self.save_user(preset)
    }

    pub fn read_preset_file(&self, path: &Path) -> PResult<Preset> {
        self.validate_preset(&read_json(path)?)
    }

    /// Validate a shared preset file and copy it into the user folder.
    pub fn import_preset(&self, path: &Path) -> anyhow::Result<Preset> {
        let mut preset = self.read_preset_file(path)?;
        preset.path = None;
        self.save_user(&mut preset)?;
        Ok(preset)
    }

    pub fn export_preset(&self, preset: &Preset, path: &Path) -> std::io::Result<()> {
        write_doc(path, &preset.to_dict())
    }

    // ------------------------------------------------------------ staging

    /// A preset from what is currently shown (applied + staged values).
    pub fn capture(&self, cm: &ConfigManager, name: &str, description: &str, author: &str) -> PResult<Preset> {
        let mut settings = Map::new();
        for key in LOOK_KEYS {
            if let Some(opt) = self.options.get(*key) {
                settings.insert(key.to_string(), json!(canon(opt, cm.get(key).as_deref())));
            }
        }
        let mut lists = Map::new();
        for kw in LIST_KEYWORDS {
            let values = cm.get_list(kw);
            if !values.is_empty() {
                lists.insert(kw.to_string(), json!(values));
            }
        }
        let mut data = json!({"format": PRESET_FORMAT, "version": FORMAT_VERSION, "name": name,
            "description": description, "author": author, "tags": [], "settings": settings, "lists": lists});
        self.validate_preset(&data).or_else(|_| {
            // e.g. animations using curves defined somewhere we can't see: keep the look only
            data["lists"] = json!({});
            self.validate_preset(&data)
        })
    }

    pub fn original_path(&self) -> PathBuf {
        self.user_dir.join(format!("{ORIGINAL_ID}.json"))
    }

    fn marker_path(&self) -> PathBuf {
        self.user_dir.parent().unwrap_or(Path::new("/")).join(ORIGINAL_MARKER)
    }

    pub fn original_captured(&self) -> bool {
        self.marker_path().exists()
    }

    pub fn load_original(&self) -> Option<Preset> {
        let path = self.original_path();
        if !path.is_file() {
            return None;
        }
        let mut preset = read_json(&path).and_then(|d| self.validate_preset(&d)).ok()?;
        preset.id = ORIGINAL_ID.into();
        preset.path = Some(path);
        Some(preset)
    }

    /// Capture "My original look" once (first launch); later calls just return it.
    pub fn ensure_original(&self, cm: &ConfigManager) -> Option<Preset> {
        if self.original_captured() {
            return self.load_original();
        }
        let keys: Vec<&str> = LOOK_KEYS.iter().copied().filter(|k| self.options.contains_key(*k)).collect();
        // Per key: the snapshot (live or the user's config), else what is shown without our
        // override, else Hyprland's own default from the schema.
        let snapshot = cm.snapshot_current_look(&keys);
        let mut settings = Map::new();
        for key in &keys {
            let opt = &self.options[*key];
            let value = snapshot
                .get(*key)
                .cloned()
                .flatten()
                .or_else(|| if cm.is_modified(key) { cm.user_value(key) } else { cm.get(key) })
                .or_else(|| format_value(opt, &parse_value(opt, None)).ok());
            if let Some(value) = value.filter(|v| !v.is_empty()) {
                settings.insert(key.to_string(), json!(value));
            }
        }
        let (beziers, animations) = cm.current_animations();
        let mut lists = Map::new();
        for (kw, v) in [("bezier", beziers), ("animation", animations)] {
            if !v.is_empty() {
                lists.insert(kw.into(), json!(v));
            }
        }
        let mut data = json!({"format": PRESET_FORMAT, "version": FORMAT_VERSION, "id": ORIGINAL_ID,
            "name": ORIGINAL_NAME, "description": ORIGINAL_DESC, "author": "", "tags": [],
            "settings": settings, "lists": lists});
        let mut preset = self
            .validate_preset(&data)
            .or_else(|_| {
                data["lists"] = json!({}); // e.g. animation lines using curves we can't see
                self.validate_preset(&data)
            })
            .ok()?;
        preset.id = ORIGINAL_ID.into();
        std::fs::create_dir_all(&self.user_dir).ok()?;
        preset.path = Some(self.original_path());
        self.save_user(&mut preset).ok()?;
        atomic_write(&self.marker_path(), "1\n").ok()?;
        Some(preset)
    }

    /// Every setting a preset can own: the look keys plus anything built-in presets set.
    pub fn managed_look_keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = LOOK_KEYS.iter().map(|k| k.to_string()).collect();
        if let Ok(builtin) = self.load_builtin() {
            for p in builtin {
                for k in p.settings.keys() {
                    if !keys.contains(k) {
                        keys.push(k.clone());
                    }
                }
            }
        }
        keys
    }

    /// True when Hyprland Mod overrides (or has staged) any preset-managed look setting.
    pub fn has_overrides(&self, cm: &ConfigManager) -> bool {
        self.managed_look_keys().iter().any(|k| cm.is_modified(k))
            || LIST_KEYWORDS.iter().any(|kw| cm.is_list_modified(kw))
    }

    /// Remove Hyprland Mod's look overrides so the user's own config takes effect again (staged).
    pub fn restore_original(&self, cm: &mut ConfigManager) {
        for key in self.managed_look_keys() {
            cm.reset(&key);
        }
        for kw in LIST_KEYWORDS {
            cm.reset_list(kw);
        }
    }

    /// Stage every setting and list of ``preset`` (nothing is written until Apply).
    pub fn stage_preset(&self, cm: &mut ConfigManager, preset: &Preset) -> anyhow::Result<()> {
        if !preset.original() {
            self.ensure_original(cm); // the way back must exist before the first change
        }
        for (key, value) in &preset.settings {
            cm.set(key, value.clone())?;
        }
        for kw in LIST_KEYWORDS {
            if let Some(values) = preset.lists.get(*kw) {
                cm.set_list(kw, values.clone())?;
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ whole setup

    pub fn setup_document(&self, cm: &ConfigManager) -> Json {
        let created = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
        json!({"format": SETUP_FORMAT, "version": FORMAT_VERSION, "created": created,
            "app_version": env!("CARGO_PKG_VERSION"), "state": cm.export_state()})
    }

    pub fn export_setup(&self, cm: &ConfigManager, path: &Path) -> std::io::Result<Json> {
        let data = self.setup_document(cm);
        write_doc(path, &data)?;
        Ok(data)
    }

    /// Check a decoded setup document; returns a clean state for [`ConfigManager::import_state`].
    pub fn validate_setup(&self, data: &Json) -> PResult<Json> {
        let Some(data) = data.as_object() else { return err("This isn't a Hyprland Mod setup file.") };
        let format = data.get("format").and_then(Json::as_str);
        if format == Some(PRESET_FORMAT) {
            return err("This is a preset. Import it from the Presets page instead.");
        }
        if format != Some(SETUP_FORMAT) {
            return err("This isn't a Hyprland Mod setup file.");
        }
        if int_version(data).is_none_or(|v| v > FORMAT_VERSION) {
            return err("This setup was made by a newer version of Hyprland Mod.");
        }
        let Some(state) = data.get("state").and_then(Json::as_object) else {
            return err("The setup file is damaged.");
        };
        let or_empty = |k: &str| state.get(k).filter(|v| truthy(v)).cloned().unwrap_or_else(|| json!({}));
        let settings = validate_settings(Some(&or_empty("settings")), &self.options, false)?;
        let lists = validate_lists(Some(&or_empty("lists")), false)?;
        Ok(json!({"version": 1, "settings": settings, "lists": lists}))
    }

    pub fn read_setup(&self, path: &Path) -> PResult<Json> {
        self.validate_setup(&read_json(path)?)
    }

    /// What importing ``state`` would change compared to what is shown now.
    pub fn setup_changes(&self, cm: &ConfigManager, state: &Json) -> Vec<Change> {
        let blank = OptionSpec::new("", OptionType::String);
        let current = cm.export_state();
        let empty = Map::new();
        let new_settings = state.get("settings").and_then(Json::as_object).unwrap_or(&empty);
        let mut changes = Vec::new();
        let mut keys: Vec<&String> = new_settings.keys().collect();
        keys.sort();
        for key in keys {
            let value = py_str(&new_settings[key]);
            let opt = self.options.get(key).unwrap_or(&blank);
            let old = cm.get(key);
            if canon(opt, old.as_deref()) != canon(opt, Some(&value)) {
                changes.push(Change { key: key.clone(), label: label_for(key, &self.options), old, new: Some(value) });
            }
        }
        let cur_settings = current.get("settings").and_then(Json::as_object).unwrap_or(&empty);
        let mut gone: Vec<&String> = cur_settings.keys().filter(|k| !new_settings.contains_key(*k)).collect();
        gone.sort();
        for key in gone {
            changes.push(Change {
                key: key.clone(),
                label: label_for(key, &self.options),
                old: cm.get(key),
                new: None,
            });
        }
        let lines =
            |v: Option<&Json>| -> Option<Vec<Json>> { v.and_then(Json::as_array).filter(|a| !a.is_empty()).cloned() };
        for kw in LIST_KEYWORDS {
            let old = lines(current.get("lists").and_then(|l| l.get(*kw)));
            let new = lines(state.get("lists").and_then(|l| l.get(*kw)));
            if old != new {
                let title = if *kw == "bezier" { "Animation curves" } else { "Animations" };
                let count = |v: Option<Vec<Json>>| {
                    v.map(|v| format!("{} line{}", v.len(), if v.len() != 1 { "s" } else { "" }))
                };
                changes.push(Change { key: kw.to_string(), label: title.into(), old: count(old), new: count(new) });
            }
        }
        changes
    }

    /// Back up first, then stage ``state`` as the complete managed setup. Returns the backup id.
    pub fn import_setup(&self, cm: &mut ConfigManager, state: Json) -> anyhow::Result<String> {
        let id = cm.create_backup("Before import")?;
        cm.import_state(state)?;
        Ok(id)
    }
}
