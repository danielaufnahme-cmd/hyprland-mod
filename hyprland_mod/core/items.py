"""Keybinds, window rules and monitor overrides: validation and rendering (Lua and conf).

Stored shapes (inside :class:`ManagedState`):

* bind: ``{"id", "mods": [...], "key", "action", "arg", "description", "flags"}``
* window rule: ``{"id", "name", "match": {...}, "effects": {...}, "enabled"}``
* monitor: ``{output: {"mode", "position", "scale", "transform", "enabled"}}``
"""

from __future__ import annotations

import json
import re
import uuid

from . import actions
from .lua import lua_string
from .managed import EXTRAS_MARKER, EXTRAS_PREFIX, ManagedState
from .parser import escape_value

MOD_ALIASES = {
    "SUPER": "SUPER", "WIN": "SUPER", "LOGO": "SUPER", "MOD4": "SUPER", "META": "SUPER",
    "SHIFT": "SHIFT", "CTRL": "CTRL", "CONTROL": "CTRL", "ALT": "ALT", "MOD1": "ALT",
    "CAPS": "CAPS", "MOD2": "MOD2", "MOD3": "MOD3", "MOD5": "MOD5",
}
MOD_ORDER = ("SUPER", "SHIFT", "CTRL", "ALT", "CAPS", "MOD2", "MOD3", "MOD5")
# hyprctl -j binds "modmask" bits
MOD_BITS = {"SHIFT": 1, "CAPS": 2, "CTRL": 4, "ALT": 8, "MOD2": 16, "MOD3": 32, "SUPER": 64, "MOD5": 128}
# classic bind flag letter -> HL.BindOptions field
BIND_FLAGS = {"l": "locked", "e": "repeating", "r": "release", "n": "non_consuming",
              "t": "transparent", "i": "ignore_mods", "o": "long_press", "m": "mouse"}

RULE_MATCH_ALIASES = {"floating": "float", "pinned": "pin"}
# Effects whose value is text even when it looks like a number ("workspace 3", "opacity 0.9").
STRING_EFFECTS = {"opacity", "size", "min_size", "max_size", "move", "workspace", "monitor", "tag",
                  "suppress_event", "idle_inhibit", "content", "animation", "border_color",
                  "fullscreen_state", "group"}
MONITOR_TRANSFORMS = range(8)

_IDENT_RE = re.compile(r"^[a-z][a-z0-9_]*$")
_KEY_RE = re.compile(r"^[^\s,+#\"']+$")
_NUM_RE = re.compile(r"^-?\d+(?:\.\d+)?$")
_MODE_RE = re.compile(r"^(?:preferred|highres|highrr|maxwidth|\d+x\d+(?:@\d+(?:\.\d+)?)?(?:Hz)?)$")
_POS_RE = re.compile(r"^(?:auto(?:-(?:left|right|up|down|center-left|center-right|center-up|center-down))?"
                     r"|-?\d+x-?\d+)$")


def new_id(prefix: str) -> str:
    return f"{prefix}-{uuid.uuid4().hex[:8]}"


def _line(text: str, what: str, conf: bool) -> str:
    text = str(text).strip()
    if "\n" in text or "\r" in text:
        raise ValueError(f"{what} must be a single line")
    if conf and ("{" in text or "}" in text):
        raise ValueError(f"{what} must not contain braces")
    return text


# ---------------------------------------------------------------- binds

def normalize_mods(mods: object) -> list[str]:
    if isinstance(mods, str):
        mods = re.split(r"[\s+_]+", mods)
    out: set[str] = set()
    for m in mods or []:
        name = MOD_ALIASES.get(str(m).strip().upper())
        if str(m).strip() == "":
            continue
        if name is None:
            raise ValueError(f"unknown modifier: {m!r}")
        out.add(name)
    return [m for m in MOD_ORDER if m in out]


def normalize_key(key: object) -> str:
    text = str(key or "").strip()
    if not text or not _KEY_RE.match(text):
        raise ValueError(f"invalid key: {key!r}")
    return text


def combo(mods: list[str], key: str) -> tuple[frozenset[str], str]:
    """Comparable form of a key combination (letters are case-insensitive)."""
    return frozenset(mods), key.upper()


