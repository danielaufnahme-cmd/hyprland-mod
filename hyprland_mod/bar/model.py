"""Bar design model: validation, presets, save/load and ``.hyprmod-bar`` sharing.

A bar design is plain data (``Bar``). Everything coming from a file goes
through ``validate_bar`` before it is used, and the generator validates again,
so colors, fonts and numbers that reach style.css can never smuggle in CSS.
"""

from __future__ import annotations

import copy
import json
import os
import re
from dataclasses import dataclass, field
from pathlib import Path

from ..core.fsutil import atomic_write
from .modules import MODULES, option_spec

BAR_FORMAT = "hyprland-mod-bar"
FORMAT_VERSION = 1
BAR_EXT = ".hyprmod-bar"
MAX_FILE_BYTES = 256 * 1024
BUILTIN_DIR = Path(__file__).resolve().parent.parent.parent / "data" / "bars"
ZONES = ("left", "center", "right")
ZONE_LABELS = {"left": "Left", "center": "Center", "right": "Right"}
POSITIONS = ("top", "bottom")
STYLES = ("solid", "islands", "pills")
WEIGHTS = ("normal", "medium", "bold")
MAX_MODULES = 24

_HEX_RE = re.compile(r"^#[0-9a-fA-F]{6}$")
# A font family name: letters, digits, spaces and a few separators — no quotes,
# semicolons, braces or backslashes, so it can't break out of the CSS string.
_FONT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9 _.+\-]{0,63}$")
_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,47}$")
_SAFE_RE = re.compile(r"^[^\x00-\x1f\x7f]*$")

# name -> (kind, default, extra); ints carry (min, max)
FIELDS = {
    "position": ("choice", "top", POSITIONS),
    "style": ("choice", "solid", STYLES),
    "floating": ("bool", False, None),
    "height": ("int", 32, (20, 72)),
    "margin": ("int", 8, (0, 48)),
    "radius": ("int", 12, (0, 36)),
    "spacing": ("int", 4, (0, 32)),
    "padding": ("int", 10, (0, 32)),
    "border": ("int", 0, (0, 4)),
    "font": ("font", "Adwaita Sans", None),
    "font_size": ("int", 13, (8, 28)),
    "font_weight": ("choice", "medium", WEIGHTS),
    "background": ("color", "#16181d", None),
    "foreground": ("color", "#e8e8ea", None),
    "accent": ("color", "#3584e4", None),
    "opacity": ("float", 0.9, (0.0, 1.0)),
    "icons": ("bool", True, None),
}


class BarError(ValueError):
    """A bar design or file was rejected; ``str(err)`` is shown to the user."""


def _default_modules():
    return {"left": [{"type": "workspaces", "options": {}}],
            "center": [{"type": "clock", "options": {}}],
            "right": [{"type": "pulseaudio", "options": {}}, {"type": "network", "options": {}},
                      {"type": "battery", "options": {}}, {"type": "tray", "options": {}}]}


@dataclass
class Bar:
    position: str = "top"
    style: str = "solid"
    floating: bool = False
    height: int = 32
    margin: int = 8
    radius: int = 12
    spacing: int = 4
    padding: int = 10
    border: int = 0
    font: str = "Adwaita Sans"
    font_size: int = 13
    font_weight: str = "medium"
    background: str = "#16181d"
    foreground: str = "#e8e8ea"
    accent: str = "#3584e4"
    opacity: float = 0.9
    icons: bool = True
    modules: dict = field(default_factory=_default_modules)

    def to_dict(self) -> dict:
        d = {name: getattr(self, name) for name in FIELDS}
        d["modules"] = {z: [{"type": m["type"], "options": dict(m.get("options") or {})}
                            for m in self.modules.get(z, [])] for z in ZONES}
        return d

    def copy(self) -> "Bar":
        return Bar(**{**{n: getattr(self, n) for n in FIELDS}, "modules": copy.deepcopy(self.modules)})

    def __eq__(self, other) -> bool:
        return isinstance(other, Bar) and self.to_dict() == other.to_dict()

    # -- module editing (raises BarError on nonsense, never leaves a half edit) --

    def types(self) -> list[str]:
        return [m["type"] for z in ZONES for m in self.modules.get(z, [])]

    def find(self, mtype: str) -> tuple[str, int] | None:
        for z in ZONES:
            for i, m in enumerate(self.modules.get(z, [])):
                if m["type"] == mtype:
                    return z, i
        return None

    def add_module(self, mtype: str, zone: str, index: int | None = None, options: dict | None = None) -> None:
        if mtype not in MODULES:
            raise BarError(f"Unknown module “{mtype}”.")
        if zone not in ZONES:
            raise BarError(f"Unknown place “{zone}”.")
        if self.find(mtype):
            raise BarError(f"{MODULES[mtype]['label']} is already on the bar.")
        if len(self.types()) >= MAX_MODULES:
            raise BarError("That's a lot of modules — remove one first.")
        opts = _validate_options(mtype, options or {})
        lst = self.modules.setdefault(zone, [])
        index = len(lst) if index is None else max(0, min(index, len(lst)))
        lst.insert(index, {"type": mtype, "options": opts})

    def remove_module(self, mtype: str) -> dict:
        where = self.find(mtype)
        if not where:
            raise BarError(f"{MODULES.get(mtype, {}).get('label', mtype)} isn't on the bar.")
        return self.modules[where[0]].pop(where[1])

    def move_module(self, mtype: str, zone: str, index: int) -> None:
        """Move to ``zone`` so that it ends up at ``index`` there (after removal)."""
        if zone not in ZONES:
            raise BarError(f"Unknown place “{zone}”.")
        entry = self.remove_module(mtype)
        lst = self.modules.setdefault(zone, [])
        lst.insert(max(0, min(index, len(lst))), entry)

    def shift_module(self, mtype: str, delta: int) -> bool:
        """Move one step left/right inside its zone; False at the edge."""
        where = self.find(mtype)
        if not where:
            return False
        zone, i = where
        j = i + delta
        lst = self.modules[zone]
        if not 0 <= j < len(lst):
            return False
        lst[i], lst[j] = lst[j], lst[i]
        return True

    def set_option(self, mtype: str, key: str, value) -> None:
        where = self.find(mtype)
        if not where:
            raise BarError(f"{MODULES.get(mtype, {}).get('label', mtype)} isn't on the bar.")
        entry = self.modules[where[0]][where[1]]
        merged = {**entry.get("options", {}), key: value}
        entry["options"] = _validate_options(mtype, merged)

    def options(self, mtype: str) -> dict:
        from .modules import defaults
        where = self.find(mtype)
        stored = self.modules[where[0]][where[1]].get("options", {}) if where else {}
        return {**defaults(mtype), **stored}

    def set(self, name: str, value) -> None:
        if name not in FIELDS:
            raise BarError(f"Unknown setting “{name}”.")
        setattr(self, name, _check_field(name, value))


