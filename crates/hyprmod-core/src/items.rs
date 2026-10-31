//! Keybinds, window rules, monitor overrides and autostart: validation and rendering (Lua and conf).
//!
//! Stored shapes (inside [`ManagedState`]), kept as JSON objects:
//!
//! * bind: ``{"id", "mods": [...], "key", "action", "arg", "description", "flags"}``
//! * window rule: ``{"id", "name", "match": {...}, "effects": {...}, "enabled"}``
//! * monitor: ``{output: {"mode", "position", "scale", "transform", "enabled"}}``

use std::sync::LazyLock;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::{json, Map, Value as Json};

use crate::actions;
use crate::lua::lua_string;
use crate::managed::{ManagedState, EXTRAS_MARKER, EXTRAS_PREFIX};
use crate::parser::escape_value;
use crate::pyjson::{dumps, Dumps};
use crate::util::{parse_float, py_repr, py_str, repr_str, strip, truthy};

pub const MOD_ORDER: &[&str] = &["SUPER", "SHIFT", "CTRL", "ALT", "CAPS", "MOD2", "MOD3", "MOD5"];
/// Classic bind flag letter -> ``HL.BindOptions`` field.
pub const BIND_FLAGS: &[(char, &str)] = &[
    ('l', "locked"),
    ('e', "repeating"),
    ('r', "release"),
    ('n', "non_consuming"),
    ('t', "transparent"),
    ('i', "ignore_mods"),
    ('o', "long_press"),
    ('m', "mouse"),
];
/// Effects whose value is text even when it looks like a number ("workspace 3", "opacity 0.9").
const STRING_EFFECTS: &[&str] = &[
    "opacity",
    "size",
    "min_size",
    "max_size",
    "move",
    "workspace",
    "monitor",
    "tag",
    "suppress_event",
    "idle_inhibit",
    "content",
    "animation",
    "border_color",
    "fullscreen_state",
    "group",
];

static IDENT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z][a-z0-9_]*$").unwrap());
static KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^[^\s,+#"']+$"#).unwrap());
static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+(?:\.\d+)?$").unwrap());
static MODE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:preferred|highres|highrr|maxwidth|\d+x\d+(?:@\d+(?:\.\d+)?)?(?:Hz)?)$").unwrap());
static POS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:auto(?:-(?:left|right|up|down|center-left|center-right|center-up|center-down))?|-?\d+x-?\d+)$")
        .unwrap()
});
static AUTOSTART_ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.\-]+$").unwrap());
static MODS_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\s+_]+").unwrap());

fn mod_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        "SUPER" | "WIN" | "LOGO" | "MOD4" | "META" => "SUPER",
        "SHIFT" => "SHIFT",
        "CTRL" | "CONTROL" => "CTRL",
        "ALT" | "MOD1" => "ALT",
        "CAPS" => "CAPS",
        "MOD2" => "MOD2",
        "MOD3" => "MOD3",
        "MOD5" => "MOD5",
        _ => return None,
    })
}

pub fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let mut x = (nanos as u64) ^ ((std::process::id() as u64) << 32) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    format!("{prefix}-{:08x}", x as u32)
}

fn line(text: &str, what: &str, conf: bool) -> Result<String> {
    let text = strip(text);
    if text.contains('\n') || text.contains('\r') {
        bail!("{what} must be a single line");
    }
    if conf && (text.contains('{') || text.contains('}')) {
        bail!("{what} must not contain braces");
    }
    Ok(text.to_string())
}

fn str_or_empty(v: Option<&Json>) -> String {
    match v {
        Some(v) if truthy(v) => py_str(v),
        _ => String::new(),
    }
}

// ---------------------------------------------------------------- binds

pub fn normalize_mods(mods: &Json) -> Result<Vec<String>> {
    let items: Vec<String> = match mods {
        Json::String(s) => MODS_SPLIT_RE.split(s).map(str::to_string).collect(),
        Json::Array(a) => a.iter().map(py_str).collect(),
        Json::Object(m) => m.keys().cloned().collect(),
        other if !truthy(other) => Vec::new(),
        other => bail!("unknown modifier: {}", py_repr(other)),
    };
    let mut out = Vec::new();
    for m in items {
        let m = strip(&m);
        if m.is_empty() {
            continue;
        }
        let name = mod_alias(&m.to_uppercase()).ok_or_else(|| anyhow!("unknown modifier: {}", repr_str(m)))?;
        if !out.contains(&name) {
            out.push(name);
        }
    }
    Ok(MOD_ORDER.iter().filter(|m| out.contains(m)).map(|m| m.to_string()).collect())
}

