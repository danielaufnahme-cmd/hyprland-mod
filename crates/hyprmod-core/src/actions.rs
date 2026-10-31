//! Keybind actions: one catalog, rendered as classic dispatchers (conf) or ``hl.dsp`` calls (Lua).
//!
//! Only what rendering and validating stored binds needs is here; the editor-facing
//! parts (reverse lookup, availability) belong to the keybind page port.

use std::sync::LazyLock;

use anyhow::{bail, Result};
use regex::Regex;

use crate::lua::lua_string;
use crate::util::{repr_str, split_ws, strip};

pub const DIRECTIONS: &[(&str, &str)] = &[("left", "l"), ("right", "r"), ("up", "u"), ("down", "d")];

static NUM_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+(?:\.\d+)?$").unwrap());
static INT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+$").unwrap());
static WS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:-?\d+|[+\-]\d+|[emr][+\-]\d+|previous|empty|name:\S.*|special(?::\S.*)?)$").unwrap()
});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgKind {
    Workspace,
    Direction,
    String,
    App,
    Command,
    Int,
}

#[derive(Clone, Copy, Debug)]
pub struct ArgSpec {
    pub kind: ArgKind,
    pub label: &'static str,
    pub optional: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Action {
    pub id: &'static str,
    pub label: &'static str,
    pub category: &'static str,
    pub arg: Option<ArgSpec>,
    pub conf: &'static str,
    pub conf_arg: Option<&'static str>,
    pub lua: &'static str,
    pub lua_noarg: Option<&'static str>,
    pub conf_noarg: Option<&'static str>,
    pub default_flags: &'static str,
    pub requires: &'static [&'static str],
}

const fn arg(kind: ArgKind, label: &'static str) -> Option<ArgSpec> {
    Some(ArgSpec { kind, label, optional: false })
}

const fn opt_arg(kind: ArgKind, label: &'static str) -> Option<ArgSpec> {
    Some(ArgSpec { kind, label, optional: true })
}

const BASE: Action = Action {
    id: "",
    label: "",
    category: "",
    arg: None,
    conf: "",
    conf_arg: None,
    lua: "",
    lua_noarg: None,
    conf_noarg: None,
    default_flags: "",
    requires: &[],
};

const WS: Option<ArgSpec> = arg(ArgKind::Workspace, "Workspace (e.g. 3)");
const DIR: Option<ArgSpec> = arg(ArgKind::Direction, "Direction");

macro_rules! a {
    ($id:expr, $label:expr, $cat:expr, $conf:expr, $lua:expr $(, $f:ident = $v:expr)*) => {
        Action { id: $id, label: $label, category: $cat, conf: $conf, lua: $lua $(, $f: $v)*, ..BASE }
    };
}