# ---------------------------------------------------------------- validation

def _check_field(name: str, value):
    kind, _default, extra = FIELDS[name]
    if kind == "bool":
        if not isinstance(value, bool):
            raise BarError(f"“{name}” must be on or off.")
        return value
    if kind == "int":
        if isinstance(value, bool) or not isinstance(value, (int, float)) or value != int(value):
            raise BarError(f"“{name}” must be a whole number.")
        lo, hi = extra
        if not lo <= value <= hi:
            raise BarError(f"“{name}” must be between {lo} and {hi}.")
        return int(value)
    if kind == "float":
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            raise BarError(f"“{name}” must be a number.")
        lo, hi = extra
        if not lo <= value <= hi:
            raise BarError(f"“{name}” must be between {lo} and {hi}.")
        return round(float(value), 2)
    if kind == "choice":
        if value not in extra:
            raise BarError(f"“{name}” must be one of: {', '.join(extra)}.")
        return value
    if kind == "color":
        if not isinstance(value, str) or not _HEX_RE.match(value):
            raise BarError(f"“{name}” must be a color like #3584e4.")
        return value.lower()
    if kind == "font":
        if not isinstance(value, str) or not _FONT_RE.match(value.strip()):
            raise BarError("The font name can only use letters, numbers, spaces and - _ . +")
        return " ".join(value.split())
    raise BarError(f"Unknown setting “{name}”.")


def _validate_options(mtype: str, options) -> dict:
    if not isinstance(options, dict):
        raise BarError(f"Options for {MODULES[mtype]['label']} are damaged.")
    out = {}
    label = MODULES[mtype]["label"]
    for key, value in options.items():
        spec = option_spec(mtype, key)
        if spec is None:
            raise BarError(f"{label} has no option “{key}”.")
        kind = spec["kind"]
        if kind == "bool":
            if not isinstance(value, bool):
                raise BarError(f"{label}: “{spec['label']}” must be on or off.")
        elif kind == "int":
            if isinstance(value, bool) or not isinstance(value, (int, float)) or value != int(value):
                raise BarError(f"{label}: “{spec['label']}” must be a whole number.")
            value = int(value)
            if not spec["min"] <= value <= spec["max"]:
                raise BarError(f"{label}: “{spec['label']}” must be between {spec['min']} and {spec['max']}.")
        elif kind == "choice":
            if value not in [c["value"] for c in spec["choices"]]:
                raise BarError(f"{label}: “{spec['label']}” has an unknown value.")
        out[key] = value
    if mtype == "battery":
        from .modules import defaults
        o = {**defaults(mtype), **out}
        if o["critical"] >= o["warning"]:
            raise BarError("Battery: “Critical below” must be lower than “Warning below”.")
    return out


