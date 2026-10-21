"""Hyprland color/gradient text <-> (r, g, b, a) floats."""

import re

_HEX = re.compile(r"^[0-9a-fA-F]+$")


def parse_color(text, fallback=(0.5, 0.5, 0.5, 1.0)):
    t = (text or "").strip()
    try:
        if t.startswith("0x"):
            h = t[2:]
            if len(h) == 8:
                a, r, g, b = (int(h[i:i + 2], 16) / 255 for i in range(0, 8, 2))
                return (r, g, b, a)
        m = re.match(r"^rgba?\(([^)]*)\)$", t)
        if m:
            inner = m.group(1).strip()
            if "," in inner:
                parts = [p.strip() for p in inner.split(",")]
                r, g, b = (float(p) / 255 for p in parts[:3])
                a = float(parts[3]) if len(parts) > 3 else 1.0
                return (r, g, b, a if a <= 1 else a / 255)
            if _HEX.match(inner) and len(inner) in (6, 8):
                vals = [int(inner[i:i + 2], 16) / 255 for i in range(0, len(inner), 2)]
                if len(vals) == 3:
                    vals.append(1.0)
                return tuple(vals)
        if _HEX.match(t) and len(t) in (6, 8):
            vals = [int(t[i:i + 2], 16) / 255 for i in range(0, len(t), 2)]
            if len(vals) == 3:
                vals.append(1.0)
            return tuple(vals)
    except ValueError:
        pass
    return fallback


def format_color(rgba):
    r, g, b, a = (max(0, min(255, round(c * 255))) for c in rgba)
    return f"rgba({r:02x}{g:02x}{b:02x}{a:02x})"


def parse_gradient(text):
    """Returns (list of rgba tuples, angle_degrees)."""
    colors, angle = [], 0
    for tok in re.findall(r"rgba?\([^)]*\)|0x[0-9a-fA-F]{8}|-?\d+(?:\.\d+)?deg", text or ""):
        if tok.endswith("deg"):
            angle = int(float(tok[:-3]))
        else:
            colors.append(parse_color(tok))
    if not colors:
        colors = [(0.5, 0.5, 0.5, 1.0)]
    return colors, angle


def format_gradient(colors, angle):
    s = " ".join(format_color(c) for c in colors)
    if len(colors) > 1:
        s += f" {int(angle)}deg"
    return s


def mix(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))