pub static ACTIONS: &[Action] = &[
    // apps & commands
    a!("launch_app", "Launch app", "Apps", "exec", "hl.dsp.exec_cmd({arg})", arg = arg(ArgKind::App, "App command")),
    a!(
        "run_command",
        "Run command",
        "Apps",
        "exec",
        "hl.dsp.exec_cmd({arg})",
        arg = arg(ArgKind::Command, "Shell command")
    ),
    a!(
        "lock_screen",
        "Lock screen",
        "System",
        "exec",
        "hl.dsp.exec_cmd(\"loginctl lock-session\")",
        conf_arg = Some("loginctl lock-session")
    ),
    a!(
        "screenshot_region",
        "Screenshot area to clipboard",
        "System",
        "exec",
        "hl.dsp.exec_cmd(\"grim -g \\\"$(slurp)\\\" - | wl-copy\")",
        conf_arg = Some("grim -g \"$(slurp)\" - | wl-copy"),
        requires = &["grim", "slurp", "wl-copy"]
    ),
    a!(
        "screenshot_full",
        "Screenshot screen to clipboard",
        "System",
        "exec",
        "hl.dsp.exec_cmd(\"grim - | wl-copy\")",
        conf_arg = Some("grim - | wl-copy"),
        requires = &["grim", "wl-copy"]
    ),
    a!(
        "global_shortcut",
        "Global shortcut",
        "Apps",
        "global",
        "hl.dsp.global({arg_str})",
        arg = arg(ArgKind::String, "Shortcut name (app:action)")
    ),
    a!(
        "reload_config",
        "Reload Hyprland config",
        "System",
        "exec",
        "hl.dsp.exec_cmd(\"hyprctl reload\")",
        conf_arg = Some("hyprctl reload")
    ),
    a!("exit_hyprland", "Exit Hyprland", "System", "exit", "hl.dsp.exit()", conf_arg = Some("")),
    // window
    a!("close_window", "Close window", "Window", "killactive", "hl.dsp.window.close()", conf_arg = Some("")),
    a!("kill_window", "Force-kill window", "Window", "forcekillactive", "hl.dsp.window.kill()", conf_arg = Some("")),
    a!(
        "toggle_floating",
        "Toggle floating",
        "Window",
        "togglefloating",
        "hl.dsp.window.float({ action = \"toggle\" })",
        conf_arg = Some("")
    ),
    a!(
        "fullscreen",
        "Fullscreen",
        "Window",
        "fullscreen",
        "hl.dsp.window.fullscreen({ mode = \"fullscreen\" })",
        conf_arg = Some("0")
    ),
    a!(
        "maximize",
        "Maximize",
        "Window",
        "fullscreen",
        "hl.dsp.window.fullscreen({ mode = \"maximized\" })",
        conf_arg = Some("1")
    ),
    a!(
        "pin_window",
        "Pin window (show on all workspaces)",
        "Window",
        "pin",
        "hl.dsp.window.pin()",
        conf_arg = Some("")
    ),
    a!("pseudo_tile", "Pseudo-tile", "Window", "pseudo", "hl.dsp.window.pseudo()", conf_arg = Some("")),
    a!("center_window", "Center window", "Window", "centerwindow", "hl.dsp.window.center()", conf_arg = Some("")),
    a!(
        "toggle_split",
        "Toggle split direction",
        "Layout",
        "layoutmsg",
        "hl.dsp.layout(\"togglesplit\")",
        conf_arg = Some("togglesplit")
    ),
    a!(
        "focus_direction",
        "Focus window in direction",
        "Focus",
        "movefocus",
        "hl.dsp.focus({{ direction = {arg} }})",
        arg = DIR
    ),
    a!(
        "move_window_direction",
        "Move window in direction",
        "Window",
        "movewindow",
        "hl.dsp.window.move({{ direction = {arg} }})",
        arg = DIR
    ),
    a!(
        "swap_window_direction",
        "Swap window in direction",
        "Window",
        "swapwindow",
        "hl.dsp.window.swap({{ direction = {arg} }})",
        arg = DIR
    ),
    a!(
        "resize_window",
        "Resize window by pixels",
        "Window",
        "resizeactive",
        "hl.dsp.window.resize({{ x = {x}, y = {y}, relative = true }})",
        arg = arg(ArgKind::String, "Change in pixels, e.g. 40 0"),
        conf_arg = Some("{x} {y}")
    ),
    a!("cycle_windows", "Focus next window", "Focus", "cyclenext", "hl.dsp.window.cycle_next()", conf_arg = Some("")),
    a!(
        "cycle_windows_prev",
        "Focus previous window",
        "Focus",
        "cyclenext",
        "hl.dsp.window.cycle_next({ next = false })",
        conf_arg = Some("prev")
    ),
    a!(
        "drag_move_window",
        "Move window with mouse",
        "Mouse",
        "movewindow",
        "hl.dsp.window.drag()",
        conf_arg = Some(""),
        default_flags = "m"
    ),
    a!(
        "drag_resize_window",
        "Resize window with mouse",
        "Mouse",
        "resizewindow",
        "hl.dsp.window.resize()",
        conf_arg = Some(""),
        default_flags = "m"
    ),
    // workspaces
    a!("workspace", "Go to workspace", "Workspaces", "workspace", "hl.dsp.focus({{ workspace = {arg} }})", arg = WS),
    a!(
        "move_to_workspace",
        "Move window to workspace",
        "Workspaces",
        "movetoworkspace",
        "hl.dsp.window.move({{ workspace = {arg} }})",
        arg = WS
    ),
    a!(
        "next_workspace",
        "Next workspace",
        "Workspaces",
        "workspace",
        "hl.dsp.focus({ workspace = \"e+1\" })",
        conf_arg = Some("e+1")
    ),
    a!(
        "prev_workspace",
        "Previous workspace",
        "Workspaces",
        "workspace",
        "hl.dsp.focus({ workspace = \"e-1\" })",
        conf_arg = Some("e-1")
    ),
    a!(
        "last_workspace",
        "Back to last workspace",
        "Workspaces",
        "workspace",
        "hl.dsp.focus({ workspace = \"previous\" })",
        conf_arg = Some("previous")
    ),
    a!(
        "move_to_next_workspace",
        "Move window to next workspace",
        "Workspaces",
        "movetoworkspace",
        "hl.dsp.window.move({ workspace = \"e+1\" })",
        conf_arg = Some("e+1")
    ),
    a!(
        "move_to_prev_workspace",
        "Move window to previous workspace",
        "Workspaces",
        "movetoworkspace",
        "hl.dsp.window.move({ workspace = \"e-1\" })",
        conf_arg = Some("e-1")
    ),
    a!(
        "toggle_special",
        "Show/hide scratchpad",
        "Workspaces",
        "togglespecialworkspace",
        "hl.dsp.workspace.toggle_special({arg_str})",
        arg = opt_arg(ArgKind::String, "Scratchpad name"),
        lua_noarg = Some("hl.dsp.workspace.toggle_special()"),
        conf_noarg = Some("")
    ),
    a!(
        "move_to_special",
        "Move window to scratchpad",
        "Workspaces",
        "movetoworkspace",
        "hl.dsp.window.move({{ workspace = {special} }})",
        arg = opt_arg(ArgKind::String, "Scratchpad name"),
        conf_arg = Some("special:{arg}"),
        lua_noarg = Some("hl.dsp.window.move({ workspace = \"special\" })"),
        conf_noarg = Some("special")
    ),
    a!(
        "move_workspace_to_monitor",
        "Move workspace to monitor",
        "Workspaces",
        "movecurrentworkspacetomonitor",
        "hl.dsp.workspace.move({{ monitor = {arg_str} }})",
        arg = arg(ArgKind::String, "Monitor (e.g. DP-1)")
    ),
    // groups
    a!("toggle_group", "Toggle window group", "Groups", "togglegroup", "hl.dsp.group.toggle()", conf_arg = Some("")),
    a!(
        "group_next",
        "Next window in group",
        "Groups",
        "changegroupactive",
        "hl.dsp.group.next()",
        conf_arg = Some("f")
    ),
    a!(
        "group_prev",
        "Previous window in group",
        "Groups",
        "changegroupactive",
        "hl.dsp.group.prev()",
        conf_arg = Some("b")
    ),
    a!(
        "lock_group",
        "Lock/unlock group",
        "Groups",
        "lockactivegroup",
        "hl.dsp.group.lock_active()",
        conf_arg = Some("toggle")
    ),
    a!(
        "move_out_of_group",
        "Move window out of group",
        "Groups",
        "moveoutofgroup",
        "hl.dsp.window.move({ out_of_group = true })",
        conf_arg = Some("")
    ),
];

