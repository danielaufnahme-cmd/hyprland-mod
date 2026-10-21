"""Keybind actions: one catalog, rendered as classic dispatchers (conf) or ``hl.dsp`` calls (Lua).

Lua forms were checked against Hyprland 0.56's ``hl.meta.lua`` stubs, the bundled
default config and the argument errors in the Hyprland binary.

Each entry: ``{"id", "label", "category", "arg", "conf", "conf_arg", "lua"}``.

* ``arg`` is None or ``{"type": workspace|direction|string|app|command|int, "label",
  "optional"?}``.
* ``conf`` is the dispatcher name; ``conf_arg`` its argument template (default ``{arg}``).
* ``lua`` is a template for the ``hl.dsp`` call. ``{arg}`` is the argument as a Lua
  literal, ``{arg_str}`` always a Lua string, ``{x}``/``{y}`` the numbers of an
  ``"x y"`` argument. Optional arguments use ``lua_noarg``/``conf_noarg`` when empty.
"""

from __future__ import annotations

import re
import shutil

from .lua import lua_string

DIRECTIONS = {"left": "l", "right": "r", "up": "u", "down": "d"}
_NUM_RE = re.compile(r"^-?\d+(?:\.\d+)?$")
_INT_RE = re.compile(r"^-?\d+$")
_WS_RE = re.compile(r"^(?:-?\d+|[+\-]\d+|[emr][+\-]\d+|previous|empty|name:\S.*|special(?::\S.*)?)$")


def _a(id_, label, category, conf, lua, arg=None, conf_arg=None, **extra) -> dict:
    entry = {"id": id_, "label": label, "category": category, "arg": arg,
             "conf": conf, "conf_arg": conf_arg, "lua": lua}
    entry.update(extra)
    return entry


def _arg(type_: str, label: str, optional: bool = False) -> dict:
    out = {"type": type_, "label": label}
    if optional:
        out["optional"] = True
    return out


_WS = _arg("workspace", "Workspace (e.g. 3)")
_DIR = _arg("direction", "Direction")