def clean_bind(data: dict, mode: str, bind_id: str | None = None) -> dict:
    """Validated, normalized bind dict. Raises ValueError."""
    if not isinstance(data, dict):
        raise ValueError("bind must be a dict")
    conf = mode == "conf"
    action_id = data.get("action")
    if not action_id:
        raise ValueError("bind needs an action")
    action = actions.get(str(action_id))
    arg = actions.clean_arg(action["id"], data.get("arg"))
    if arg is not None:
        arg = _line(arg, "argument", conf)
    flags = str(data.get("flags") if data.get("flags") is not None else action.get("default_flags", ""))
    flags = flags.strip().lower()
    bad = set(flags) - set(BIND_FLAGS)
    if bad:
        raise ValueError(f"unknown bind flags: {''.join(sorted(bad))}")
    description = _line(data.get("description") or "", "description", conf)
    if conf and "," in description:
        raise ValueError("description must not contain commas")
    return {
        "id": bind_id or str(data.get("id") or new_id("bind")),
        "mods": normalize_mods(data.get("mods")),
        "key": normalize_key(data.get("key")),
        "action": action["id"],
        "arg": arg,
        "description": description,
        "flags": "".join(f for f in BIND_FLAGS if f in flags),
    }


def bind_keys(b: dict) -> str:
    """``"SUPER + SHIFT + C"`` (Lua) form."""
    return " + ".join([*b["mods"], b["key"]])


def bind_lua(b: dict) -> str:
    opts = [f"{BIND_FLAGS[f]} = true" for f in b.get("flags", "")]
    if b.get("description"):
        opts.insert(0, f"description = {lua_string(b['description'])}")
    tail = f", {{ {', '.join(opts)} }}" if opts else ""
    return f"hl.bind({lua_string(bind_keys(b))}, {actions.lua_call(b['action'], b.get('arg'))}{tail})"


def bind_conf(b: dict) -> tuple[str, str]:
    """``(keyword, value)``, e.g. ``("bindd", "SUPER SHIFT, C, Close, killactive, ")``."""
    dispatcher, arg = actions.conf_dispatch(b["action"], b.get("arg"))
    keyword = "bind" + b.get("flags", "") + ("d" if b.get("description") else "")
    fields = [" ".join(b["mods"]), b["key"]]
    if b.get("description"):
        fields.append(b["description"])
    fields += [dispatcher, arg]
    return keyword, ", ".join(fields).rstrip()


def bind_raw(b: dict) -> str:
    dispatcher, arg = actions.conf_dispatch(b["action"], b.get("arg"))
    return f"{dispatcher} {arg}".strip()


# ---------------------------------------------------------------- window rules

def _rule_value(value: object, what: str, conf: bool) -> str | bool:
    if isinstance(value, bool):
        return value
    if isinstance(value, (int, float)):
        return str(value)
    if not isinstance(value, str):
        raise ValueError(f"{what}: value must be text, a number or true/false")
    text = _line(value, what, conf)
    if conf and "," in text:
        raise ValueError(f"{what}: value must not contain commas")
    if text == "":
        raise ValueError(f"{what}: value is empty")
    return text


def _rule_fields(data: object, what: str, conf: bool) -> dict:
    if not isinstance(data, dict) or not data:
        raise ValueError(f"window rule needs at least one {what}")
    out: dict = {}
    for k, v in data.items():
        key = str(k).strip()
        if not _IDENT_RE.match(key):
            raise ValueError(f"invalid {what} name: {k!r}")
        out[key] = _rule_value(v, f"{what} '{key}'", conf)
    return out


def clean_rule(data: dict, mode: str, rule_id: str | None = None) -> dict:
    if not isinstance(data, dict):
        raise ValueError("window rule must be a dict")
    conf = mode == "conf"
    return {
        "id": rule_id or str(data.get("id") or new_id("rule")),
        "name": _line(data.get("name") or "", "name", conf),
        "match": _rule_fields(data.get("match"), "match", conf),
        "effects": _rule_fields(data.get("effects"), "effect", conf),
        "enabled": bool(data.get("enabled", True)),
    }


def _lua_effect(key: str, value: str | bool) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if _NUM_RE.match(value) and key not in STRING_EFFECTS:
        return value
    return lua_string(value)


def rule_lua(r: dict) -> str:
    match = ", ".join(
        f"{RULE_MATCH_ALIASES.get(k, k)} = "
        f"{('true' if v else 'false') if isinstance(v, bool) else lua_string(v)}"
        for k, v in r["match"].items())
    effects = "".join(f", {k} = {_lua_effect(k, v)}" for k, v in r["effects"].items())
    name = lua_string(f"hyprland-mod-{r['id']}")
    return f"hl.window_rule({{ name = {name}, match = {{ {match} }}{effects} }})"


def rule_conf(r: dict) -> str:
    """Window rule v3 value: ``float on, size 800 600, match:class ^(kitty)$``."""
    parts = [f"{k} {('on' if v else 'off') if isinstance(v, bool) else v}" for k, v in r["effects"].items()]
    parts += [f"match:{RULE_MATCH_ALIASES.get(k, k)} {('true' if v else 'false') if isinstance(v, bool) else v}"
              for k, v in r["match"].items()]
    return ", ".join(parts)


