"""Conversion between Python values and Hyprland config strings."""

from __future__ import annotations

import re
from typing import Any

TRUE_WORDS = {"true", "yes", "on", "1"}
FALSE_WORDS = {"false", "no", "off", "0"}

_HEX_RE = re.compile(r"^[0-9a-fA-F]+$")
_DEG_RE = re.compile(r"^-?\d+(?:\.\d+)?deg$")
_FUNC_RE = re.compile(r"^(rgba?)\(\s*(.*?)\s*\)$", re.IGNORECASE)


# ---------------------------------------------------------------- colors

def _clamp_byte(v: float) -> int:
    return max(0, min(255, int(round(v))))


def parse_color(text: str) -> str | None:
    """Normalize any Hyprland color notation to ``rgba(rrggbbaa)``; None if invalid."""
    t = text.strip()
    m = _FUNC_RE.match(t)
    if m:
        func, body = m.group(1).lower(), m.group(2)
        if "," in body:
            parts = [p.strip() for p in body.split(",")]
            try:
                nums = [float(p) for p in parts]
            except ValueError:
                return None
            if func == "rgb" and len(nums) == 3:
                r, g, b = nums
                a = 1.0
            elif func == "rgba" and len(nums) == 4:
                r, g, b, a = nums
            else:
                return None
            return _rgba(_clamp_byte(r), _clamp_byte(g), _clamp_byte(b), _clamp_byte(a * 255))
        if _HEX_RE.match(body):
            if func == "rgb" and len(body) == 6:
                return f"rgba({body.lower()}ff)"
            if func == "rgba" and len(body) == 8:
                return f"rgba({body.lower()})"
        return None
    if t.lower().startswith("0x") and _HEX_RE.match(t[2:]) and len(t) == 10:
        h = t[2:].lower()
        return f"rgba({h[2:]}{h[:2]})"  # 0xAARRGGBB
    if len(t) == 8 and _HEX_RE.match(t):  # bare rrggbbaa (hyprctl descriptions)
        return f"rgba({t.lower()})"
    if t.startswith("#") and _HEX_RE.match(t[1:]) and len(t) in (7, 9):
        h = t[1:].lower()
        return f"rgba({h}{'ff' if len(h) == 6 else ''})"
    return None


def normalize_gradient(text: str) -> str:
    """Canonical gradient text: every color token as ``rgba(rrggbbaa)``, angle kept."""
    out = []
    for tok in text.split():
        out.append(tok if _DEG_RE.match(tok) else (parse_color(tok) or tok))
    return " ".join(out)


def _parse_single_color(text: str) -> str | None:
    """A color, or a one-color gradient like ``ffffffff 0deg``."""
    color = parse_color(text)
    if color is not None:
        return color
    tokens = [t for t in text.split() if not _DEG_RE.match(t)]
    return parse_color(tokens[0]) if len(tokens) == 1 else None


def _rgba(r: int, g: int, b: int, a: int) -> str:
    return f"rgba({r:02x}{g:02x}{b:02x}{a:02x})"


def color_to_rgba_tuple(color: str) -> tuple[int, int, int, int] | None:
    """``rgba(rrggbbaa)`` (or any accepted form) -> (r, g, b, a) bytes."""
    norm = parse_color(color)
    if norm is None:
        return None
    h = norm[5:-1]
    return tuple(int(h[i:i + 2], 16) for i in range(0, 8, 2))  # type: ignore[return-value]


def _format_color(value: Any) -> str:
    if isinstance(value, str):
        norm = parse_color(value)
        if norm is None:
            raise ValueError(f"invalid color: {value!r}")
        return norm
    if isinstance(value, (tuple, list)) and len(value) in (3, 4):
        r, g, b = value[:3]
        a = value[3] if len(value) == 4 else 1.0
        if isinstance(a, float) and a <= 1.0:
            a = a * 255
        return _rgba(_clamp_byte(r), _clamp_byte(g), _clamp_byte(b), _clamp_byte(a))
    raise ValueError(f"invalid color: {value!r}")


# ---------------------------------------------------------------- numbers

def format_float(value: float, digits: int | None = None) -> str:
    text = f"{float(value):.{digits if digits is not None else 6}f}"
    if "." in text:
        text = text.rstrip("0").rstrip(".")
    return "0" if text in ("-0", "") else text


def _parse_number(text: str) -> float:
    t = text.strip()
    if " " in t:  # CSS-style gaps "20 20 20 20": the first value is representative
        t = t.split()[0]
    try:
        return float(int(t, 0))
    except ValueError:
        return float(t)


def _num_out(v: float) -> int | float:
    return int(v) if float(v).is_integer() else v


# ---------------------------------------------------------------- public API

def format_value(option: dict, py_value: Any) -> str:
    """Python value -> Hyprland config string for the option's ``type``."""
    kind = option.get("type", "string")
    if kind == "bool":
        if isinstance(py_value, str):
            parsed = _parse_bool(py_value)
            if parsed is None:
                raise ValueError(f"invalid bool: {py_value!r}")
            py_value = parsed
        return "true" if py_value else "false"
    if kind == "int":
        return str(int(round(float(py_value))))
    if kind == "float":
        return format_float(float(py_value), option.get("digits"))
    if kind in ("color", "gradient") and (py_value is None or str(py_value).strip() == ""):
        return ""  # unset / automatic
    if kind == "color":
        return _format_color(py_value)
    if kind == "vec2":
        if isinstance(py_value, str):
            py_value = _parse_vec2(py_value)
            if py_value is None:
                raise ValueError("invalid vec2")
        x, y = py_value
        return f"{format_float(x)} {format_float(y)}"
    if kind == "enum":
        if isinstance(py_value, bool):
            return "true" if py_value else "false"
        return str(py_value)
    if kind == "gradient":
        return normalize_gradient(str(py_value))
    return str(py_value)


def _parse_bool(text: str) -> bool | None:
    t = text.strip().lower()
    if t in TRUE_WORDS:
        return True
    if t in FALSE_WORDS:
        return False
    return None


def _parse_vec2(text: str) -> list[int | float] | None:
    parts = text.replace(",", " ").split()
    if len(parts) != 2:
        return None
    try:
        return [_num_out(float(p)) for p in parts]
    except ValueError:
        return None


def _default(option: dict) -> Any:
    default = option.get("default")
    if isinstance(default, str) and option.get("type") in ("bool", "int", "float", "color", "vec2"):
        try:
            parsed = _parse(option, default)
        except (ValueError, TypeError):
            parsed = None
        return default if parsed is None else parsed
    return default


def parse_value(option: dict, text: str | None) -> Any:
    """Hyprland config string -> Python value; None or unparsable -> option default."""
    if text is None:
        return _default(option)
    try:
        parsed = _parse(option, text)
    except (ValueError, TypeError):
        parsed = None
    return _default(option) if parsed is None else parsed


def _parse(option: dict, text: str) -> Any:
    kind = option.get("type", "string")
    t = text.strip()
    if kind == "bool":
        return _parse_bool(t)
    if kind == "int":
        return int(_parse_number(t))
    if kind == "float":
        return float(_parse_number(t))
    if kind == "color":
        return "" if t == "" else _parse_single_color(t)
    if kind == "vec2":
        return _parse_vec2(t)
    if kind == "enum":
        for choice in option.get("choices") or []:
            value = choice.get("value") if isinstance(choice, dict) else choice
            if str(value).lower() == t.lower() or (
                isinstance(value, bool) and _parse_bool(t) is value
            ):
                return value
        try:
            return int(t)
        except ValueError:
            return t
    if kind == "gradient":
        return normalize_gradient(t)
    return text.strip()