pub fn get(action_id: &str) -> Result<&'static Action> {
    match ACTIONS.iter().find(|a| a.id == action_id) {
        Some(a) => Ok(a),
        None => bail!("unknown action: {}", repr_str(action_id)),
    }
}

/// Validated argument text for ``action_id`` (None when it takes none).
pub fn clean_arg(action_id: &str, arg: Option<&str>) -> Result<Option<String>> {
    let action = get(action_id)?;
    let text = strip(arg.unwrap_or(""));
    if text.contains('\n') || text.contains('\r') {
        bail!("argument must be a single line");
    }
    let Some(spec) = action.arg else { return Ok(None) };
    if text.is_empty() {
        if spec.optional {
            return Ok(None);
        }
        bail!("{}: {} is required", action.label, spec.label);
    }
    match spec.kind {
        ArgKind::Direction => {
            let low = text.to_lowercase();
            let low = DIRECTIONS.iter().find(|(_, l)| *l == low).map_or(low.clone(), |(n, _)| n.to_string());
            if !DIRECTIONS.iter().any(|(n, _)| *n == low) {
                bail!("invalid direction: {} (left, right, up or down)", repr_str(text));
            }
            return Ok(Some(low));
        }
        ArgKind::Workspace if !WS_RE.is_match(text) => bail!("invalid workspace: {}", repr_str(text)),
        ArgKind::Int if !INT_RE.is_match(text) => bail!("not a whole number: {}", repr_str(text)),
        _ => {}
    }
    if action_id == "resize_window" {
        let replaced = text.replace(',', " ");
        let nums = split_ws(&replaced);
        if nums.len() != 2 || !nums.iter().all(|n| NUM_RE.is_match(n)) {
            bail!("invalid size change: {} (expected 'x y', e.g. '40 0')", repr_str(text));
        }
        return Ok(Some(nums.join(" ")));
    }
    Ok(Some(text.to_string()))
}

