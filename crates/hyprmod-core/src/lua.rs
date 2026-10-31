//! Lua config backend (Hyprland 0.56+): render ``hyprland-mod.lua``, the require line,
//! live ``hyprctl eval`` code, and a light scan of the user's curves/animations.
//!
//! The user's Lua is never executed or fully parsed.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{bail, Result};
use indexmap::IndexMap;
use regex::Regex;
use serde_json::Value as Json;

use crate::fsutil::read_text;
use crate::managed::{strip_loader, ManagedState, LIST_ORDER};
use crate::pyjson::{dumps, Dumps};
use crate::util::{abspath, path_str, realpath, relpath, repr_str, split_ws, splitlines, strip};
use crate::values::{format_float, normalize_gradient, parse_color, FALSE_WORDS, TRUE_WORDS};

pub const LUA_HEADER: &str = "-- Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n\
-- Loaded from the end of hyprland.lua so these values take precedence.\n";
pub const STATE_PREFIX: &str = "-- hyprland-mod-state: ";
pub const REQUIRE_MARKER: &str = "-- Hyprland Mod: keep this line last so its settings take precedence";
pub const LUA_LIST_KEYWORDS: &[&str] = &["bezier", "animation"];
pub const DEFAULT_STUBS: &str = "/usr/share/hypr/stubs/hl.meta.lua";

static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z_][A-Za-z0-9_]*$").unwrap());
static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?(?:\d+\.?\d*|\.\d+)$").unwrap());
static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^0[xX][0-9a-fA-F]+$").unwrap());
static DEG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(-?\d+(?:\.\d+)?)deg$").unwrap());
static STUB_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^---@field \['([^']+)'\]\s+(\S+)").unwrap());
static SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[:.]").unwrap());
static FIELD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(\w+)\s*=\s*("[^"]*"|'[^']*'|[^,}\s]+)"#).unwrap());
static CURVE_ARGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?s)^\s*["']([^"']+)["']\s*,(.*)$"#).unwrap());
static NUMS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"-?(?:\d+\.?\d*|\.\d+)").unwrap());
static REQUIRE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"require\s*\(?\s*["']([\w.\-/]+)["']"#).unwrap());
const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

pub type LuaTypes = IndexMap<String, String>;

// ---------------------------------------------------------------- types from stubs

/// ``{'general.gaps_in': 'integer|HL.CssGap', ...}`` from Hyprland's API stubs (optional).
pub fn load_lua_types(path: Option<&Path>) -> LuaTypes {
    let path = path.map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(DEFAULT_STUBS));
    let Ok(bytes) = std::fs::read(&path) else { return LuaTypes::new() };
    let text = String::from_utf8_lossy(&bytes);
    STUB_RE.captures_iter(&text).map(|c| (c[1].to_string(), c[2].to_string())).collect()
}

/// ``general:col.active_border`` -> ``["general", "col", "active_border"]``.
pub fn lua_path(key: &str) -> Vec<String> {
    SPLIT_RE.split(key).filter(|p| !p.is_empty()).map(str::to_string).collect()
}

pub fn dotted(key: &str) -> String {
    lua_path(key).join(".")
}

// ---------------------------------------------------------------- literals