# ---------------------------------------------------------------- monitors

def clean_monitor_name(name: object) -> str:
    text = str(name or "").strip()
    if not text or "," in text or "\n" in text or "{" in text or "}" in text or "#" in text:
        raise ValueError(f"invalid monitor name: {name!r}")
    return text


def clean_monitor(spec: dict) -> dict:
    """Full ``{"mode", "position", "scale", "transform", "enabled"}``; raises ValueError."""
    if not isinstance(spec, dict):
        raise ValueError("monitor settings must be a dict")
    mode = str(spec.get("mode") or "preferred").strip()
    if not _MODE_RE.match(mode):
        raise ValueError(f"invalid monitor mode: {mode!r} (e.g. 2560x1440@144 or preferred)")
    mode = mode[:-2] if mode.endswith("Hz") else mode
    position = str(spec.get("position") or "auto").strip()
    if not _POS_RE.match(position):
        raise ValueError(f"invalid monitor position: {position!r} (e.g. 0x0 or auto)")
    scale = str(spec.get("scale") if spec.get("scale") is not None else "auto").strip()
    if scale != "auto" and not (_NUM_RE.match(scale) and 0.1 <= float(scale) <= 10):
        raise ValueError(f"invalid monitor scale: {scale!r}")
    try:
        transform = int(spec.get("transform") or 0)
    except (TypeError, ValueError):
        raise ValueError(f"invalid monitor transform: {spec.get('transform')!r}") from None
    if transform not in MONITOR_TRANSFORMS:
        raise ValueError(f"invalid monitor transform: {transform} (0-7)")
    return {"mode": mode, "position": position, "scale": scale, "transform": transform,
            "enabled": bool(spec.get("enabled", True))}


def monitor_lua(name: str, m: dict) -> str:
    if not m.get("enabled", True):
        return f"hl.monitor({{ output = {lua_string(name)}, disabled = true }})"
    scale = m["scale"] if _NUM_RE.match(m["scale"]) else lua_string(m["scale"])
    return (f"hl.monitor({{ output = {lua_string(name)}, mode = {lua_string(m['mode'])}, "
            f"position = {lua_string(m['position'])}, scale = {scale}, transform = {int(m['transform'])} }})")


def monitor_conf(name: str, m: dict) -> str:
    if not m.get("enabled", True):
        return f"{name}, disable"
    out = f"{name}, {m['mode']}, {m['position']}, {m['scale']}"
    return out + (f", transform, {int(m['transform'])}" if int(m["transform"]) else "")


# ---------------------------------------------------------------- autostart

_AUTOSTART_ID_RE = re.compile(r"^[A-Za-z0-9_.\-]+$")


def clean_autostart_id(entry_id: object) -> str:
    text = str(entry_id or "").strip()
    if not _AUTOSTART_ID_RE.match(text):
        raise ValueError(f"invalid autostart id: {entry_id!r}")
    return text


def clean_command(command: object) -> str:
    text = _line(command if isinstance(command, str) else "", "command", conf=False)
    if not text:
        raise ValueError("command is empty")
    return text


def autostart_lua(state: ManagedState) -> str | None:
    """One ``hyprland.start`` handler running every command (next login only)."""
    if not state.autostart:
        return None
    body = " ".join(f"hl.exec_cmd({lua_string(cmd)})" for _, cmd in sorted(state.autostart.items()))
    return f'hl.on("hyprland.start", function() {body} end)'


# ---------------------------------------------------------------- whole state

def lua_calls(state: ManagedState) -> list[str]:
    """Monitors, enabled window rules, then binds."""
    out = [monitor_lua(n, m) for n, m in sorted(state.monitors.items())]
    out += [rule_lua(r) for r in state.window_rules if r.get("enabled", True)]
    out += [bind_lua(b) for b in state.binds]
    start = autostart_lua(state)
    return out + ([start] if start else [])


def conf_lines(state: ManagedState) -> list[tuple[str, str]]:
    out = [("monitor", monitor_conf(n, m)) for n, m in sorted(state.monitors.items())]
    out += [("windowrule", rule_conf(r)) for r in state.window_rules if r.get("enabled", True)]
    out += [bind_conf(b) for b in state.binds]
    out += [("exec-once", cmd) for _, cmd in sorted(state.autostart.items())]
    return out


def render_conf_extras(state: ManagedState) -> str:
    """Trailing block of the managed conf file (empty when there are no items)."""
    extras = state.extras()
    if not extras:
        return ""
    out = ["\n", EXTRAS_MARKER, "\n", EXTRAS_PREFIX, json.dumps(extras, sort_keys=True), "\n"]
    for kw, value in conf_lines(state):
        out.append(f"{kw} = {escape_value(value)}\n")
    return "".join(out)
