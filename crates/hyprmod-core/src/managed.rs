//! The managed settings file (``hyprland-mod.conf``), its state and the main config's source line.

use std::path::Path;
use std::sync::LazyLock;

use anyhow::{bail, Result};
use indexmap::IndexMap;
use regex::Regex;
use serde_json::{json, Map, Value as Json};

use crate::fsutil::read_text;
use crate::parser::{escape_value, expand_path, parse_text, strip_comment, ParsedConfig};
use crate::util::{abspath, path_str, py_str, realpath, relpath, repr_str, splitlines, truthy};

pub const HEADER: &str = "# Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n\
# It is sourced at the end of hyprland.conf so these values take precedence.\n";
pub const SOURCE_MARKER: &str = "# Hyprland Mod: keep this line last so its settings take precedence";
/// Binds, window rules and monitors follow this line; their exact state is the JSON comment.
pub const EXTRAS_MARKER: &str = "# ---- Hyprland Mod: keybinds, window rules, monitors ----";
pub const EXTRAS_PREFIX: &str = "# hyprland-mod-extras: ";
pub const EXTRA_FIELDS: &[&str] = &["binds", "window_rules", "monitors", "autostart"];

/// Keywords that may legitimately repeat; stored as ordered lists, not single values.
pub const LIST_KEYWORDS: &[&str] = &[
    "bezier",
    "animation",
    "monitor",
    "workspace",
    "windowrule",
    "windowrulev2",
    "layerrule",
    "exec",
    "exec-once",
    "exec-shutdown",
    "env",
    "submap",
    "gesture",
    "permission",
    "plugin",
];
/// Beziers must be defined before animations use them.
pub const LIST_ORDER: &[&str] = &["bezier", "animation"];

static KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.\-]+(?::[A-Za-z0-9_.\-]+)*$").unwrap());
static SOURCE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^source\s*=\s*(.+)$").unwrap());

pub fn is_list_keyword(key: &str) -> bool {
    LIST_KEYWORDS.contains(&key) || key.starts_with("bind")
}

/// Everything Hyprland Mod manages. Item fields (binds, rules, monitors,
/// autostart) are kept as plain JSON so nothing written by any version is lost;
/// unknown top-level keys are kept in ``other``.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManagedState {
    pub settings: IndexMap<String, String>,
    pub lists: IndexMap<String, Vec<String>>,
    pub binds: Vec<Map<String, Json>>,
    pub window_rules: Vec<Map<String, Json>>,
    pub monitors: IndexMap<String, Map<String, Json>>,
    pub autostart: IndexMap<String, String>,
    pub other: Map<String, Json>,
}

impl ManagedState {
    /// Plain data; the item fields only appear when used (old snapshots stay equal).
    pub fn to_dict(&self) -> Json {
        let mut out = Map::new();
        out.insert("version".into(), json!(1));
        out.insert(
            "settings".into(),
            Json::Object(self.settings.iter().map(|(k, v)| (k.clone(), Json::String(v.clone()))).collect()),
        );
        out.insert("lists".into(), Json::Object(self.lists.iter().map(|(k, v)| (k.clone(), json!(v))).collect()));
        if let Json::Object(extras) = self.extras() {
            out.extend(extras);
        }
        for (k, v) in &self.other {
            out.entry(k.clone()).or_insert_with(|| v.clone());
        }
        Json::Object(out)
    }

    pub fn extras(&self) -> Json {
        let mut out = Map::new();
        for name in EXTRA_FIELDS {
            if let Some(v) = self.item(name) {
                if truthy(&v) {
                    out.insert((*name).into(), v);
                }
            }
        }
        Json::Object(out)
    }

    /// One item field (``binds``/``window_rules``/``monitors``/``autostart``) as JSON.
    pub fn item(&self, name: &str) -> Option<Json> {
        Some(match name {
            "binds" => Json::Array(self.binds.iter().cloned().map(Json::Object).collect()),
            "window_rules" => Json::Array(self.window_rules.iter().cloned().map(Json::Object).collect()),
            "monitors" => {
                Json::Object(self.monitors.iter().map(|(k, v)| (k.clone(), Json::Object(v.clone()))).collect())
            }
            "autostart" => {
                Json::Object(self.autostart.iter().map(|(k, v)| (k.clone(), Json::String(v.clone()))).collect())
            }
            _ => return None,
        })
    }

