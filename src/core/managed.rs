//! The managed configuration file (`hyprland-mod.conf`) generator and parser.

use anyhow::{bail, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use super::fsutil::read_text;
use super::parser::{escape_value, expand_path, parse_text, strip_comment, ParsedConfig};

pub const HEADER: &str = "# Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n# It is sourced at the end of hyprland.conf so these values take precedence.\n";
pub const SOURCE_MARKER: &str = "# Hyprland Mod: keep this line last so its settings take precedence";
pub const EXTRAS_MARKER: &str = "# ---- Hyprland Mod: keybinds, window rules, monitors ----";
pub const EXTRAS_PREFIX: &str = "# hyprland-mod-extras: ";

pub const LIST_ORDER: &[&str] = &["bezier", "animation"];
static LIST_KEYWORDS: &[&str] = &[
    "bezier", "animation", "monitor", "workspace", "windowrule", "windowrulev2",
    "layerrule", "exec", "exec-once", "exec-shutdown", "env", "submap", "gesture",
    "permission", "plugin",
];

static KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.\-]+(?::[A-Za-z0-9_.\-]+)*$").unwrap());

pub fn is_list_keyword(key: &str) -> bool {
    LIST_KEYWORDS.contains(&key) || key.starts_with("bind")
}

pub fn validate_key(key: &str) -> Result<()> {
    if !KEY_RE.is_match(key) {
        bail!("invalid setting key: {:?}", key);
    }
    let first = key.split(':').next().unwrap_or("");
    if first == "device" || key == "source" || key.starts_with('$') {
        bail!("unsupported setting key: {:?}", key);
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct ManagedState {
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    #[serde(default)]
    pub lists: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub binds: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub window_rules: Vec<serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub monitors: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub autostart: BTreeMap<String, String>,
}

impl ManagedState {
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty()
            && self.lists.is_empty()
            && self.binds.is_empty()
            && self.window_rules.is_empty()
            && self.monitors.is_empty()
            && self.autostart.is_empty()
    }
}

pub fn state_from_parsed(parsed: &ParsedConfig) -> ManagedState {
    let mut state = ManagedState::default();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for e in &parsed.entries {
        *counts.entry(&e.key).or_insert(0) += 1;
    }
    for e in &parsed.entries {
        if is_list_keyword(&e.key) || counts.get(e.key.as_str()).copied().unwrap_or(0) > 1 {
            state.lists.entry(e.key.clone()).or_default().push(e.value.clone());
        } else {
            state.settings.insert(e.key.clone(), e.value.clone());
        }
    }
    state
}

pub fn read_managed<P: AsRef<Path>>(path: P) -> ManagedState {
    let path = path.as_ref();
    if !path.is_file() {
        return ManagedState::default();
    }
    let text = match read_text(path) {
        Ok(t) => t,
        Err(_) => return ManagedState::default(),
    };

    let marker_line = format!("{}\n", EXTRAS_MARKER);
    let (head, tail) = match text.find(&marker_line) {
        Some(pos) => (&text[..pos], &text[pos + marker_line.len()..]),
        None => (text.as_str(), ""),
    };

    let parsed = parse_text(head, &path.to_string_lossy());
    let mut state = state_from_parsed(&parsed);

    for line in tail.lines() {
        if let Some(json_str) = line.strip_prefix(EXTRAS_PREFIX) {
            if let Ok(extras) = serde_json::from_str::<ManagedState>(json_str) {
                state.binds = extras.binds;
                state.window_rules = extras.window_rules;
                state.monitors = extras.monitors;
                state.autostart = extras.autostart;
            }
            break;
        }
    }

    state
}



pub fn render(state: &ManagedState) -> String {
    let mut out = String::from(HEADER);

    // Build section tree
    let mut root_sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut top_level_settings: BTreeMap<String, String> = BTreeMap::new();

    for (k, v) in &state.settings {
        if let Some((section, subkey)) = k.split_once(':') {
            root_sections
                .entry(section.to_string())
                .or_default()
                .insert(subkey.to_string(), v.clone());
        } else {
            top_level_settings.insert(k.clone(), v.clone());
        }
    }

    if !top_level_settings.is_empty() {
        out.push('\n');
        for (k, v) in &top_level_settings {
            out.push_str(&format!("{} = {}\n", k, escape_value(v)));
        }
    }

    for (section, entries) in &root_sections {
        out.push('\n');
        out.push_str(&format!("{} {{\n", section));
        for (k, v) in entries {
            if let Some((sub, subsub)) = k.split_once(':') {
                out.push_str(&format!("    {} {{\n", sub));
                out.push_str(&format!("        {} = {}\n", subsub, escape_value(v)));
                out.push_str("    }\n");
            } else {
                out.push_str(&format!("    {} = {}\n", k, escape_value(v)));
            }
        }
        out.push_str("}\n");
    }

    // Lists in defined order
    let mut ordered: Vec<&str> = Vec::new();
    for &kw in LIST_ORDER {
        if state.lists.contains_key(kw) {
            ordered.push(kw);
        }
    }
    for kw in state.lists.keys() {
        if !LIST_ORDER.contains(&kw.as_str()) {
            ordered.push(kw);
        }
    }

    for kw in ordered {
        if let Some(vals) = state.lists.get(kw) {
            if !vals.is_empty() {
                out.push('\n');
                for v in vals {
                    out.push_str(&format!("{} = {}\n", kw, escape_value(v)));
                }
            }
        }
    }

    // Extras comment if any items exist
    if !state.binds.is_empty()
        || !state.window_rules.is_empty()
        || !state.monitors.is_empty()
        || !state.autostart.is_empty()
    {
        out.push('\n');
        out.push_str(EXTRAS_MARKER);
        out.push('\n');
        let extras_json = serde_json::to_string(state).unwrap_or_default();
        out.push_str(&format!("{}{}\n", EXTRAS_PREFIX, extras_json));
    }

    out
}

pub fn source_path_text(managed_path: &Path) -> String {
    let abs = managed_path.canonicalize().unwrap_or_else(|_| managed_path.to_path_buf());
    if let Some(home) = dirs::home_dir() {
        if let Ok(rel) = abs.strip_prefix(&home) {
            return format!("~/{}", rel.display());
        }
    }
    managed_path.display().to_string()
}

fn points_to(line: &str, managed_path: &Path, config_dir: &Path) -> bool {
    let cleaned = strip_comment(line);
    if let Some(target_raw) = cleaned.strip_prefix("source") {
        if let Some(val) = target_raw.strip_prefix('=').map(|s| s.trim()) {
            let target = expand_path(val, config_dir);
            let m_canon = managed_path.canonicalize().unwrap_or_else(|_| managed_path.to_path_buf());
            let t_canon = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
            return m_canon == t_canon;
        }
    }
    false
}

pub fn with_source_line(text: &str, config_path: &Path, managed_path: &Path) -> Option<String> {
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let lines: Vec<&str> = text.lines().collect();

    let ours: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, ln)| points_to(ln, managed_path, config_dir))
        .map(|(i, _)| i)
        .collect();

    let meaningful: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, ln)| !strip_comment(ln).is_empty())
        .map(|(i, _)| i)
        .collect();

    if ours.len() == 1 && !meaningful.is_empty() && ours[0] == *meaningful.last().unwrap() {
        return None;
    }

    let mut drop = std::collections::HashSet::new();
    for &i in &ours {
        drop.insert(i);
        if i > 0 && lines[i - 1].trim() == SOURCE_MARKER {
            drop.insert(i - 1);
        }
    }

    let mut body = String::new();
    for (i, &ln) in lines.iter().enumerate() {
        if !drop.contains(&i) {
            body.push_str(ln);
            body.push('\n');
        }
    }

    while body.ends_with("\n\n") {
        body.pop();
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() {
        body.push('\n');
    }

    body.push_str(&format!(
        "{}\nsource = {}\n",
        SOURCE_MARKER,
        source_path_text(managed_path)
    ));
    Some(body)
}