pub fn normalize_bind_key(key: Option<&Json>) -> Result<String> {
    let text = str_or_empty(key);
    let text = strip(&text);
    if text.is_empty() || !KEY_RE.is_match(text) {
        bail!("invalid key: {}", key.map(py_repr).unwrap_or_else(|| "None".into()));
    }
    Ok(text.to_string())
}

/// Validated, normalized bind object.
pub fn clean_bind(data: &Json, conf: bool, bind_id: Option<&str>) -> Result<Map<String, Json>> {
    let Some(data) = data.as_object() else { bail!("bind must be a dict") };
    let action_id = str_or_empty(data.get("action"));
    if action_id.is_empty() {
        bail!("bind needs an action");
    }
    let action = actions::get(&action_id)?;
    let raw_arg = data.get("arg").filter(|a| !a.is_null()).map(py_str);
    let arg = actions::clean_arg(action.id, raw_arg.as_deref())?.map(|a| line(&a, "argument", conf)).transpose()?;
    let flags = match data.get("flags") {
        Some(f) if !f.is_null() => py_str(f),
        _ => action.default_flags.to_string(),
    };
    let flags = strip(&flags).to_lowercase();
    let mut bad: Vec<char> = flags.chars().filter(|c| !BIND_FLAGS.iter().any(|(f, _)| f == c)).collect();
    if !bad.is_empty() {
        bad.sort();
        bad.dedup();
        bail!("unknown bind flags: {}", bad.into_iter().collect::<String>());
    }
    let description = line(&str_or_empty(data.get("description")), "description", conf)?;
    if conf && description.contains(',') {
        bail!("description must not contain commas");
    }
    let id = match bind_id {
        Some(id) => id.to_string(),
        None => match data.get("id") {
            Some(v) if truthy(v) => py_str(v),
            _ => new_id("bind"),
        },
    };
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("mods".into(), json!(normalize_mods(data.get("mods").unwrap_or(&Json::Null))?));
    out.insert("key".into(), json!(normalize_bind_key(data.get("key"))?));
    out.insert("action".into(), json!(action.id));
    out.insert("arg".into(), arg.map_or(Json::Null, Json::String));
    out.insert("description".into(), json!(description));
    out.insert(
        "flags".into(),
        json!(BIND_FLAGS.iter().filter(|(f, _)| flags.contains(*f)).map(|(f, _)| *f).collect::<String>()),
    );
    Ok(out)
}

fn s<'a>(m: &'a Map<String, Json>, k: &str) -> &'a str {
    m.get(k).and_then(Json::as_str).unwrap_or("")
}

fn mods_of(b: &Map<String, Json>) -> Vec<String> {
    b.get("mods").and_then(Json::as_array).map(|a| a.iter().map(py_str).collect()).unwrap_or_default()
}

fn arg_of(b: &Map<String, Json>) -> Option<String> {
    b.get("arg").filter(|a| !a.is_null()).map(py_str)
}

/// ``"SUPER + SHIFT + C"`` (Lua) form.
pub fn bind_keys(b: &Map<String, Json>) -> String {
    let mut parts = mods_of(b);
    parts.push(s(b, "key").to_string());
    parts.join(" + ")
}

pub fn bind_lua(b: &Map<String, Json>) -> Result<String> {
    let mut opts: Vec<String> = Vec::new();
    for c in s(b, "flags").chars() {
        let field = BIND_FLAGS.iter().find(|(f, _)| *f == c).map(|(_, n)| *n).ok_or_else(|| anyhow!("bad flag {c}"))?;
        opts.push(format!("{field} = true"));
    }
    let description = s(b, "description");
    if !description.is_empty() {
        opts.insert(0, format!("description = {}", lua_string(description)));
    }
    let tail = if opts.is_empty() { String::new() } else { format!(", {{ {} }}", opts.join(", ")) };
    let call = actions::lua_call(s(b, "action"), arg_of(b).as_deref())?;
    Ok(format!("hl.bind({}, {call}{tail})", lua_string(&bind_keys(b))))
}

/// ``(keyword, value)``, e.g. ``("bindd", "SUPER SHIFT, C, Close, killactive, ")``.
pub fn bind_conf(b: &Map<String, Json>) -> Result<(String, String)> {
    let (dispatcher, arg) = actions::conf_dispatch(s(b, "action"), arg_of(b).as_deref())?;
    let description = s(b, "description");
    let keyword = format!("bind{}{}", s(b, "flags"), if description.is_empty() { "" } else { "d" });
    let mut fields = vec![mods_of(b).join(" "), s(b, "key").to_string()];
    if !description.is_empty() {
        fields.push(description.to_string());
    }
    fields.push(dispatcher);
    fields.push(arg);
    Ok((keyword, fields.join(", ").trim_end_matches(crate::util::is_space).to_string()))
}