ACTIONS: list[dict] = [
    # apps & commands
    _a("launch_app", "Launch app", "Apps", "exec", "hl.dsp.exec_cmd({arg})", _arg("app", "App command")),
    _a("run_command", "Run command", "Apps", "exec", "hl.dsp.exec_cmd({arg})",
       _arg("command", "Shell command")),
    _a("lock_screen", "Lock screen", "System", "exec", 'hl.dsp.exec_cmd("loginctl lock-session")',
       conf_arg="loginctl lock-session"),
    _a("screenshot_region", "Screenshot area to clipboard", "System", "exec",
       'hl.dsp.exec_cmd("grim -g \\"$(slurp)\\" - | wl-copy")',
       conf_arg='grim -g "$(slurp)" - | wl-copy', requires=["grim", "slurp", "wl-copy"]),
    _a("screenshot_full", "Screenshot screen to clipboard", "System", "exec",
       'hl.dsp.exec_cmd("grim - | wl-copy")', conf_arg="grim - | wl-copy", requires=["grim", "wl-copy"]),
    _a("global_shortcut", "Global shortcut", "Apps", "global", "hl.dsp.global({arg_str})",
       _arg("string", "Shortcut name (app:action)")),
    _a("reload_config", "Reload Hyprland config", "System", "exec", 'hl.dsp.exec_cmd("hyprctl reload")',
       conf_arg="hyprctl reload"),
    _a("exit_hyprland", "Exit Hyprland", "System", "exit", "hl.dsp.exit()", conf_arg=""),
    # window
    _a("close_window", "Close window", "Window", "killactive", "hl.dsp.window.close()", conf_arg=""),
    _a("kill_window", "Force-kill window", "Window", "forcekillactive", "hl.dsp.window.kill()", conf_arg=""),
    _a("toggle_floating", "Toggle floating", "Window", "togglefloating",
       'hl.dsp.window.float({ action = "toggle" })', conf_arg=""),
    _a("fullscreen", "Fullscreen", "Window", "fullscreen", 'hl.dsp.window.fullscreen({ mode = "fullscreen" })',
       conf_arg="0"),
    _a("maximize", "Maximize", "Window", "fullscreen", 'hl.dsp.window.fullscreen({ mode = "maximized" })',
       conf_arg="1"),
    _a("pin_window", "Pin window (show on all workspaces)", "Window", "pin", "hl.dsp.window.pin()", conf_arg=""),
    _a("pseudo_tile", "Pseudo-tile", "Window", "pseudo", "hl.dsp.window.pseudo()", conf_arg=""),
    _a("center_window", "Center window", "Window", "centerwindow", "hl.dsp.window.center()", conf_arg=""),
    _a("toggle_split", "Toggle split direction", "Layout", "layoutmsg", 'hl.dsp.layout("togglesplit")',
       conf_arg="togglesplit"),
    _a("focus_direction", "Focus window in direction", "Focus", "movefocus",
       "hl.dsp.focus({{ direction = {arg} }})", _DIR),
    _a("move_window_direction", "Move window in direction", "Window", "movewindow",
       "hl.dsp.window.move({{ direction = {arg} }})", _DIR),
    _a("swap_window_direction", "Swap window in direction", "Window", "swapwindow",
       "hl.dsp.window.swap({{ direction = {arg} }})", _DIR),
    _a("resize_window", "Resize window by pixels", "Window", "resizeactive",
       "hl.dsp.window.resize({{ x = {x}, y = {y}, relative = true }})",
       _arg("string", "Change in pixels, e.g. 40 0"), conf_arg="{x} {y}"),
    _a("cycle_windows", "Focus next window", "Focus", "cyclenext", "hl.dsp.window.cycle_next()", conf_arg=""),
    _a("cycle_windows_prev", "Focus previous window", "Focus", "cyclenext",
       "hl.dsp.window.cycle_next({ next = false })", conf_arg="prev"),
    _a("drag_move_window", "Move window with mouse", "Mouse", "movewindow", "hl.dsp.window.drag()",
       conf_arg="", default_flags="m"),
    _a("drag_resize_window", "Resize window with mouse", "Mouse", "resizewindow", "hl.dsp.window.resize()",
       conf_arg="", default_flags="m"),
    # workspaces
    _a("workspace", "Go to workspace", "Workspaces", "workspace", "hl.dsp.focus({{ workspace = {arg} }})", _WS),
    _a("move_to_workspace", "Move window to workspace", "Workspaces", "movetoworkspace",
       "hl.dsp.window.move({{ workspace = {arg} }})", _WS),
    _a("next_workspace", "Next workspace", "Workspaces", "workspace", 'hl.dsp.focus({ workspace = "e+1" })',
       conf_arg="e+1"),
    _a("prev_workspace", "Previous workspace", "Workspaces", "workspace", 'hl.dsp.focus({ workspace = "e-1" })',
       conf_arg="e-1"),
    _a("last_workspace", "Back to last workspace", "Workspaces", "workspace",
       'hl.dsp.focus({ workspace = "previous" })', conf_arg="previous"),
    _a("move_to_next_workspace", "Move window to next workspace", "Workspaces", "movetoworkspace",
       'hl.dsp.window.move({ workspace = "e+1" })', conf_arg="e+1"),
    _a("move_to_prev_workspace", "Move window to previous workspace", "Workspaces", "movetoworkspace",
       'hl.dsp.window.move({ workspace = "e-1" })', conf_arg="e-1"),
    _a("toggle_special", "Show/hide scratchpad", "Workspaces", "togglespecialworkspace",
       "hl.dsp.workspace.toggle_special({arg_str})", _arg("string", "Scratchpad name", optional=True),
       lua_noarg="hl.dsp.workspace.toggle_special()", conf_noarg=""),
    _a("move_to_special", "Move window to scratchpad", "Workspaces", "movetoworkspace",
       'hl.dsp.window.move({{ workspace = {special} }})', _arg("string", "Scratchpad name", optional=True),
       conf_arg="special:{arg}", lua_noarg='hl.dsp.window.move({ workspace = "special" })', conf_noarg="special"),
    _a("move_workspace_to_monitor", "Move workspace to monitor", "Workspaces",
       "movecurrentworkspacetomonitor", "hl.dsp.workspace.move({{ monitor = {arg_str} }})",
       _arg("string", "Monitor (e.g. DP-1)")),
    # groups
    _a("toggle_group", "Toggle window group", "Groups", "togglegroup", "hl.dsp.group.toggle()", conf_arg=""),
    _a("group_next", "Next window in group", "Groups", "changegroupactive", "hl.dsp.group.next()",
       conf_arg="f"),
    _a("group_prev", "Previous window in group", "Groups", "changegroupactive", "hl.dsp.group.prev()",
       conf_arg="b"),
    _a("lock_group", "Lock/unlock group", "Groups", "lockactivegroup", "hl.dsp.group.lock_active()",
       conf_arg="toggle"),
    _a("move_out_of_group", "Move window out of group", "Groups", "moveoutofgroup",
       "hl.dsp.window.move({ out_of_group = true })", conf_arg=""),
]