pub fn lua_string(text: &str) -> String {
    let mut out = String::from("\"");
    for ch in text.chars() {
        match ch {
            '\\' | '"' => {
                out.push('\\');
                out.push(ch);
            }
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 32 || c as u32 == 127 => out.push_str(&format!("\\{:03}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn lua_number(text: &str) -> String {
    let mut t = strip(text).to_string();
    if t.starts_with('.') {
        t = format!("0{t}");
    }
    if let Some(rest) = t.strip_prefix("-.") {
        t = format!("-0.{rest}");
    }
    match t.strip_suffix('.') {
        Some(s) => s.to_string(),
        None => t,
    }
}

fn is_num(s: &str) -> bool {
    NUM_RE.is_match(s)
}

fn gradient(tokens: &[&str]) -> Option<String> {
    let mut colors: Vec<String> = Vec::new();
    let mut angle: Option<String> = None;
    for (i, tok) in tokens.iter().enumerate() {
        if let Some(m) = DEG_RE.captures(tok) {
            if i == tokens.len() - 1 && !colors.is_empty() {
                angle = Some(m[1].to_string());
                continue;
            }
        }
        colors.push(parse_color(tok)?);
    }
    if colors.is_empty() {
        return None;
    }
    if colors.len() == 1 && angle.is_none() {
        return Some(lua_string(&colors[0]));
    }
    let inner = colors.iter().map(|c| lua_string(c)).collect::<Vec<_>>().join(", ");
    let angle = match angle {
        Some(a) if !a.is_empty() => format!(", angle = {}", lua_number(&a)),
        _ => String::new(),
    };
    Some(format!("{{ colors = {{ {inner} }}{angle} }}"))
}

fn css_gap(nums: &[&str]) -> String {
    let n: Vec<String> = nums.iter().map(|x| lua_number(x)).collect();
    let n = match n.len() {
        1 => vec![n[0].clone(), n[0].clone(), n[0].clone(), n[0].clone()],
        2 => vec![n[0].clone(), n[1].clone(), n[0].clone(), n[1].clone()],
        3 => vec![n[0].clone(), n[1].clone(), n[2].clone(), n[1].clone()],
        _ => n,
    };
    format!("{{ top = {}, right = {}, bottom = {}, left = {} }}", n[0], n[1], n[2], n[3])
}

fn colorish(key: &str, hint: &str) -> bool {
    let path = lua_path(key);
    hint.contains("Gradient") || path.iter().any(|p| p == "col") || path.last().is_some_and(|l| l.contains("color"))
}

fn is_bool_word(s: &str) -> bool {
    TRUE_WORDS.contains(&s) || FALSE_WORDS.contains(&s)
}

/// Convert a classic config string to a Lua literal for ``key``.
///
/// ``hint`` is the stub type (e.g. ``boolean``, ``integer|boolean``, ``string|HL.Gradient``).
pub fn lua_literal(key: &str, value: &str, hint: Option<&str>) -> String {
    let v = strip(value);
    let low = v.to_lowercase();
    let hint = hint.unwrap_or("");
    let path = lua_path(key);
    let name = path.last().map(String::as_str).unwrap_or("");
    let color = colorish(key, hint);
    let numeric = is_num(v);
    if hint == "string" {
        let text = if color { parse_color(v) } else { None };
        return lua_string(text.as_deref().unwrap_or(v));
    }
    if is_bool_word(&low) && (!numeric || hint == "boolean") && (hint.is_empty() || hint.contains("boolean")) {
        return if TRUE_WORDS.contains(&low.as_str()) { "true" } else { "false" }.into();
    }
    if numeric {
        return lua_number(v);
    }
    if color {
        if let Some(g) = gradient(&split_ws(v)) {
            return g;
        }
    }
    if HEX_RE.is_match(v) {
        return v.to_string();
    }
    let replaced = v.replace(',', " ");
    let nums = split_ws(&replaced);
    if (2..=4).contains(&nums.len()) && nums.iter().all(|x| is_num(x)) {
        if hint.contains("CssGap") || name.contains("gaps") {
            return css_gap(&nums);
        }
        if nums.len() == 2 {
            return format!("{{ {} }}", nums.iter().map(|x| lua_number(x)).collect::<Vec<_>>().join(", "));
        }
    }
    lua_string(v)
}

pub fn lua_key(name: &str) -> String {
    if IDENT_RE.is_match(name) && !LUA_KEYWORDS.contains(&name) {
        name.to_string()
    } else {
        format!("[{}]", lua_string(name))
    }
}

// ---------------------------------------------------------------- tables & calls

#[derive(Clone, Debug)]
pub enum LuaNode {
    Table(IndexMap<String, LuaNode>),
    Literal(String),
}

/// Nested ``{name: subtree | literal}``. A value that collides with a section is dropped.
pub fn settings_tree(settings: &IndexMap<String, String>, types: &LuaTypes) -> IndexMap<String, LuaNode> {
    let mut tree: IndexMap<String, LuaNode> = IndexMap::new();
    let mut keys: Vec<&String> = settings.keys().collect();
    keys.sort();
    for key in keys {
        let path = lua_path(key);
        let Some((leaf, sections)) = path.split_last() else { continue };
        let mut node = &mut tree;
        let mut ok = true;
        for part in sections {
            if matches!(node.get(part), Some(LuaNode::Literal(_))) {
                ok = false;
                break;
            }
            node = match node.entry(part.clone()).or_insert_with(|| LuaNode::Table(IndexMap::new())) {
                LuaNode::Table(t) => t,
                LuaNode::Literal(_) => unreachable!("checked above"),
            };
        }
        if ok && !matches!(node.get(leaf), Some(LuaNode::Table(_))) {
            let lit = lua_literal(key, &settings[key], types.get(&dotted(key)).map(String::as_str));
            node.insert(leaf.clone(), LuaNode::Literal(lit));
        }
    }
    tree
}

pub fn render_table(tree: &IndexMap<String, LuaNode>, indent: usize, compact: bool) -> String {
    if compact {
        let items: Vec<String> = tree
            .iter()
            .map(|(k, v)| {
                let val = match v {
                    LuaNode::Table(t) => render_table(t, 1, true),
                    LuaNode::Literal(l) => l.clone(),
                };
                format!("{} = {val}", lua_key(k))
            })
            .collect();
        return format!("{{ {} }}", items.join(", "));
    }
    let pad = "    ".repeat(indent);
    let mut lines = vec!["{".to_string()];
    for (k, v) in tree {
        let val = match v {
            LuaNode::Table(t) => render_table(t, indent + 1, false),
            LuaNode::Literal(l) => l.clone(),
        };
        lines.push(format!("{pad}{} = {val},", lua_key(k)));
    }
    lines.push(format!("{}}}", "    ".repeat(indent.saturating_sub(1))));
    lines.join("\n")
}

/// ``"name, x0, y0, x1, y1"`` -> ``hl.curve("name", {...})``.
pub fn curve_call(entry: &str) -> Result<String> {
    let parts: Vec<&str> = entry.split(',').map(strip).collect();
    if parts.len() != 5 || parts[0].is_empty() || !parts[1..].iter().all(|p| is_num(p)) {
        bail!("invalid bezier: {} (expected 'name, x0, y0, x1, y1')", repr_str(entry));
    }
    let n: Vec<String> = parts[1..].iter().map(|p| lua_number(p)).collect();
    Ok(format!(
        "hl.curve({}, {{ type = \"bezier\", points = {{ {{{}, {}}}, {{{}, {}}} }} }})",
        lua_string(parts[0]),
        n[0],
        n[1],
        n[2],
        n[3]
    ))
}

/// ``"windows, 1, 4.79, easeOutQuint, popin 87%"`` -> ``hl.animation({...})``.
///
/// A curve written as ``spring:name`` maps to ``spring = "name"``.
pub fn animation_call(entry: &str) -> Result<String> {
    let parts: Vec<&str> = entry.splitn(5, ',').map(strip).collect();
    if parts.len() < 2 || parts[0].is_empty() {
        bail!("invalid animation: {} (expected 'leaf, on, speed, curve[, style]')", repr_str(entry));
    }
    let on = parts[1].to_lowercase();
    if !is_bool_word(&on) {
        bail!("invalid animation enabled flag: {}", repr_str(parts[1]));
    }
    let mut fields = vec![
        format!("leaf = {}", lua_string(parts[0])),
        format!("enabled = {}", if TRUE_WORDS.contains(&on.as_str()) { "true" } else { "false" }),
    ];
    if let Some(speed) = parts.get(2).filter(|s| !s.is_empty()) {
        if !is_num(speed) {
            bail!("invalid animation speed: {}", repr_str(speed));
        }
        fields.push(format!("speed = {}", lua_number(speed)));
    }
    if let Some(curve) = parts.get(3).filter(|s| !s.is_empty()) {
        match curve.strip_prefix("spring:") {
            Some(spring) => fields.push(format!("spring = {}", lua_string(spring))),
            None => fields.push(format!("bezier = {}", lua_string(curve))),
        }
    }
    if let Some(style) = parts.get(4).filter(|s| !s.is_empty()) {
        fields.push(format!("style = {}", lua_string(style)));
    }
    Ok(format!("hl.animation({{ {} }})", fields.join(", ")))
}

pub fn list_calls(keyword: &str, values: &[String]) -> Result<Vec<String>> {
    match keyword {
        "bezier" => values.iter().map(|v| curve_call(v)).collect(),
        "animation" => values.iter().map(|v| animation_call(v)).collect(),
        _ => bail!("list '{keyword}' is not supported in Lua configs"),
    }
}

/// The managed Lua file; the exact state is kept in a JSON comment for reading back.
///
/// ``item_calls`` are the rendered binds/rules/monitors (see [`crate::items::lua_calls`]).
pub fn render_lua(state: &ManagedState, types: &LuaTypes, item_calls: &[String]) -> Result<String> {
    let mut out = String::from(LUA_HEADER);
    out.push_str(STATE_PREFIX);
    out.push_str(&dumps(&state.to_dict(), Dumps::sorted()));
    out.push('\n');
    let tree = settings_tree(&state.settings, types);
    if !tree.is_empty() {
        out.push_str(&format!("\nhl.config({})\n", render_table(&tree, 1, false)));
    }
    for kw in LIST_ORDER {
        let calls = list_calls(kw, state.lists.get(*kw).map(Vec::as_slice).unwrap_or(&[]))?;
        if !calls.is_empty() {
            out.push_str(&format!("\n{}\n", calls.join("\n")));
        }
    }
    if !item_calls.is_empty() {
        out.push_str(&format!("\n{}\n", item_calls.join("\n")));
    }
    Ok(out)
}

pub fn read_lua_state(path: &Path) -> ManagedState {
    if !path.is_file() {
        return ManagedState::default();
    }
    let Ok(text) = read_text(path) else { return ManagedState::default() };
    for line in splitlines(&text, false) {
        if let Some(payload) = line.strip_prefix(STATE_PREFIX) {
            return serde_json::from_str::<Json>(payload)
                .ok()
                .and_then(|d| ManagedState::from_dict(&d).ok())
                .unwrap_or_default();
        }
    }
    ManagedState::default()
}

/// One-line Lua chunk for ``hyprctl eval``.
pub fn eval_code(
    settings: &IndexMap<String, String>,
    lists: &IndexMap<String, Vec<String>>,
    types: &LuaTypes,
) -> Result<String> {
    let mut parts = Vec::new();
    let tree = settings_tree(settings, types);
    if !tree.is_empty() {
        parts.push(format!("hl.config({})", render_table(&tree, 1, true)));
    }
    for kw in LIST_ORDER {
        parts.extend(list_calls(kw, lists.get(*kw).map(Vec::as_slice).unwrap_or(&[]))?);
    }
    Ok(parts.join("; "))
}

// ---------------------------------------------------------------- require line

/// Per line, the code part (``--`` line and ``--[[ ]]`` block comments removed).
pub fn strip_lua_comments(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    for line in splitlines(text, false) {
        let mut code = String::new();
        let mut rest = line;
        while !rest.is_empty() {
            if in_block {
                match rest.find("]]") {
                    None => rest = "",
                    Some(end) => {
                        in_block = false;
                        rest = &rest[end + 2..];
                    }
                }
                continue;
            }
            let Some(idx) = rest.find("--") else {
                code.push_str(rest);
                break;
            };
            code.push_str(&rest[..idx]);
            if rest[idx + 2..].starts_with("[[") {
                in_block = true;
                rest = &rest[idx + 4..];
            } else {
                break;
            }
        }
        out.push(strip(&code).to_string());
    }
    out
}

pub fn module_name(config_path: &Path, managed_path: &Path) -> Option<String> {
    let dir = abspath(config_path).parent().map(Path::to_path_buf).unwrap_or_else(|| "/".into());
    let rel = path_str(&relpath(&abspath(managed_path), &dir));
    if rel.starts_with("..") || !rel.ends_with(".lua") {
        return None;
    }
    Some(rel[..rel.len() - 4].replace('/', "."))
}

pub fn require_line(config_path: &Path, managed_path: &Path) -> String {
    match module_name(config_path, managed_path) {
        Some(m) => format!("pcall(require, {})", lua_string(&m)),
        None => format!("pcall(dofile, {})", lua_string(&path_str(&abspath(managed_path)))),
    }
}

fn loader_regex(config_path: &Path, managed_path: &Path) -> Regex {
    let mut names = Vec::new();
    if let Some(m) = module_name(config_path, managed_path) {
        names.push(regex::escape(&m));
    }
    names.push(regex::escape(&path_str(&abspath(managed_path))));
    let target = format!("(?:{})", names.join("|"));
    Regex::new(&format!(
        r#"^(?:(?:pcall\s*\(\s*(?:require|dofile)\s*,\s*|(?:require|dofile)\s*\(?\s*)["']{target}["']\s*\)?)\s*;?$"#
    ))
    .expect("valid loader regex")
}

fn loader_lines(code: &[String], re: &Regex) -> Vec<usize> {
    (0..code.len()).filter(|&i| !code[i].is_empty() && re.is_match(&code[i])).collect()
}

/// Config text with exactly one loader line at the end, or None if already so.
pub fn with_require_line(text: &str, config_path: &Path, managed_path: &Path) -> Option<String> {
    let lines = splitlines(text, true);
    let code = strip_lua_comments(text);
    let ours = loader_lines(&code, &loader_regex(config_path, managed_path));
    let meaningful: Vec<usize> = (0..code.len()).filter(|&i| !code[i].is_empty()).collect();
    if ours.len() == 1 && meaningful.last() == Some(&ours[0]) {
        return None;
    }
    let body = crate::managed::append_loader_body(&lines, &ours, REQUIRE_MARKER);
    Some(format!("{body}{REQUIRE_MARKER}\n{}\n", require_line(config_path, managed_path)))
}

/// Config text with our loader line removed, or None if it has none.
pub fn without_require_line(text: &str, config_path: &Path, managed_path: &Path) -> Option<String> {
    let lines = splitlines(text, true);
    let code = strip_lua_comments(text);
    let ours = loader_lines(&code, &loader_regex(config_path, managed_path));
    strip_loader(&lines, &ours, REQUIRE_MARKER)
}

// ---------------------------------------------------------------- user lists (light scan)

/// Argument text of every ``name(...)`` call (balanced parens, strings respected).
pub fn calls(text: &str, name: &str) -> Vec<String> {
    let re = Regex::new(&format!(r"{}\s*\(", regex::escape(name))).expect("valid call regex");
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    for m in re.find_iter(text) {
        if let Some(prev) = text[..m.start()].chars().next_back() {
            if prev == '.' || prev.is_alphanumeric() || prev == '_' {
                continue;
            }
        }
        let (mut depth, mut i, mut quote) = (1usize, m.end(), 0u8);
        while i < bytes.len() && depth > 0 {
            let ch = bytes[i];
            if quote != 0 {
                if ch == b'\\' {
                    i += 1;
                } else if ch == quote {
                    quote = 0;
                }
            } else if ch == b'"' || ch == b'\'' {
                quote = ch;
            } else if ch == b'(' {
                depth += 1;
            } else if ch == b')' {
                depth -= 1;
            }
            i += 1;
        }
        if depth == 0 {
            out.push(String::from_utf8_lossy(&bytes[m.end()..i - 1]).into_owned());
        }
    }
    out
}

fn fields(text: &str) -> IndexMap<String, String> {
    FIELD_RE.captures_iter(text).map(|c| (c[1].to_string(), c[2].trim_matches(['"', '\'']).to_string())).collect()
}

/// Classic ``bezier``/``animation`` strings from ``hl.curve``/``hl.animation`` calls.
pub fn scan_lists(paths: &[PathBuf]) -> IndexMap<String, Vec<String>> {
    let mut beziers = Vec::new();
    let mut animations = Vec::new();
    for path in paths {
        let Ok(text) = read_text(path) else { continue };
        let code = strip_lua_comments(&text).join("\n");
        for args in calls(&code, "hl.curve") {
            let Some(m) = CURVE_ARGS_RE.captures(&args) else { continue };
            let spec = &m[2];
            if fields(spec).get("type").map(String::as_str).unwrap_or("bezier") != "bezier" {
                continue;
            }
            let after = spec.split_once("points").map_or(spec, |(_, r)| r);
            let nums: Vec<&str> = NUMS_RE.find_iter(after).map(|n| n.as_str()).collect();
            if spec.contains("points") && nums.len() >= 4 {
                let mut parts = vec![&m[1]];
                parts.extend(&nums[..4]);
                beziers.push(parts.join(", "));
            }
        }
        for args in calls(&code, "hl.animation") {
            let f = fields(&args);
            let Some(leaf) = f.get("leaf") else { continue };
            let enabled = f.get("enabled").map(String::as_str).unwrap_or("true") == "true";
            let mut parts = vec![leaf.clone(), if enabled { "1" } else { "0" }.to_string()];
            let curve = match (f.get("bezier").filter(|b| !b.is_empty()), f.get("spring")) {
                (Some(b), _) => b.clone(),
                (None, Some(s)) => format!("spring:{s}"),
                _ => String::new(),
            };
            if let Some(speed) = f.get("speed") {
                parts.push(speed.clone());
                if !curve.is_empty() {
                    parts.push(curve);
                    if let Some(style) = f.get("style") {
                        parts.push(style.clone());
                    }
                }
            }
            animations.push(parts.join(", "));
        }
    }
    IndexMap::from([("bezier".to_string(), beziers), ("animation".to_string(), animations)])
}

/// The main Lua file plus modules it ``require``s (resolved in the config dir).
pub fn required_files(config_path: &Path, exclude: &[PathBuf]) -> Vec<PathBuf> {
    let base = abspath(config_path).parent().map(Path::to_path_buf).unwrap_or_else(|| "/".into());
    let skip: HashSet<PathBuf> = exclude.iter().map(|p| realpath(p)).collect();
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut seen_real: HashSet<PathBuf> = HashSet::new();
    let mut queue = std::collections::VecDeque::from([abspath(config_path)]);
    while let Some(path) = queue.pop_front() {
        let real = realpath(&path);
        if skip.contains(&real) || seen_real.contains(&real) || !path.is_file() {
            continue;
        }
        seen.push(path.clone());
        seen_real.insert(real);
        let Ok(text) = read_text(&path) else { continue };
        let code = strip_lua_comments(&text).join("\n");
        for m in REQUIRE_RE.captures_iter(&code) {
            queue.push_back(base.join(format!("{}.lua", m[1].replace('.', "/"))));
        }
    }
    seen
}

/// A ``hyprctl -j descriptions`` "current" value as a classic config string.
pub fn describe_value(name: &str, current: &Json) -> Option<String> {
    Some(match current {
        Json::Null => return None,
        Json::Bool(b) => if *b { "true" } else { "false" }.into(),
        Json::Number(n) if !n.is_f64() => {
            let big = n.as_i64().map(|i| i > 0xFFFFFF).unwrap_or(true);
            if colorish(name, "") && big {
                let masked = n.as_i64().map(|i| i as u64).or_else(|| n.as_u64()).unwrap_or(0) & 0xFFFF_FFFF;
                parse_color(&format!("0x{masked:08x}")).unwrap_or_else(|| n.to_string())
            } else {
                n.to_string()
            }
        }
        Json::Number(n) => format_float(n.as_f64().unwrap_or(0.0), None),
        Json::Object(obj) => {
            if let Some(Json::Array(colors)) = obj.get("colors") {
                let mut parts: Vec<String> =
                    colors.iter().map(|c| describe_value("x:color", c).unwrap_or_default()).collect();
                if let Some(angle) = obj.get("angle").filter(|a| !a.is_null()) {
                    parts.push(format!("{}deg", format_num(angle)));
                }
                parts.join(" ")
            } else if obj.contains_key("x") && obj.contains_key("y") {
                format!("{} {}", format_num(&obj["x"]), format_num(&obj["y"]))
            } else {
                dumps(current, Dumps::compact())
            }
        }
        Json::Array(items) => {
            items.iter().map(|c| describe_value(name, c).unwrap_or_default()).collect::<Vec<_>>().join(" ")
        }
        Json::String(s) => {
            let tokens = split_ws(s);
            let any_bare = tokens.iter().any(|t| {
                parse_color(t).is_some() && !(t.starts_with("rgb") || t.starts_with("0x") || t.starts_with('#'))
            });
            let all_ok = tokens.iter().all(|t| parse_color(t).is_some() || crate::values::is_deg(t));
            if !tokens.is_empty() && any_bare && all_ok {
                normalize_gradient(s) // "595959aa 0deg" -> "rgba(595959aa) 0deg"
            } else {
                s.clone()
            }
        }
    })
}

pub fn format_num(v: &Json) -> String {
    match v {
        Json::Number(n) => format_float(n.as_f64().unwrap_or(0.0), None),
        Json::Bool(b) => format_float(*b as i64 as f64, None),
        other => crate::util::py_str(other),
    }
}
