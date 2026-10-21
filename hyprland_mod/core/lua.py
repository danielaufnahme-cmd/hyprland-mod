"""Lua config backend (Hyprland 0.56+): render ``hyprland-mod.lua``, the require line,
live ``hyprctl eval`` code, and a light scan of the user's curves/animations.

The user's Lua is never executed or fully parsed.
"""

from __future__ import annotations

import json
import os
import re
from pathlib import Path

from .fsutil import read_text
from .managed import LIST_ORDER, ManagedState
from .values import FALSE_WORDS, TRUE_WORDS, format_float, normalize_gradient, parse_color

LUA_HEADER = (
    "-- Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n"
    "-- Loaded from the end of hyprland.lua so these values take precedence.\n"
)
STATE_PREFIX = "-- hyprland-mod-state: "
REQUIRE_MARKER = "-- Hyprland Mod: keep this line last so its settings take precedence"
LUA_LIST_KEYWORDS = ("bezier", "animation")
DEFAULT_STUBS = "/usr/share/hypr/stubs/hl.meta.lua"

_IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")
_NUM_RE = re.compile(r"^-?(?:\d+\.?\d*|\.\d+)$")
_HEX_RE = re.compile(r"^0[xX][0-9a-fA-F]+$")
_DEG_RE = re.compile(r"^(-?\d+(?:\.\d+)?)deg$")
_LUA_KEYWORDS = {
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if",
    "in", "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
}


# ---------------------------------------------------------------- types from stubs

def load_lua_types(path: str | None = None) -> dict[str, str]:
    """``{'general.gaps_in': 'integer|HL.CssGap', ...}`` from Hyprland's API stubs (optional)."""
    try:
        text = Path(path or DEFAULT_STUBS).read_text(encoding="utf-8", errors="replace")
    except OSError:
        return {}
    return dict(re.findall(r"^---@field \['([^']+)'\]\s+(\S+)", text, re.MULTILINE))


def lua_path(key: str) -> list[str]:
    """``general:col.active_border`` -> ``['general', 'col', 'active_border']``."""
    return [p for p in re.split(r"[:.]", key) if p]


def dotted(key: str) -> str:
    return ".".join(lua_path(key))


# ---------------------------------------------------------------- literals