BY_ID: dict[str, dict] = {a["id"]: a for a in ACTIONS}


def get(action_id: str) -> dict:
    try:
        return BY_ID[action_id]
    except KeyError:
        raise ValueError(f"unknown action: {action_id!r}") from None


def available(action: dict) -> bool:
    """False when a helper program the action needs (e.g. grim) is missing."""
    return all(shutil.which(p) for p in action.get("requires", []))


# ---------------------------------------------------------------- arguments

def clean_arg(action_id: str, arg: object) -> str | None:
    """Validated argument text for ``action_id`` (None when it takes none). Raises ValueError."""
    action = get(action_id)
    spec = action["arg"]
    text = "" if arg is None else str(arg).strip()
    if "\n" in text or "\r" in text:
        raise ValueError("argument must be a single line")
    if spec is None:
        return None
    if not text:
        if spec.get("optional"):
            return None
        raise ValueError(f"{action['label']}: {spec['label']} is required")
    kind = spec["type"]
    if kind == "direction":
        low = text.lower()
        low = {v: k for k, v in DIRECTIONS.items()}.get(low, low)
        if low not in DIRECTIONS:
            raise ValueError(f"invalid direction: {text!r} (left, right, up or down)")
        return low
    if kind == "workspace" and not _WS_RE.match(text):
        raise ValueError(f"invalid workspace: {text!r}")
    if kind == "int" and not _INT_RE.match(text):
        raise ValueError(f"not a whole number: {text!r}")
    if action_id == "resize_window":
        nums = text.replace(",", " ").split()
        if len(nums) != 2 or not all(_NUM_RE.match(n) for n in nums):
            raise ValueError(f"invalid size change: {text!r} (expected 'x y', e.g. '40 0')")
        return " ".join(nums)
    return text


def _fields(action: dict, arg: str | None) -> dict[str, str]:
    if arg is None:
        return {}
    kind = action["arg"]["type"]
    if kind == "direction":
        lit = lua_string(arg)
    elif kind in ("workspace", "int") and _INT_RE.match(arg):
        lit = arg
    else:
        lit = lua_string(arg)
    out = {"arg": lit, "arg_str": lua_string(arg), "special": lua_string(f"special:{arg}")}
    nums = arg.split()
    if len(nums) == 2:
        out["x"], out["y"] = nums
    return out


def lua_call(action_id: str, arg: object = None) -> str:
    """``hl.dsp...`` expression for a bind (argument validated)."""
    action = get(action_id)
    clean = clean_arg(action_id, arg)
    if action["arg"] and clean is None:
        return action["lua_noarg"]
    if not action["arg"]:
        return action["lua"]
    return action["lua"].format(**_fields(action, clean))


def conf_dispatch(action_id: str, arg: object = None) -> tuple[str, str]:
    """``(dispatcher, argument)`` for a classic ``bind =`` line."""
    action = get(action_id)
    clean = clean_arg(action_id, arg)
    if action["arg"] is None:
        return action["conf"], action["conf_arg"] or ""
    if clean is None:
        return action["conf"], action.get("conf_noarg", "")
    if action["arg"]["type"] == "direction":
        clean = DIRECTIONS[clean]
    template = action["conf_arg"] or "{arg}"
    nums = clean.split()
    return action["conf"], template.format(arg=clean, x=nums[0], y=nums[-1])


def match_conf(dispatcher: str, arg: str) -> tuple[str | None, str | None]:
    """Best-effort reverse lookup of a classic dispatcher: ``(action_id, arg)``."""
    dispatcher = dispatcher.strip().lower()
    arg = arg.strip()
    ordered = [a for a in ACTIONS if a["arg"] is None] + [a for a in ACTIONS if a["arg"] is not None]
    for action in ordered:
        if action["conf"] != dispatcher:
            continue
        if action["arg"] is None:
            if (action["conf_arg"] or "") == arg:
                return action["id"], None
            continue
        if action["id"] == "launch_app":
            continue  # "run_command" is the generic reading of exec
        kind = action["arg"]["type"]
        if kind == "direction":
            back = {v: k for k, v in DIRECTIONS.items()}
            if arg.lower() in back:
                return action["id"], back[arg.lower()]
            continue
        if action["id"] == "move_to_special":
            if arg.startswith("special"):
                return action["id"], arg.partition(":")[2] or None
            continue
        if action["id"] == "toggle_special":
            return action["id"], arg or None
        try:
            return action["id"], clean_arg(action["id"], arg)
        except ValueError:
            continue
    return None, None