pub fn without_source_line(text: &str, config_path: &Path, managed_path: &Path) -> Option<String> {
    let config_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
    let lines: Vec<&str> = text.lines().collect();

    let ours: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, ln)| points_to(ln, managed_path, config_dir))
        .map(|(i, _)| i)
        .collect();

    if ours.is_empty() {
        return None;
    }

    let mut drop = std::collections::HashSet::new();
    for &i in &ours {
        drop.insert(i);
        if i > 0 && lines[i - 1].trim() == SOURCE_MARKER {
            drop.insert(i - 1);
        }
    }

    let mut body = String::new();
    for (i, &ln) in lines.iter().enumerate() {
        if !drop.contains(&i) {
            body.push_str(ln);
            body.push('\n');
        }
    }

    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_and_read() {
        let mut state = ManagedState::default();
        state.settings.insert("general:gaps_in".into(), "5".into());
        state.settings.insert("decoration:blur:enabled".into(), "true".into());
        state.lists.entry("animation".into()).or_default().push("windows, 1, 3, default".into());

        let rendered = render(&state);
        let parsed = parse_text(&rendered, "<test>");
        let back = state_from_parsed(&parsed);

        assert_eq!(back.settings.get("general:gaps_in").unwrap(), "5");
        assert_eq!(back.settings.get("decoration:blur:enabled").unwrap(), "true");
        assert_eq!(back.lists.get("animation").unwrap()[0], "windows, 1, 3, default");
    }
}