def lua_string(text: str) -> str:
    out = ['"']
    for ch in text:
        if ch in '\\"':
            out.append("\\" + ch)
        elif ch == "\n":
            out.append("\\n")
        elif ord(ch) < 32 or ord(ch) == 127:
            out.append(f"\\{ord(ch):03d}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def _lua_number(text: str) -> str:
    t = text.strip()
    if t.startswith("."):
        t = "0" + t
    if t.startswith("-."):
        t = "-0" + t[1:]
    return t.rstrip(".") if t.endswith(".") else t


def _gradient(tokens: list[str]) -> str | None:
    colors: list[str] = []
    angle: str | None = None
    for i, tok in enumerate(tokens):
        m = _DEG_RE.match(tok)
        if m and i == len(tokens) - 1 and colors:
            angle = m.group(1)
            continue
        color = parse_color(tok)
        if color is None:
            return None
        colors.append(color)
    if not colors:
        return None
    if len(colors) == 1 and angle is None:
        return lua_string(colors[0])
    inner = ", ".join(lua_string(c) for c in colors)
    return "{ colors = { " + inner + " }" + (f", angle = {_lua_number(angle)}" if angle else "") + " }"


def _css_gap(nums: list[str]) -> str:
    n = [_lua_number(x) for x in nums]
    if len(n) == 1:
        n = n * 4
    elif len(n) == 2:
        n = [n[0], n[1], n[0], n[1]]
    elif len(n) == 3:
        n = [n[0], n[1], n[2], n[1]]
    top, right, bottom, left = n  # CSS order
    return f"{{ top = {top}, right = {right}, bottom = {bottom}, left = {left} }}"


def _colorish(key: str, hint: str) -> bool:
    path = lua_path(key)
    return "Gradient" in hint or "col" in path or bool(path and "color" in path[-1])


def lua_literal(key: str, value: str, hint: str | None = None) -> str:
    """Convert a classic config string to a Lua literal for ``key``.

    ``hint`` is the stub type (e.g. ``boolean``, ``integer|boolean``, ``string|HL.Gradient``).
    """
    v = value.strip()
    low = v.lower()
    hint = hint or ""
    name = (lua_path(key) or [""])[-1]
    colorish = _colorish(key, hint)
    is_num = bool(_NUM_RE.match(v))
    if hint == "string":
        return lua_string((parse_color(v) if colorish else None) or v)
    if low in TRUE_WORDS | FALSE_WORDS and (not is_num or hint == "boolean"):
        if not hint or "boolean" in hint:
            return "true" if low in TRUE_WORDS else "false"
    if is_num:
        return _lua_number(v)
    if colorish:
        grad = _gradient(v.split())
        if grad:
            return grad
    if _HEX_RE.match(v):
        return v
    nums = v.replace(",", " ").split()
    if 2 <= len(nums) <= 4 and all(_NUM_RE.match(x) for x in nums):
        if "CssGap" in hint or "gaps" in name:
            return _css_gap(nums)
        if len(nums) == 2:
            return "{ " + ", ".join(_lua_number(x) for x in nums) + " }"
    return lua_string(v)


def lua_key(name: str) -> str:
    return name if _IDENT_RE.match(name) and name not in _LUA_KEYWORDS else f"[{lua_string(name)}]"


# ---------------------------------------------------------------- tables & calls

def settings_tree(settings: dict[str, str], types: dict[str, str]) -> dict:
    """Nested ``{name: subtree | literal}``. A value that collides with a section is dropped."""
    tree: dict = {}
    for key in sorted(settings):
        path = lua_path(key)
        if not path:
            continue
        node = tree
        ok = True
        for part in path[:-1]:
            child = node.setdefault(part, {})
            if not isinstance(child, dict):
                ok = False
                break
            node = child
        if ok and not isinstance(node.get(path[-1]), dict):
            node[path[-1]] = lua_literal(key, settings[key], types.get(dotted(key)))
    return tree


def render_table(tree: dict, indent: int = 1, compact: bool = False) -> str:
    if compact:
        items = [f"{lua_key(k)} = {render_table(v, compact=True) if isinstance(v, dict) else v}"
                 for k, v in tree.items()]
        return "{ " + ", ".join(items) + " }"
    pad = "    " * indent
    lines = ["{"]
    for k, v in tree.items():
        val = render_table(v, indent + 1) if isinstance(v, dict) else v
        lines.append(f"{pad}{lua_key(k)} = {val},")
    lines.append("    " * (indent - 1) + "}")
    return "\n".join(lines)


def curve_call(entry: str) -> str:
    """``"name, x0, y0, x1, y1"`` -> ``hl.curve("name", {...})``."""
    parts = [p.strip() for p in entry.split(",")]
    if len(parts) != 5 or not parts[0] or not all(_NUM_RE.match(p) for p in parts[1:]):
        raise ValueError(f"invalid bezier: {entry!r} (expected 'name, x0, y0, x1, y1')")
    x0, y0, x1, y1 = (_lua_number(p) for p in parts[1:])
    return (f"hl.curve({lua_string(parts[0])}, {{ type = \"bezier\", "
            f"points = {{ {{{x0}, {y0}}}, {{{x1}, {y1}}} }} }})")


def animation_call(entry: str) -> str:
    """``"windows, 1, 4.79, easeOutQuint, popin 87%"`` -> ``hl.animation({...})``.

    A curve written as ``spring:name`` maps to ``spring = "name"``.
    """
    parts = [p.strip() for p in entry.split(",", 4)]
    if len(parts) < 2 or not parts[0]:
        raise ValueError(f"invalid animation: {entry!r} (expected 'leaf, on, speed, curve[, style]')")
    on = parts[1].lower()
    if on not in TRUE_WORDS | FALSE_WORDS:
        raise ValueError(f"invalid animation enabled flag: {parts[1]!r}")
    fields = [f"leaf = {lua_string(parts[0])}", f"enabled = {'true' if on in TRUE_WORDS else 'false'}"]
    if len(parts) > 2 and parts[2]:
        if not _NUM_RE.match(parts[2]):
            raise ValueError(f"invalid animation speed: {parts[2]!r}")
        fields.append(f"speed = {_lua_number(parts[2])}")
    if len(parts) > 3 and parts[3]:
        curve = parts[3]
        if curve.startswith("spring:"):
            fields.append(f"spring = {lua_string(curve[7:])}")
        else:
            fields.append(f"bezier = {lua_string(curve)}")
    if len(parts) > 4 and parts[4]:
        fields.append(f"style = {lua_string(parts[4])}")
    return "hl.animation({ " + ", ".join(fields) + " })"


def list_calls(keyword: str, values: list[str]) -> list[str]:
    if keyword == "bezier":
        return [curve_call(v) for v in values]
    if keyword == "animation":
        return [animation_call(v) for v in values]
    raise ValueError(f"list '{keyword}' is not supported in Lua configs")


def render_lua(state: ManagedState, types: dict[str, str]) -> str:
    """The managed Lua file; the exact state is kept in a JSON comment for reading back."""
    out = [LUA_HEADER, STATE_PREFIX + json.dumps(state.to_dict(), sort_keys=True) + "\n"]
    tree = settings_tree(state.settings, types)
    if tree:
        out.append("\nhl.config(" + render_table(tree) + ")\n")
    for kw in LIST_ORDER:
        calls = list_calls(kw, state.lists.get(kw, []))
        if calls:
            out.append("\n" + "\n".join(calls) + "\n")
    return "".join(out)


def read_lua_state(path: str) -> ManagedState:
    if not os.path.isfile(path):
        return ManagedState()
    for line in read_text(path).splitlines():
        if line.startswith(STATE_PREFIX):
            try:
                return ManagedState.from_dict(json.loads(line[len(STATE_PREFIX):]))
            except ValueError:
                break
    return ManagedState()


def eval_code(settings: dict[str, str], lists: dict[str, list[str]], types: dict[str, str]) -> str:
    """One-line Lua chunk for ``hyprctl eval``."""
    parts: list[str] = []
    tree = settings_tree(settings, types)
    if tree:
        parts.append("hl.config(" + render_table(tree, compact=True) + ")")
    for kw in LIST_ORDER:
        parts.extend(list_calls(kw, lists.get(kw, [])))
    return "; ".join(parts)


# ---------------------------------------------------------------- require line

def _strip_lua_comments(text: str) -> list[str]:
    """Per line, the code part (``--`` line and ``--[[ ]]`` block comments removed)."""
    out: list[str] = []
    in_block = False
    for line in text.splitlines():
        code = ""
        rest = line
        while rest:
            if in_block:
                end = rest.find("]]")
                if end < 0:
                    rest = ""
                else:
                    in_block, rest = False, rest[end + 2:]
                continue
            idx = rest.find("--")
            if idx < 0:
                code += rest
                break
            code += rest[:idx]
            if rest[idx + 2: idx + 4] == "[[":
                in_block, rest = True, rest[idx + 4:]
            else:
                break
        out.append(code.strip())
    return out


def module_name(config_path: str, managed_path: str) -> str | None:
    rel = os.path.relpath(os.path.abspath(managed_path), os.path.dirname(os.path.abspath(config_path)))
    if rel.startswith("..") or not rel.endswith(".lua"):
        return None
    return rel[:-4].replace(os.sep, ".")


def require_line(config_path: str, managed_path: str) -> str:
    mod = module_name(config_path, managed_path)
    if mod:
        return f"pcall(require, {lua_string(mod)})"
    return f"pcall(dofile, {lua_string(os.path.abspath(managed_path))})"


def _loads_managed(code: str, config_path: str, managed_path: str) -> bool:
    mod = module_name(config_path, managed_path)
    names = [re.escape(mod)] if mod else []
    names.append(re.escape(os.path.abspath(managed_path)))
    target = "(?:" + "|".join(names) + ")"
    pattern = (rf"(?:pcall\s*\(\s*(?:require|dofile)\s*,\s*|(?:require|dofile)\s*\(?\s*)"
               rf"[\"']{target}[\"']\s*\)?")
    return re.fullmatch(pattern + r"\s*;?", code) is not None


def with_require_line(text: str, config_path: str, managed_path: str) -> str | None:
    """Config text with exactly one loader line at the end, or None if already so."""
    lines = text.splitlines(keepends=True)
    code = _strip_lua_comments(text)
    ours = [i for i, c in enumerate(code) if c and _loads_managed(c, config_path, managed_path)]
    meaningful = [i for i, c in enumerate(code) if c]
    if len(ours) == 1 and ours[0] == meaningful[-1]:
        return None
    drop = set(ours)
    for i in ours:
        if i > 0 and lines[i - 1].strip() == REQUIRE_MARKER:
            drop.add(i - 1)
    body = "".join(ln for i, ln in enumerate(lines) if i not in drop)
    if body and not body.endswith("\n"):
        body += "\n"
    if body and not body.endswith("\n\n"):
        body += "\n"
    return f"{body}{REQUIRE_MARKER}\n{require_line(config_path, managed_path)}\n"


# ---------------------------------------------------------------- user lists (light scan)

def _calls(text: str, name: str) -> list[str]:
    """Argument text of every ``name(...)`` call (balanced parens, strings respected)."""
    out: list[str] = []
    for m in re.finditer(rf"(?<![\w.]){re.escape(name)}\s*\(", text):
        depth, i, quote = 1, m.end(), ""
        while i < len(text) and depth:
            ch = text[i]
            if quote:
                if ch == "\\":
                    i += 1
                elif ch == quote:
                    quote = ""
            elif ch in "\"'":
                quote = ch
            elif ch == "(":
                depth += 1
            elif ch == ")":
                depth -= 1
            i += 1
        if depth == 0:
            out.append(text[m.end(): i - 1])
    return out


_FIELD_RE = re.compile(r"(\w+)\s*=\s*(\"[^\"]*\"|'[^']*'|[^,}\s]+)")


def _fields(text: str) -> dict[str, str]:
    return {k: v.strip("\"'") for k, v in _FIELD_RE.findall(text)}


def scan_lists(paths: list[str]) -> dict[str, list[str]]:
    """Classic ``bezier``/``animation`` strings from ``hl.curve``/``hl.animation`` calls."""
    result: dict[str, list[str]] = {"bezier": [], "animation": []}
    for path in paths:
        try:
            code = "\n".join(_strip_lua_comments(read_text(path)))
        except OSError:
            continue
        for args in _calls(code, "hl.curve"):
            m = re.match(r"\s*[\"']([^\"']+)[\"']\s*,(.*)$", args, re.DOTALL)
            if not m or _fields(m.group(2)).get("type", "bezier") != "bezier":
                continue
            nums = re.findall(r"-?(?:\d+\.?\d*|\.\d+)", m.group(2).split("points", 1)[-1])
            if "points" in m.group(2) and len(nums) >= 4:
                result["bezier"].append(", ".join([m.group(1), *nums[:4]]))
        for args in _calls(code, "hl.animation"):
            f = _fields(args)
            if "leaf" not in f:
                continue
            parts = [f["leaf"], "1" if f.get("enabled", "true") == "true" else "0"]
            curve = f.get("bezier") or (f"spring:{f['spring']}" if "spring" in f else "")
            if "speed" in f:
                parts.append(f["speed"])
                if curve:
                    parts.append(curve)
                    if "style" in f:
                        parts.append(f["style"])
            result["animation"].append(", ".join(parts))
    return result


def required_files(config_path: str, exclude: list[str]) -> list[str]:
    """The main Lua file plus modules it ``require``s (resolved in the config dir)."""
    base = os.path.dirname(os.path.abspath(config_path))
    skip = {os.path.realpath(p) for p in exclude}
    seen: list[str] = []
    queue = [os.path.abspath(config_path)]
    while queue:
        path = queue.pop(0)
        real = os.path.realpath(path)
        if real in skip or real in (os.path.realpath(p) for p in seen) or not os.path.isfile(path):
            continue
        seen.append(path)
        try:
            code = "\n".join(_strip_lua_comments(read_text(path)))
        except OSError:
            continue
        for mod in re.findall(r"require\s*\(?\s*[\"']([\w.\-/]+)[\"']", code):
            queue.append(os.path.join(base, mod.replace(".", os.sep) + ".lua"))
    return seen


def describe_value(name: str, current: object) -> str | None:
    """A ``hyprctl -j descriptions`` "current" value as a classic config string."""
    if current is None:
        return None
    if isinstance(current, bool):
        return "true" if current else "false"
    if isinstance(current, int) and _colorish(name, "") and current > 0xFFFFFF:
        return parse_color(f"0x{current & 0xFFFFFFFF:08x}") or str(current)
    if isinstance(current, float):
        return format_float(current)
    if isinstance(current, dict):
        if isinstance(current.get("colors"), list):
            colors = [describe_value("x:color", c) or "" for c in current["colors"]]
            angle = current.get("angle")
            return " ".join(colors + ([f"{format_num(angle)}deg"] if angle is not None else []))
        if {"x", "y"} <= current.keys():
            return f"{format_num(current['x'])} {format_num(current['y'])}"
        return json.dumps(current)
    if isinstance(current, list):
        return " ".join(describe_value(name, c) or "" for c in current)
    if isinstance(current, str):
        tokens = current.split()
        if tokens and any(parse_color(t) and not t.startswith(("rgb", "0x", "#")) for t in tokens) \
                and all(parse_color(t) or _DEG_RE.match(t) for t in tokens):
            return normalize_gradient(current)  # "595959aa 0deg" -> "rgba(595959aa) 0deg"
    return str(current)


def format_num(v: object) -> str:
    return format_float(float(v)) if isinstance(v, (int, float)) else str(v)