pub fn bind_raw(b: &Map<String, Json>) -> Result<String> {
    let (dispatcher, arg) = actions::conf_dispatch(s(b, "action"), arg_of(b).as_deref())?;
    Ok(strip(&format!("{dispatcher} {arg}")).to_string())
}

// ---------------------------------------------------------------- window rules

fn rule_value(value: &Json, what: &str, conf: bool) -> Result<Json> {
    match value {
        Json::Bool(_) => Ok(value.clone()),
        Json::Number(_) => Ok(Json::String(py_str(value))),
        Json::String(text) => {
            let text = line(text, what, conf)?;
            if conf && text.contains(',') {
                bail!("{what}: value must not contain commas");
            }
            if text.is_empty() {
                bail!("{what}: value is empty");
            }
            Ok(Json::String(text))
        }
        _ => bail!("{what}: value must be text, a number or true/false"),
    }
}

fn rule_fields(data: Option<&Json>, what: &str, conf: bool) -> Result<Map<String, Json>> {
    let Some(obj) = data.and_then(Json::as_object).filter(|o| !o.is_empty()) else {
        bail!("window rule needs at least one {what}");
    };
    let mut out = Map::new();
    for (k, v) in obj {
        let key = strip(k);
        if !IDENT_RE.is_match(key) {
            bail!("invalid {what} name: {}", repr_str(k));
        }
        out.insert(key.to_string(), rule_value(v, &format!("{what} '{key}'"), conf)?);
    }
    Ok(out)
}

pub fn clean_rule(data: &Json, conf: bool, rule_id: Option<&str>) -> Result<Map<String, Json>> {
    let Some(obj) = data.as_object() else { bail!("window rule must be a dict") };
    let id = match rule_id {
        Some(id) => id.to_string(),
        None => match obj.get("id") {
            Some(v) if truthy(v) => py_str(v),
            _ => new_id("rule"),
        },
    };
    let mut out = Map::new();
    out.insert("id".into(), json!(id));
    out.insert("name".into(), json!(line(&str_or_empty(obj.get("name")), "name", conf)?));
    out.insert("match".into(), Json::Object(rule_fields(obj.get("match"), "match", conf)?));
    out.insert("effects".into(), Json::Object(rule_fields(obj.get("effects"), "effect", conf)?));
    out.insert("enabled".into(), json!(obj.get("enabled").is_none_or(truthy)));
    Ok(out)
}

fn match_alias(k: &str) -> &str {
    match k {
        "floating" => "float",
        "pinned" => "pin",
        other => other,
    }
}

fn lua_effect(key: &str, value: &Json) -> String {
    match value {
        Json::Bool(b) => if *b { "true" } else { "false" }.into(),
        other => {
            let text = py_str(other);
            if NUM_RE.is_match(&text) && !STRING_EFFECTS.contains(&key) {
                text
            } else {
                lua_string(&text)
            }
        }
    }
}

fn empty_map() -> &'static Map<String, Json> {
    static EMPTY: LazyLock<Map<String, Json>> = LazyLock::new(Map::new);
    &EMPTY
}

fn obj<'a>(m: &'a Map<String, Json>, k: &str) -> &'a Map<String, Json> {
    m.get(k).and_then(Json::as_object).unwrap_or_else(|| empty_map())
}

pub fn rule_lua(r: &Map<String, Json>) -> String {
    let matches: Vec<String> = obj(r, "match")
        .iter()
        .map(|(k, v)| {
            let val = match v {
                Json::Bool(b) => if *b { "true" } else { "false" }.to_string(),
                other => lua_string(&py_str(other)),
            };
            format!("{} = {val}", match_alias(k))
        })
        .collect();
    let effects: String = obj(r, "effects").iter().map(|(k, v)| format!(", {k} = {}", lua_effect(k, v))).collect();
    let name = lua_string(&format!("hyprland-mod-{}", py_str(r.get("id").unwrap_or(&Json::Null))));
    format!("hl.window_rule({{ name = {name}, match = {{ {} }}{effects} }})", matches.join(", "))
}