/// ``str.format`` for the catalog's templates (``{{``/``}}`` escapes, named fields).
fn format_template(template: &str, fields: &[(&str, &str)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
        } else if let (true, Some(end)) = (tail.starts_with('{'), tail.find('}')) {
            let name = &tail[1..end];
            out.push_str(fields.iter().find(|(k, _)| *k == name).map_or("", |(_, v)| v));
            rest = &tail[end + 1..];
        } else {
            out.push_str(&tail[..1]);
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    out
}

/// ``hl.dsp...`` expression for a bind (argument validated).
pub fn lua_call(action_id: &str, arg: Option<&str>) -> Result<String> {
    let action = get(action_id)?;
    let clean = clean_arg(action_id, arg)?;
    let Some(spec) = action.arg else { return Ok(action.lua.to_string()) };
    let Some(clean) = clean else { return Ok(action.lua_noarg.unwrap_or("").to_string()) };
    let lit = match spec.kind {
        ArgKind::Workspace | ArgKind::Int if INT_RE.is_match(&clean) => clean.clone(),
        _ => lua_string(&clean),
    };
    let arg_str = lua_string(&clean);
    let special = lua_string(&format!("special:{clean}"));
    let nums = split_ws(&clean);
    let (x, y) = if nums.len() == 2 { (nums[0], nums[1]) } else { ("", "") };
    Ok(format_template(action.lua, &[("arg", &lit), ("arg_str", &arg_str), ("special", &special), ("x", x), ("y", y)]))
}

/// ``(dispatcher, argument)`` for a classic ``bind =`` line.
pub fn conf_dispatch(action_id: &str, arg: Option<&str>) -> Result<(String, String)> {
    let action = get(action_id)?;
    let clean = clean_arg(action_id, arg)?;
    let Some(spec) = action.arg else {
        return Ok((action.conf.into(), action.conf_arg.unwrap_or("").into()));
    };
    let Some(mut clean) = clean else {
        return Ok((action.conf.into(), action.conf_noarg.unwrap_or("").into()));
    };
    if spec.kind == ArgKind::Direction {
        clean = DIRECTIONS.iter().find(|(n, _)| *n == clean).map_or(clean.clone(), |(_, l)| l.to_string());
    }
    let template = action.conf_arg.filter(|t| !t.is_empty()).unwrap_or("{arg}");
    let nums = split_ws(&clean);
    let (x, y) = (nums.first().copied().unwrap_or(""), nums.last().copied().unwrap_or(""));
    Ok((action.conf.into(), format_template(template, &[("arg", &clean), ("x", x), ("y", y)])))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forms() {
        assert_eq!(conf_dispatch("close_window", None).unwrap(), ("killactive".into(), "".into()));
        assert_eq!(conf_dispatch("focus_direction", Some("up")).unwrap(), ("movefocus".into(), "u".into()));
        assert_eq!(conf_dispatch("resize_window", Some("40, -20")).unwrap().1, "40 -20");
        assert_eq!(conf_dispatch("move_to_special", Some("magic")).unwrap().1, "special:magic");
        assert_eq!(conf_dispatch("toggle_special", None).unwrap().1, "");
        assert_eq!(lua_call("workspace", Some("3")).unwrap(), "hl.dsp.focus({ workspace = 3 })");
        assert_eq!(lua_call("workspace", Some("e+1")).unwrap(), "hl.dsp.focus({ workspace = \"e+1\" })");
        assert_eq!(lua_call("focus_direction", Some("r")).unwrap(), "hl.dsp.focus({ direction = \"right\" })");
        assert_eq!(lua_call("run_command", Some("a \"b\"")).unwrap(), "hl.dsp.exec_cmd(\"a \\\"b\\\"\")");
        assert_eq!(
            lua_call("resize_window", Some("40 -20")).unwrap(),
            "hl.dsp.window.resize({ x = 40, y = -20, relative = true })"
        );
        assert_eq!(lua_call("toggle_special", None).unwrap(), "hl.dsp.workspace.toggle_special()");
        assert!(clean_arg("workspace", Some("nope nope")).is_err());
        assert!(clean_arg("workspace", None).is_err());
        assert!(get("frobnicate").is_err());
    }
}
