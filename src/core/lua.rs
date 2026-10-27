//! Lua configuration backend for Hyprland 0.56+.

use std::path::Path;

use super::fsutil::read_text;
use super::managed::ManagedState;
use super::values::{format_float, normalize_gradient, parse_color};

pub const LUA_HEADER: &str = "-- Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n-- Loaded from the end of hyprland.lua so these values take precedence.\n";
pub const STATE_PREFIX: &str = "-- hyprland-mod-state: ";
pub const REQUIRE_MARKER: &str = "-- Hyprland Mod: keep this line last so its settings take precedence";



pub fn lua_path(key: &str) -> Vec<String> {
    key.split(|c| c == ':' || c == '.')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}

pub fn dotted(key: &str) -> String {
    lua_path(key).join(".")
}

pub fn lua_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

pub fn render_lua_value(val: &str) -> String {
    let trimmed = val.trim();
    if trimmed == "true" || trimmed == "false" {
        return trimmed.to_string();
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return i.to_string();
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        return format_float(f);
    }
    if let Some(col) = parse_color(trimmed) {
        return lua_string(&col);
    }
    if trimmed.contains("deg") {
        let norm = normalize_gradient(trimmed);
        return lua_string(&norm);
    }
    lua_string(trimmed)
}

pub fn render_lua(state: &ManagedState) -> String {
    let mut out = String::from(LUA_HEADER);
    let state_json = serde_json::to_string(state).unwrap_or_default();
    out.push_str(&format!("{}{}\n\n", STATE_PREFIX, state_json));
    out.push_str("local mod = {}\n\n");

    for (k, v) in &state.settings {
        let dot = dotted(k);
        let val_lua = render_lua_value(v);
        out.push_str(&format!("mod[\"{}\"] = {}\n", dot, val_lua));
    }

    out.push_str("\nreturn mod\n");
    out
}

pub fn read_lua_managed<P: AsRef<Path>>(path: P) -> ManagedState {
    let path = path.as_ref();
    if !path.is_file() {
        return ManagedState::default();
    }
    let text = match read_text(path) {
        Ok(t) => t,
        Err(_) => return ManagedState::default(),
    };

    for line in text.lines() {
        if let Some(json_str) = line.strip_prefix(STATE_PREFIX) {
            if let Ok(state) = serde_json::from_str::<ManagedState>(json_str) {
                return state;
            }
        }
    }
    ManagedState::default()
}

pub fn with_require_line(text: &str) -> Option<String> {
    let require_str = "pcall(require, \"hyprland-mod\")";
    if text.contains(require_str) {
        return None;
    }

    let mut body = text.to_string();
    while body.ends_with("\n\n") {
        body.pop();
    }
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    if !body.is_empty() {
        body.push('\n');
    }

    body.push_str(&format!("{}\n{}\n", REQUIRE_MARKER, require_str));
    Some(body)
}

pub fn without_require_line(text: &str) -> Option<String> {
    let require_str = "pcall(require, \"hyprland-mod\")";
    if !text.contains(require_str) {
        return None;
    }

    let lines: Vec<&str> = text.lines().collect();
    let mut drop = std::collections::HashSet::new();

    for (i, &ln) in lines.iter().enumerate() {
        if ln.contains(require_str) {
            drop.insert(i);
            if i > 0 && lines[i - 1].trim() == REQUIRE_MARKER {
                drop.insert(i - 1);
            }
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