    /// Replace one item field from JSON (same coercions as [`ManagedState::from_dict`]).
    pub fn set_item(&mut self, name: &str, value: &Json) {
        match name {
            "binds" => self.binds = objects(value),
            "window_rules" => self.window_rules = objects(value),
            "monitors" => {
                self.monitors = value
                    .as_object()
                    .map(|m| m.iter().filter_map(|(k, v)| Some((k.clone(), v.as_object()?.clone()))).collect())
                    .unwrap_or_default()
            }
            "autostart" => {
                self.autostart = value
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), py_str(v))).collect())
                    .unwrap_or_default()
            }
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        self.settings.is_empty()
            && self.lists.is_empty()
            && self.binds.is_empty()
            && self.window_rules.is_empty()
            && self.monitors.is_empty()
            && self.autostart.is_empty()
    }

    pub fn from_dict(data: &Json) -> Result<Self> {
        let Some(obj) = data.as_object() else { bail!("state must be a dict") };
        let empty_obj = Json::Object(Map::new());
        let empty_arr = Json::Array(Vec::new());
        let field = |name: &str, default: &'_ Json| -> Json {
            match obj.get(name) {
                Some(v) if truthy(v) => v.clone(),
                _ => default.clone(),
            }
        };
        let settings = field("settings", &empty_obj);
        let lists = field("lists", &empty_obj);
        let (Some(settings), Some(lists)) = (settings.as_object(), lists.as_object()) else {
            bail!("settings and lists must be dicts");
        };
        let binds = field("binds", &empty_arr);
        let rules = field("window_rules", &empty_arr);
        let monitors = field("monitors", &empty_obj);
        let autostart = field("autostart", &empty_obj);
        if !binds.is_array() || !rules.is_array() || !monitors.is_object() || !autostart.is_object() {
            bail!("binds/window_rules must be lists, monitors/autostart dicts");
        }
        let mut state = ManagedState {
            settings: settings.iter().map(|(k, v)| (k.clone(), py_str(v))).collect(),
            lists: lists
                .iter()
                .filter_map(|(k, v)| Some((k.clone(), v.as_array()?.iter().map(py_str).collect())))
                .collect(),
            ..Default::default()
        };
        state.set_item("binds", &binds);
        state.set_item("window_rules", &rules);
        state.set_item("monitors", &monitors);
        state.set_item("autostart", &autostart);
        for (k, v) in obj {
            if !matches!(k.as_str(), "version" | "settings" | "lists") && !EXTRA_FIELDS.contains(&k.as_str()) {
                state.other.insert(k.clone(), v.clone());
            }
        }
        Ok(state)
    }
}

fn objects(value: &Json) -> Vec<Map<String, Json>> {
    value.as_array().map(|a| a.iter().filter_map(|v| v.as_object().cloned()).collect()).unwrap_or_default()
}

pub fn state_from_parsed(parsed: &ParsedConfig) -> ManagedState {
    let mut state = ManagedState::default();
    let mut counts: IndexMap<&str, usize> = IndexMap::new();
    for e in &parsed.entries {
        *counts.entry(e.key.as_str()).or_default() += 1;
    }
    for e in &parsed.entries {
        if is_list_keyword(&e.key) || counts[e.key.as_str()] > 1 {
            state.lists.entry(e.key.clone()).or_default().push(e.value.clone());
        } else {
            state.settings.insert(e.key.clone(), e.value.clone());
        }
    }
    state
}

/// Settings/lists are parsed from the file; items come from the extras JSON comment.
pub fn read_managed(path: &Path) -> ManagedState {
    if !path.is_file() {
        return ManagedState::default();
    }
    let Ok(text) = read_text(path) else { return ManagedState::default() };
    let marker = format!("{EXTRAS_MARKER}\n");
    let (head, tail) = match text.find(&marker) {
        Some(i) => (&text[..i], Some(&text[i + marker.len()..])),
        None => (text.as_str(), None),
    };
    let mut state = state_from_parsed(&parse_text(head, &path_str(path)));
    if let Some(tail) = tail {
        for line in splitlines(tail, false) {
            if let Some(payload) = line.strip_prefix(EXTRAS_PREFIX) {
                if let Some(extras) =
                    serde_json::from_str::<Json>(payload).ok().and_then(|d| ManagedState::from_dict(&d).ok())
                {
                    state.binds = extras.binds;
                    state.window_rules = extras.window_rules;
                    state.monitors = extras.monitors;
                    state.autostart = extras.autostart;
                    state.other = extras.other;
                }
                break;
            }
        }
    }
    state
}

pub fn validate_key(key: &str) -> Result<()> {
    if !KEY_RE.is_match(key) {
        bail!("invalid setting key: {}", repr_str(key));
    }
    if key.split(':').next() == Some("device") || key == "source" || key.starts_with('$') {
        bail!("unsupported setting key: {}", repr_str(key));
    }
    Ok(())
}

pub fn validate_value(value: &str) -> Result<()> {
    if value.contains('\n') || value.contains('\r') {
        bail!("value must be a single line");
    }
    if value.contains('{') || value.contains('}') {
        bail!("value must not contain braces");
    }
    Ok(())
}

#[derive(Default)]
struct ConfNode {
    values: IndexMap<String, String>,
    sections: IndexMap<String, ConfNode>,
}