/// Window rule v3 value: ``float on, size 800 600, match:class ^(kitty)$``.
pub fn rule_conf(r: &Map<String, Json>) -> String {
    let mut parts: Vec<String> = obj(r, "effects")
        .iter()
        .map(|(k, v)| match v {
            Json::Bool(b) => format!("{k} {}", if *b { "on" } else { "off" }),
            other => format!("{k} {}", py_str(other)),
        })
        .collect();
    parts.extend(obj(r, "match").iter().map(|(k, v)| match v {
        Json::Bool(b) => format!("match:{} {}", match_alias(k), if *b { "true" } else { "false" }),
        other => format!("match:{} {}", match_alias(k), py_str(other)),
    }));
    parts.join(", ")
}

fn enabled(m: &Map<String, Json>) -> bool {
    m.get("enabled").is_none_or(truthy)
}

// ---------------------------------------------------------------- monitors

pub fn clean_monitor_name(name: &str) -> Result<String> {
    let text = strip(name);
    if text.is_empty() || text.contains([',', '\n', '{', '}', '#']) {
        bail!("invalid monitor name: {}", repr_str(name));
    }
    Ok(text.to_string())
}

/// Full ``{"mode", "position", "scale", "transform", "enabled"}``.
pub fn clean_monitor(spec: &Json) -> Result<Map<String, Json>> {
    let Some(spec) = spec.as_object() else { bail!("monitor settings must be a dict") };
    let mode = match spec.get("mode") {
        Some(v) if truthy(v) => py_str(v),
        _ => "preferred".into(),
    };
    let mode = strip(&mode).to_string();
    if !MODE_RE.is_match(&mode) {
        bail!("invalid monitor mode: {} (e.g. 2560x1440@144 or preferred)", repr_str(&mode));
    }
    let mode = mode.strip_suffix("Hz").map(str::to_string).unwrap_or(mode);
    let position = match spec.get("position") {
        Some(v) if truthy(v) => py_str(v),
        _ => "auto".into(),
    };
    let position = strip(&position).to_string();
    if !POS_RE.is_match(&position) {
        bail!("invalid monitor position: {} (e.g. 0x0 or auto)", repr_str(&position));
    }
    let scale = match spec.get("scale") {
        Some(v) if !v.is_null() => py_str(v),
        _ => "auto".into(),
    };
    let scale = strip(&scale).to_string();
    let scale_ok =
        scale == "auto" || (NUM_RE.is_match(&scale) && parse_float(&scale).is_some_and(|f| (0.1..=10.0).contains(&f)));
    if !scale_ok {
        bail!("invalid monitor scale: {}", repr_str(&scale));
    }
    let raw_t = spec.get("transform").cloned().unwrap_or(Json::Null);
    let transform: i64 = match &raw_t {
        v if !truthy(v) => 0,
        Json::Bool(b) => *b as i64,
        Json::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)).unwrap_or(0),
        Json::String(t) => {
            crate::util::parse_int(t).ok_or_else(|| anyhow!("invalid monitor transform: {}", py_repr(&raw_t)))?
        }
        other => bail!("invalid monitor transform: {}", py_repr(other)),
    };
    if !(0..8).contains(&transform) {
        bail!("invalid monitor transform: {transform} (0-7)");
    }
    let mut out = Map::new();
    out.insert("mode".into(), json!(mode));
    out.insert("position".into(), json!(position));
    out.insert("scale".into(), json!(scale));
    out.insert("transform".into(), json!(transform));
    out.insert("enabled".into(), json!(enabled(spec)));
    Ok(out)
}

fn transform_of(m: &Map<String, Json>) -> i64 {
    match m.get("transform") {
        Some(Json::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Json::String(s)) => crate::util::parse_int(s).unwrap_or(0),
        Some(Json::Bool(b)) => *b as i64,
        _ => 0,
    }
}

pub fn monitor_lua(name: &str, m: &Map<String, Json>) -> String {
    if !enabled(m) {
        return format!("hl.monitor({{ output = {}, disabled = true }})", lua_string(name));
    }
    let scale_text = py_str(m.get("scale").unwrap_or(&Json::Null));
    let scale = if NUM_RE.is_match(&scale_text) { scale_text } else { lua_string(&scale_text) };
    format!(
        "hl.monitor({{ output = {}, mode = {}, position = {}, scale = {scale}, transform = {} }})",
        lua_string(name),
        lua_string(&py_str(m.get("mode").unwrap_or(&Json::Null))),
        lua_string(&py_str(m.get("position").unwrap_or(&Json::Null))),
        transform_of(m)
    )
}

pub fn monitor_conf(name: &str, m: &Map<String, Json>) -> String {
    if !enabled(m) {
        return format!("{name}, disable");
    }
    let field = |k: &str| py_str(m.get(k).unwrap_or(&Json::Null));
    let out = format!("{name}, {}, {}, {}", field("mode"), field("position"), field("scale"));
    match transform_of(m) {
        0 => out,
        t => format!("{out}, transform, {t}"),
    }
}