def validate_bar(data) -> Bar:
    """A checked ``Bar`` from plain data; missing settings get defaults."""
    if not isinstance(data, dict):
        raise BarError("The bar design is damaged.")
    unknown = set(data) - set(FIELDS) - {"modules"}
    if unknown:
        raise BarError(f"Unknown bar setting “{sorted(unknown)[0]}”.")
    values = {}
    for name, (_kind, default, _extra) in FIELDS.items():
        values[name] = _check_field(name, data.get(name, default))
    raw = data.get("modules", _default_modules())
    if not isinstance(raw, dict) or set(raw) - set(ZONES):
        raise BarError("The module layout is damaged.")
    modules, seen = {}, set()
    for zone in ZONES:
        lst = raw.get(zone, [])
        if not isinstance(lst, list):
            raise BarError("The module layout is damaged.")
        modules[zone] = []
        for entry in lst:
            if isinstance(entry, str):
                entry = {"type": entry}
            if not isinstance(entry, dict) or set(entry) - {"type", "options"}:
                raise BarError("A module entry is damaged.")
            mtype = entry.get("type")
            if mtype not in MODULES:
                raise BarError(f"Unknown module “{mtype}”.")
            if mtype in seen:
                raise BarError(f"{MODULES[mtype]['label']} appears twice.")
            seen.add(mtype)
            modules[zone].append({"type": mtype, "options": _validate_options(mtype, entry.get("options", {}))})
    if len(seen) > MAX_MODULES:
        raise BarError("The bar has too many modules.")
    return Bar(**values, modules=modules)


# ---------------------------------------------------------------- documents / presets

@dataclass
class BarPreset:
    id: str
    name: str
    description: str
    bar: Bar
    tags: list = field(default_factory=list)
    path: str | None = None


def _text(data: dict, name: str, limit: int, required: bool = False) -> str:
    value = data.get(name, "")
    if value is None:
        value = ""
    if not isinstance(value, str) or not _SAFE_RE.match(value):
        raise BarError(f"The {name} must be plain text.")
    value = value.strip()
    if required and not value:
        raise BarError(f"The bar needs a {name}.")
    if len(value) > limit:
        raise BarError(f"The {name} is too long (at most {limit} characters).")
    return value


def slugify(text: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:48].strip("-")
    return s or "bar"


def validate_document(data) -> BarPreset:
    if not isinstance(data, dict) or data.get("format") != BAR_FORMAT:
        raise BarError("This isn't a Hyprland Mod bar file.")
    version = data.get("version")
    if not isinstance(version, int) or isinstance(version, bool) or version < 1:
        raise BarError("The bar file has no valid version.")
    if version > FORMAT_VERSION:
        raise BarError("This bar was made by a newer Hyprland Mod — update the app to open it.")
    name = _text(data, "name", 60, required=True)
    desc = _text(data, "description", 200)
    pid = data.get("id") or slugify(name)
    if not isinstance(pid, str) or not _ID_RE.match(pid):
        raise BarError("The bar file has an invalid id.")
    tags = data.get("tags", [])
    if not isinstance(tags, list) or not all(isinstance(t, str) and _ID_RE.match(t) for t in tags):
        raise BarError("The bar file has invalid tags.")
    return BarPreset(pid, name, desc, validate_bar(data.get("bar")), list(tags[:6]))


def document(bar: Bar, name: str, description: str = "", tags=()) -> dict:
    validate_bar(bar.to_dict())
    return {"format": BAR_FORMAT, "version": FORMAT_VERSION, "id": slugify(name), "name": name,
            "description": description, "tags": list(tags), "bar": bar.to_dict()}


def _read_json(path) -> dict:
    try:
        if os.path.getsize(path) > MAX_FILE_BYTES:
            raise BarError("The file is too large to be a bar design.")
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except BarError:
        raise
    except (OSError, UnicodeDecodeError) as exc:
        raise BarError(f"Couldn't read the file: {getattr(exc, 'strerror', None) or exc}.") from None
    except ValueError:
        raise BarError("The file is damaged or isn't a bar design.") from None


def load_presets(folder=None) -> list[BarPreset]:
    out = []
    for path in sorted(Path(folder or BUILTIN_DIR).glob("*.json")):
        data = _read_json(path)
        preset = validate_document(data)
        preset.path = str(path)
        out.append((data.get("order", 999), preset))
    return [p for _o, p in sorted(out, key=lambda t: t[0])]


def export_bar(bar: Bar, path, name: str, description: str = "") -> None:
    atomic_write(str(path), json.dumps(document(bar, name, description), indent=2, ensure_ascii=False) + "\n")


def import_bar(path) -> BarPreset:
    preset = validate_document(_read_json(path))
    preset.path = str(path)
    return preset


# ---------------------------------------------------------------- the design being edited

def data_dir(cm=None) -> Path:
    base = getattr(cm, "data_dir", None) or os.environ.get("HYPRLAND_MOD_DATA") \
        or os.path.expanduser("~/.local/share/hyprland-mod")
    return Path(base) / "bar"


def save_current(bar: Bar, cm=None, folder=None) -> Path:
    path = Path(folder or data_dir(cm)) / "current.json"
    atomic_write(str(path), json.dumps(bar.to_dict(), indent=2, ensure_ascii=False) + "\n")
    return path


def load_current(cm=None, folder=None) -> Bar | None:
    """The design saved last time, or None (missing or damaged file)."""
    path = Path(folder or data_dir(cm)) / "current.json"
    if not path.exists():
        return None
    try:
        return validate_bar(_read_json(path))
    except BarError:
        return None