/// Render as nested sections (``decoration { blur { enabled = true } }``).
pub fn render(state: &ManagedState) -> String {
    let mut tree = ConfNode::default();
    let mut keys: Vec<&String> = state.settings.keys().collect();
    keys.sort();
    for key in keys {
        let parts: Vec<&str> = key.split(':').collect();
        let (leaf, sections) = parts.split_last().expect("split yields one part");
        let mut node = &mut tree;
        for s in sections {
            node = node.sections.entry((*s).to_string()).or_default();
        }
        node.values.insert((*leaf).to_string(), state.settings[key].clone());
    }
    let mut out = String::from(HEADER);
    fn emit(node: &ConfNode, depth: usize, out: &mut String) {
        let pad = "    ".repeat(depth);
        for (name, val) in &node.values {
            out.push_str(&format!("{pad}{name} = {}\n", escape_value(val)));
        }
        for (name, child) in &node.sections {
            out.push_str(&format!("{pad}{name} {{\n"));
            emit(child, depth + 1, out);
            out.push_str(&format!("{pad}}}\n"));
        }
    }
    if !tree.values.is_empty() || !tree.sections.is_empty() {
        out.push('\n');
        emit(&tree, 0, &mut out);
    }
    let ordered = LIST_ORDER
        .iter()
        .filter(|k| state.lists.contains_key(**k))
        .map(|k| k.to_string())
        .chain(state.lists.keys().filter(|k| !LIST_ORDER.contains(&k.as_str())).cloned());
    for kw in ordered {
        let values = &state.lists[&kw];
        if !values.is_empty() {
            out.push('\n');
            for v in values {
                out.push_str(&format!("{kw} = {}\n", escape_value(v)));
            }
        }
    }
    out
}

// ---------------------------------------------------------------- source line

/// Path as written in the source line (``~/`` form when under ``home``).
pub fn source_path_text(managed_path: &Path, home: Option<&str>) -> String {
    let path = path_str(&abspath(managed_path));
    if let Some(home) = home {
        if !home.is_empty() && home != "/" && path.starts_with(&format!("{}/", home.trim_end_matches('/'))) {
            return format!("~/{}", path_str(&relpath(Path::new(&path), Path::new(home))));
        }
    }
    path
}

fn points_to(line: &str, managed_path: &Path, config_dir: &Path, home: Option<&str>) -> bool {
    let code = strip_comment(line);
    let Some(m) = SOURCE_RE.captures(&code) else { return false };
    realpath(&expand_path(&m[1], config_dir, home)) == realpath(managed_path)
}

fn config_dir(config_path: &Path) -> std::path::PathBuf {
    abspath(config_path).parent().map(Path::to_path_buf).unwrap_or_else(|| "/".into())
}

/// The config text with exactly one source line at the end, or None if already so.
pub fn with_source_line(text: &str, config_path: &Path, managed_path: &Path, home: Option<&str>) -> Option<String> {
    let dir = config_dir(config_path);
    let lines = splitlines(text, true);
    let ours: Vec<usize> = (0..lines.len()).filter(|&i| points_to(lines[i], managed_path, &dir, home)).collect();
    let meaningful: Vec<usize> = (0..lines.len()).filter(|&i| !strip_comment(lines[i]).is_empty()).collect();
    if ours.len() == 1 && meaningful.last() == Some(&ours[0]) {
        return None;
    }
    let body = append_loader_body(&lines, &ours, SOURCE_MARKER);
    Some(format!("{body}{SOURCE_MARKER}\nsource = {}\n", source_path_text(managed_path, home)))
}

/// Lines without ours (and their markers), ending with a blank line.
pub(crate) fn append_loader_body(lines: &[&str], ours: &[usize], marker: &str) -> String {
    let drop = drop_set(lines, ours, marker);
    let mut body: String = lines.iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, l)| *l).collect();
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() && !body.ends_with("\n\n") {
        body.push('\n');
    }
    body
}

fn drop_set(lines: &[&str], ours: &[usize], marker: &str) -> std::collections::BTreeSet<usize> {
    let mut drop: std::collections::BTreeSet<usize> = ours.iter().copied().collect();
    for &i in ours {
        if i > 0 && crate::util::strip(lines[i - 1]) == marker {
            drop.insert(i - 1);
        }
    }
    drop
}

/// Text without our loader lines (and their marker); None when there are none.
///
/// The blank line [`with_source_line`] put before the marker is removed again.
pub fn strip_loader(lines: &[&str], ours: &[usize], marker: &str) -> Option<String> {
    if ours.is_empty() {
        return None;
    }
    let drop = drop_set(lines, ours, marker);
    let mut body: String = lines.iter().enumerate().filter(|(i, _)| !drop.contains(i)).map(|(_, l)| *l).collect();
    let last = *drop.iter().next_back().expect("non-empty");
    if body.ends_with("\n\n") && last + 1 >= lines.len() {
        body.pop();
    }
    Some(body)
}

/// Config text with our ``source =`` line removed, or None if it has none.
pub fn without_source_line(text: &str, config_path: &Path, managed_path: &Path, home: Option<&str>) -> Option<String> {
    let dir = config_dir(config_path);
    let lines = splitlines(text, true);
    let ours: Vec<usize> = (0..lines.len()).filter(|&i| points_to(lines[i], managed_path, &dir, home)).collect();
    strip_loader(&lines, &ours, SOURCE_MARKER)
}