// ---------------------------------------------------------------- autostart

pub fn clean_autostart_id(entry_id: &str) -> Result<String> {
    let text = strip(entry_id);
    if !AUTOSTART_ID_RE.is_match(text) {
        bail!("invalid autostart id: {}", repr_str(entry_id));
    }
    Ok(text.to_string())
}

pub fn clean_command(command: &str) -> Result<String> {
    let text = line(command, "command", false)?;
    if text.is_empty() {
        bail!("command is empty");
    }
    Ok(text)
}

fn sorted_autostart(state: &ManagedState) -> Vec<(&String, &String)> {
    let mut entries: Vec<_> = state.autostart.iter().collect();
    entries.sort();
    entries
}

/// One ``hyprland.start`` handler running every command (next login only).
pub fn autostart_lua(state: &ManagedState) -> Option<String> {
    if state.autostart.is_empty() {
        return None;
    }
    let body: Vec<String> =
        sorted_autostart(state).into_iter().map(|(_, cmd)| format!("hl.exec_cmd({})", lua_string(cmd))).collect();
    Some(format!("hl.on(\"hyprland.start\", function() {} end)", body.join(" ")))
}

// ---------------------------------------------------------------- whole state

fn sorted_monitors(state: &ManagedState) -> Vec<(&String, &Map<String, Json>)> {
    let mut entries: Vec<_> = state.monitors.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    entries
}

/// Monitors, enabled window rules, binds, then the autostart handler.
pub fn lua_calls(state: &ManagedState) -> Result<Vec<String>> {
    let mut out: Vec<String> = sorted_monitors(state).into_iter().map(|(n, m)| monitor_lua(n, m)).collect();
    out.extend(state.window_rules.iter().filter(|r| enabled(r)).map(rule_lua));
    for b in &state.binds {
        out.push(bind_lua(b)?);
    }
    out.extend(autostart_lua(state));
    Ok(out)
}

pub fn conf_lines(state: &ManagedState) -> Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> =
        sorted_monitors(state).into_iter().map(|(n, m)| ("monitor".into(), monitor_conf(n, m))).collect();
    out.extend(state.window_rules.iter().filter(|r| enabled(r)).map(|r| ("windowrule".into(), rule_conf(r))));
    for b in &state.binds {
        out.push(bind_conf(b)?);
    }
    out.extend(sorted_autostart(state).into_iter().map(|(_, cmd)| ("exec-once".to_string(), cmd.clone())));
    Ok(out)
}

/// Trailing block of the managed conf file (empty when there are no items).
pub fn render_conf_extras(state: &ManagedState) -> Result<String> {
    let extras = state.extras();
    if extras.as_object().is_none_or(Map::is_empty) {
        return Ok(String::new());
    }
    let mut out = format!("\n{EXTRAS_MARKER}\n{EXTRAS_PREFIX}{}\n", dumps(&extras, Dumps::sorted()));
    for (kw, value) in conf_lines(state)? {
        out.push_str(&format!("{kw} = {}\n", escape_value(&value)));
    }
    Ok(out)
}

/// Items to add live (as a partial state) and whether a reload is needed.
///
/// A bind or window rule that was removed or edited needs a reload (Hyprland would keep
/// the old one); a monitor override can simply be re-sent, but dropping one needs a reload.
pub fn item_changes(old: &ManagedState, new: &ManagedState) -> (Option<ManagedState>, bool) {
    let mut reload = false;
    let mut add = ManagedState::default();
    let by_id = |items: &[Map<String, Json>]| -> indexmap::IndexMap<String, Map<String, Json>> {
        items.iter().filter(|i| enabled(i)).map(|i| (py_str(i.get("id").unwrap_or(&Json::Null)), i.clone())).collect()
    };
    for (before, after, target) in [
        (by_id(&old.binds), by_id(&new.binds), &mut add.binds),
        (by_id(&old.window_rules), by_id(&new.window_rules), &mut add.window_rules),
    ] {
        if before.iter().any(|(k, v)| after.get(k) != Some(v)) {
            reload = true;
        }
        *target = after.into_iter().filter(|(k, _)| !before.contains_key(k)).map(|(_, v)| v).collect();
    }
    if old.monitors.keys().any(|k| !new.monitors.contains_key(k)) {
        reload = true;
    }
    add.monitors = new
        .monitors
        .iter()
        .filter(|(k, v)| old.monitors.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    (if add.is_empty() { None } else { Some(add) }, reload)
}
