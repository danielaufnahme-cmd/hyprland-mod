"""Ricing presets and whole-setup files: model, validation, load/save, import/export.

A preset is a small JSON document (``.hyprmod-preset`` when shared) holding look
settings plus optional ``bezier``/``animation`` lines. Everything coming from a
file is validated against the schema before it can reach the ConfigManager:
unknown keys, commands (``exec``...) and malformed values are rejected with a
message meant for people, not developers.
"""

from __future__ import annotations

import datetime
import json
import os
import re
from dataclasses import dataclass, field
from pathlib import Path

from .core.fsutil import atomic_write
from .core.managed import validate_value
from .core.schema import load_schema
from .core.values import format_value, parse_color, parse_value

PRESET_FORMAT = "hyprland-mod-preset"
SETUP_FORMAT = "hyprland-mod-setup"
FORMAT_VERSION = 1
PRESET_EXT = ".hyprmod-preset"
SETUP_EXT = ".hyprmod"
MAX_FILE_BYTES = 512 * 1024
BUILTIN_DIR = Path(__file__).resolve().parent.parent / "data" / "presets"

# Every built-in preset sets all of these, so trying one after another never
# leaves a stale value behind. "Save current setup" captures the same keys.
LOOK_KEYS = (
    "general:gaps_in", "general:gaps_out", "general:border_size",
    "general:col.active_border", "general:col.inactive_border",
    "decoration:rounding", "decoration:rounding_power",
    "decoration:active_opacity", "decoration:inactive_opacity",
    "decoration:blur:enabled", "decoration:blur:size", "decoration:blur:passes",
    "decoration:blur:vibrancy", "decoration:blur:noise",
    "decoration:shadow:enabled", "decoration:shadow:range", "decoration:shadow:render_power",
    "decoration:shadow:color", "decoration:shadow:offset", "decoration:shadow:sharp",
    "decoration:dim_inactive", "decoration:dim_strength",
    "animations:enabled",
)
# Presets may only touch the look of things.
PRESET_SECTIONS = ("general:", "decoration:", "animations:", "group:", "dwindle:", "master:")
PRESET_DENY = {"decoration:screen_shader", "general:locale", "general:layout"}
LIST_KEYWORDS = ("bezier", "animation")

ANIMATION_LEAVES = {
    "global", "windows", "windowsIn", "windowsOut", "windowsMove", "layers", "layersIn",
    "layersOut", "fade", "fadeIn", "fadeOut", "fadeSwitch", "fadeShadow", "fadeDim", "fadeLayers",
    "fadeLayersIn", "fadeLayersOut", "fadePopups", "fadePopupsIn", "fadePopupsOut", "fadeDpms",
    "border", "borderangle", "workspaces", "workspacesIn", "workspacesOut", "specialWorkspace",
    "specialWorkspaceIn", "specialWorkspaceOut", "zoomFactor", "monitorAdded",
}
_STYLE_RE = re.compile(
    r"^(popin( \d{1,3}%)?|slide( (left|right|top|bottom))?|slidevert|slidefade( \d{1,3}%)?"
    r"|slidefadevert( \d{1,3}%)?|fade|once|loop|gnomed)$")
_CURVE_NAME_RE = re.compile(r"^[A-Za-z][A-Za-z0-9_]{0,31}$")
_NUM_RE = re.compile(r"^-?\d+(\.\d+)?$")
_ANGLE_RE = re.compile(r"^-?\d{1,3}(\.\d+)?deg$")
_HEX_RE = re.compile(r"^#[0-9a-fA-F]{6}$")
_ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,47}$")
_SAFE_RE = re.compile(r"^[^\x00-\x1f\x7f]*$")


class PresetError(ValueError):
    """A preset or setup file was rejected; ``str(err)`` is shown to the user."""


@dataclass
class Preset:
    id: str
    name: str
    description: str = ""
    author: str = ""
    tags: list[str] = field(default_factory=list)
    settings: dict[str, str] = field(default_factory=dict)
    lists: dict[str, list[str]] = field(default_factory=dict)
    preview: dict = field(default_factory=dict)
    builtin: bool = False
    path: str | None = None

    def to_dict(self) -> dict:
        data = {"format": PRESET_FORMAT, "version": FORMAT_VERSION, "id": self.id, "name": self.name,
                "author": self.author, "description": self.description, "tags": list(self.tags),
                "settings": dict(self.settings),
                "lists": {k: list(v) for k, v in self.lists.items()}}
        if self.preview:
            data["preview"] = dict(self.preview)
        return data

    def matches(self, cm, options: dict | None = None) -> bool:
        """True when the ConfigManager currently shows (or has staged) this exact look."""
        options = options if options is not None else schema_options()
        for key, value in self.settings.items():
            opt = options.get(key)
            if opt is None or _canon(opt, cm.get(key)) != _canon(opt, value):
                return False
        for kw, values in self.lists.items():
            if [" ".join(v.split()) for v in cm.get_list(kw)] != [" ".join(v.split()) for v in values]:
                return False
        return True


# ---------------------------------------------------------------- schema

_OPTIONS: dict[str, dict] | None = None


def schema_options() -> dict[str, dict]:
    global _OPTIONS
    if _OPTIONS is None:
        _OPTIONS = {o["key"]: o for p in load_schema() for g in p.get("groups", [])
                    for o in g.get("options", [])}
    return _OPTIONS


def _canon(opt: dict, text: str | None) -> str:
    try:
        return format_value(opt, parse_value(opt, text))
    except (ValueError, TypeError):
        return str(text)


def label_for(key: str, options: dict | None = None) -> str:
    opt = (options if options is not None else schema_options()).get(key)
    return (opt or {}).get("label") or key.rsplit(":", 1)[-1].replace("_", " ").capitalize()


# ---------------------------------------------------------------- validation

def _text_field(data: dict, name: str, limit: int, required: bool = False) -> str:
    value = data.get(name, "")
    if value is None:
        value = ""
    if not isinstance(value, str):
        raise PresetError(f"The preset's {name} must be text.")
    value = " ".join(value.split())
    if len(value) > limit:
        raise PresetError(f"The preset's {name} is too long (at most {limit} characters).")
    if not _SAFE_RE.match(value):
        raise PresetError(f"The preset's {name} contains control characters.")
    if required and not value:
        raise PresetError(f"The preset has no {name}.")
    return value


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:40].strip("-")
    return slug or "preset"


def _as_text(key: str, raw) -> str:
    if isinstance(raw, bool):
        return "true" if raw else "false"
    if isinstance(raw, (int, float)):
        return str(raw)
    if isinstance(raw, str):
        return raw
    raise PresetError(f"“{key}” has a value Hyprland Mod can't understand.")


def _range_check(key: str, opt: dict, num: float) -> None:
    lo, hi = opt.get("min"), opt.get("max")
    if (lo is not None and num < lo) or (hi is not None and num > hi):
        raise PresetError(f"“{label_for(key)}” must be between {lo} and {hi} (got {num:g}).")


def normalize_value(key: str, raw, opt: dict) -> str:
    """Validate one setting by its schema type; returns the canonical config text."""
    text = _as_text(key, raw).strip()
    if len(text) > 300:
        raise PresetError(f"“{label_for(key)}” has a value that's far too long.")
    try:
        validate_value(text)
    except ValueError as exc:
        raise PresetError(f"“{label_for(key)}”: {exc}.") from None
    kind = opt.get("type", "string")
    bad = PresetError(f"“{label_for(key)}” has an invalid value: {text!r}.")
    if kind == "bool":
        if text.lower() not in ("true", "false", "yes", "no", "on", "off", "1", "0"):
            raise bad
    elif kind in ("int", "float"):
        if not _NUM_RE.match(text) or (kind == "int" and "." in text):
            raise bad
        _range_check(key, opt, float(text))
    elif kind == "color":
        if parse_color(text) is None:
            raise bad
    elif kind == "gradient":
        tokens = re.findall(r"rgba?\([^)]*\)|\S+", text)
        if not tokens or len(tokens) > 11:
            raise bad
        if _ANGLE_RE.match(tokens[-1]):
            tokens = tokens[:-1]
        if not tokens or any(parse_color(t) is None for t in tokens):
            raise bad
    elif kind == "vec2":
        parts = text.replace(",", " ").split()
        if len(parts) != 2 or not all(_NUM_RE.match(p) for p in parts):
            raise bad
    elif kind == "enum":
        allowed = {str(c.get("value") if isinstance(c, dict) else c).lower() for c in opt.get("choices") or []}
        if allowed and text.lower() not in allowed:
            raise bad
    elif not _SAFE_RE.match(text):
        raise bad
    return _canon(opt, text) if kind != "string" else text


def validate_settings(raw, options: dict, preset_only: bool = True) -> dict[str, str]:
    if raw is None:
        return {}
    if not isinstance(raw, dict):
        raise PresetError("The “settings” part of the file is malformed.")
    if len(raw) > 400:
        raise PresetError("The file has too many settings.")
    out: dict[str, str] = {}
    for key, value in raw.items():
        if not isinstance(key, str) or key not in options:
            name = key if isinstance(key, str) else repr(key)
            if isinstance(key, str) and key.split(":")[0].startswith("exec"):
                raise PresetError("This file tries to run commands, which isn't allowed.")
            raise PresetError(f"“{name[:60]}” isn't a setting Hyprland Mod knows.")
        if preset_only and (key in PRESET_DENY or not key.startswith(PRESET_SECTIONS)):
            raise PresetError(f"Presets can only change the look; “{label_for(key, options)}” isn't allowed.")
        out[key] = normalize_value(key, value, options[key])
    return out


def _bezier(entry: str) -> tuple[str, str]:
    parts = [p.strip() for p in entry.split(",")]
    if len(parts) != 5 or not _CURVE_NAME_RE.match(parts[0]) or not all(_NUM_RE.match(p) for p in parts[1:]):
        raise PresetError(f"Invalid animation curve: {entry[:60]!r}.")
    x0, y0, x1, y1 = (float(p) for p in parts[1:])
    if not (0 <= x0 <= 1 and 0 <= x1 <= 1 and -3 <= y0 <= 3 and -3 <= y1 <= 3):
        raise PresetError(f"Animation curve “{parts[0]}” is out of range.")
    return parts[0], ", ".join(parts)


def _animation(entry: str, curves: set[str] | None) -> str:
    parts = [p.strip() for p in entry.split(",", 4)]
    bad = PresetError(f"Invalid animation line: {entry[:60]!r}.")
    if len(parts) < 2 or parts[0] not in ANIMATION_LEAVES or parts[1] not in ("0", "1"):
        raise bad
    if len(parts) > 2:
        if len(parts) < 4 or not _NUM_RE.match(parts[2]) or not 0 < float(parts[2]) <= 100:
            raise bad
        curve = parts[3]
        if not _CURVE_NAME_RE.match(curve):
            raise bad
        if curves is not None and curve != "default" and curve not in curves:
            raise PresetError(f"Animation “{parts[0]}” uses the curve “{curve}”, which the file doesn't define.")
        if len(parts) == 5 and not _STYLE_RE.match(" ".join(parts[4].split())):
            raise bad
    return ", ".join(" ".join(p.split()) for p in parts)


def validate_lists(raw, strict_curves: bool = True) -> dict[str, list[str]]:
    if raw is None:
        return {}
    if not isinstance(raw, dict):
        raise PresetError("The “lists” part of the file is malformed.")
    for kw, values in raw.items():
        if not isinstance(kw, str) or kw not in LIST_KEYWORDS:
            name = kw if isinstance(kw, str) else repr(kw)
            if name.startswith(("exec", "bind", "env", "plugin", "source")):
                raise PresetError("This file tries to run commands or change key bindings, which isn't allowed.")
            raise PresetError(f"“{name[:40]}” lines aren't allowed in this file.")
        if not isinstance(values, list) or len(values) > 64 or not all(isinstance(v, str) for v in values):
            raise PresetError(f"The “{kw}” lines are malformed.")
        for v in values:
            if not _SAFE_RE.match(v) or len(v) > 200:
                raise PresetError(f"The “{kw}” lines are malformed.")
    out: dict[str, list[str]] = {}
    names: set[str] = set()
    if "bezier" in raw:
        out["bezier"] = []
        for entry in raw["bezier"]:
            name, line = _bezier(entry)
            names.add(name)
            out["bezier"].append(line)
    if "animation" in raw:
        out["animation"] = [_animation(e, names if strict_curves else None) for e in raw["animation"]]
    return out


def _preview_meta(raw) -> dict:
    if not isinstance(raw, dict):
        return {}
    out = {}
    wall = raw.get("wallpaper")
    if isinstance(wall, list) and 1 <= len(wall) <= 3 and all(isinstance(c, str) and _HEX_RE.match(c) for c in wall):
        out["wallpaper"] = list(wall)
    win = raw.get("window")
    if isinstance(win, str) and _HEX_RE.match(win):
        out["window"] = win
    return out


def validate_preset(data, options: dict | None = None, builtin: bool = False) -> Preset:
    """Check a decoded preset document; returns a clean :class:`Preset` or raises PresetError."""
    options = options if options is not None else schema_options()
    if not isinstance(data, dict):
        raise PresetError("This isn't a Hyprland Mod preset.")
    if data.get("format") == SETUP_FORMAT:
        raise PresetError("This is a whole-setup file. Open it from Import & Export instead.")
    if data.get("format") != PRESET_FORMAT:
        raise PresetError("This isn't a Hyprland Mod preset.")
    if not isinstance(data.get("version"), int) or data["version"] > FORMAT_VERSION:
        raise PresetError("This preset was made by a newer version of Hyprland Mod.")
    name = _text_field(data, "name", 60, required=True)
    pid = data.get("id")
    if not isinstance(pid, str) or not _ID_RE.match(pid):
        pid = slugify(name)
    tags = data.get("tags") or []
    if not isinstance(tags, list) or len(tags) > 8:
        raise PresetError("The preset's tags are malformed.")
    clean_tags = []
    for t in tags:
        if not isinstance(t, str) or not t.strip() or len(t) > 24 or not _SAFE_RE.match(t):
            raise PresetError("The preset's tags are malformed.")
        if t.strip().lower() not in clean_tags:
            clean_tags.append(t.strip().lower())
    settings = validate_settings(data.get("settings"), options)
    lists = validate_lists(data.get("lists"))
    if not settings and not lists:
        raise PresetError("This preset doesn't change anything.")
    return Preset(id=pid, name=name, description=_text_field(data, "description", 300),
                  author=_text_field(data, "author", 60), tags=clean_tags, settings=settings,
                  lists=lists, preview=_preview_meta(data.get("preview")), builtin=builtin)


def _read_json(path: str | os.PathLike[str]):
    try:
        if os.path.getsize(path) > MAX_FILE_BYTES:
            raise PresetError("The file is too large to be a Hyprland Mod file.")
        with open(path, encoding="utf-8") as fh:
            return json.load(fh)
    except PresetError:
        raise
    except (OSError, UnicodeDecodeError) as exc:
        raise PresetError(f"Couldn't read the file: {getattr(exc, 'strerror', None) or exc}.") from None
    except ValueError:
        raise PresetError("The file is damaged or isn't a Hyprland Mod file.") from None


# ---------------------------------------------------------------- built-in & user presets

def load_builtin(folder: str | os.PathLike[str] | None = None) -> list[Preset]:
    out = []
    for path in sorted(Path(folder or BUILTIN_DIR).glob("*.json")):
        data = _read_json(path)
        preset = validate_preset(data, builtin=True)
        preset.path = str(path)
        out.append((data.get("order", 999), preset))
    return [p for _o, p in sorted(out, key=lambda t: t[0])]


def user_dir(cm=None) -> Path:
    base = getattr(cm, "data_dir", None) or os.environ.get("HYPRLAND_MOD_DATA") \
        or os.path.expanduser("~/.local/share/hyprland-mod")
    return Path(base) / "presets"


def load_user(cm=None) -> tuple[list[Preset], list[str]]:
    """User presets (sorted by name) plus messages about files that were skipped."""
    folder = user_dir(cm)
    presets, problems = [], []
    if not folder.is_dir():
        return presets, problems
    for path in sorted(folder.glob("*.json")):
        try:
            preset = validate_preset(_read_json(path))
        except PresetError as exc:
            problems.append(f"{path.name}: {exc}")
            continue
        preset.path = str(path)
        presets.append(preset)
    presets.sort(key=lambda p: p.name.lower())
    return presets, problems


def _unique_id(base: str, taken: set[str]) -> str:
    pid, n = base, 2
    while pid in taken:
        pid = f"{base[:44]}-{n}"
        n += 1
    return pid


def save_user(preset: Preset, cm=None, builtin_ids: set[str] | None = None) -> Preset:
    """Write ``preset`` into the user folder (new presets get a unique id)."""
    folder = user_dir(cm)
    folder.mkdir(parents=True, exist_ok=True)
    if not preset.path:
        taken = {p.stem for p in folder.glob("*.json")} | set(builtin_ids or ())
        if builtin_ids is None:
            taken |= {p.stem for p in BUILTIN_DIR.glob("*.json")}
        preset.id = _unique_id(slugify(preset.id or preset.name), taken)
        preset.path = str(folder / f"{preset.id}.json")
    preset.builtin = False
    atomic_write(preset.path, json.dumps(preset.to_dict(), indent=2, ensure_ascii=False) + "\n")
    return preset


def delete_user(preset: Preset, cm=None) -> None:
    """Remove a user preset file; refuses anything outside the user preset folder."""
    if preset.builtin or not preset.path:
        raise PresetError("Built-in presets can't be deleted.")
    path = Path(preset.path).resolve()
    if path.parent != user_dir(cm).resolve():
        raise PresetError("Only your own presets can be deleted.")
    path.unlink(missing_ok=True)


def rename_user(preset: Preset, name: str, cm=None) -> Preset:
    if preset.builtin:
        raise PresetError("Built-in presets can't be renamed.")
    preset.name = _text_field({"name": name}, "name", 60, required=True)
    return save_user(preset, cm)


def read_preset_file(path: str | os.PathLike[str]) -> Preset:
    return validate_preset(_read_json(path))


def import_preset(path: str | os.PathLike[str], cm=None) -> Preset:
    """Validate a shared preset file and copy it into the user folder."""
    preset = read_preset_file(path)
    preset.path = None
    return save_user(preset, cm)


def export_preset(preset: Preset, path: str | os.PathLike[str]) -> None:
    data = preset.to_dict()
    atomic_write(str(path), json.dumps(data, indent=2, ensure_ascii=False) + "\n")


# ---------------------------------------------------------------- staging

def capture(cm, name: str, description: str = "", author: str = "", options: dict | None = None) -> Preset:
    """A preset from what is currently shown (applied + staged values)."""
    options = options if options is not None else schema_options()
    settings = {}
    for key in LOOK_KEYS:
        opt = options.get(key)
        if opt is not None:
            settings[key] = _canon(opt, cm.get(key))
    data = {"format": PRESET_FORMAT, "version": FORMAT_VERSION, "name": name, "description": description,
            "author": author, "tags": [], "settings": settings,
            "lists": {kw: cm.get_list(kw) for kw in LIST_KEYWORDS if cm.get_list(kw)}}
    try:
        return validate_preset(data, options)
    except PresetError:
        # e.g. animations using curves defined somewhere we can't see: keep the look only
        data["lists"] = {}
        return validate_preset(data, options)


def stage_preset(cm, preset: Preset) -> None:
    """Stage every setting and list of ``preset`` (nothing is written until Apply)."""
    for key, value in preset.settings.items():
        cm.set(key, value)
    for kw in LIST_KEYWORDS:
        if kw in preset.lists:
            cm.set_list(kw, preset.lists[kw])


# ---------------------------------------------------------------- whole setup

def setup_document(cm) -> dict:
    from . import __version__
    return {"format": SETUP_FORMAT, "version": FORMAT_VERSION,
            "created": datetime.datetime.now().astimezone().isoformat(timespec="seconds"),
            "app_version": __version__, "state": cm.export_state()}


def export_setup(cm, path: str | os.PathLike[str]) -> dict:
    data = setup_document(cm)
    atomic_write(str(path), json.dumps(data, indent=2, ensure_ascii=False) + "\n")
    return data


def validate_setup(data, options: dict | None = None) -> dict:
    """Check a decoded setup document; returns a clean state for ``cm.import_state``."""
    options = options if options is not None else schema_options()
    if not isinstance(data, dict):
        raise PresetError("This isn't a Hyprland Mod setup file.")
    if data.get("format") == PRESET_FORMAT:
        raise PresetError("This is a preset. Import it from the Presets page instead.")
    if data.get("format") != SETUP_FORMAT:
        raise PresetError("This isn't a Hyprland Mod setup file.")
    if not isinstance(data.get("version"), int) or data["version"] > FORMAT_VERSION:
        raise PresetError("This setup was made by a newer version of Hyprland Mod.")
    state = data.get("state")
    if not isinstance(state, dict):
        raise PresetError("The setup file is damaged.")
    return {"version": 1,
            "settings": validate_settings(state.get("settings") or {}, options, preset_only=False),
            "lists": validate_lists(state.get("lists") or {}, strict_curves=False)}


def read_setup(path: str | os.PathLike[str]) -> dict:
    return validate_setup(_read_json(path))


@dataclass
class Change:
    key: str
    label: str
    old: str | None
    new: str | None


def setup_changes(cm, state: dict, options: dict | None = None) -> list[Change]:
    """What importing ``state`` would change compared to what is shown now."""
    options = options if options is not None else schema_options()
    current = cm.export_state()
    changes = []
    for key, value in sorted(state["settings"].items()):
        opt = options.get(key, {})
        old = cm.get(key)
        if _canon(opt, old) != _canon(opt, value):
            changes.append(Change(key, label_for(key, options), old, value))
    for key in sorted(set(current.get("settings", {})) - set(state["settings"])):
        changes.append(Change(key, label_for(key, options), cm.get(key), None))
    for kw in LIST_KEYWORDS:
        old_list = current.get("lists", {}).get(kw)
        new_list = state["lists"].get(kw)
        if (old_list or None) != (new_list or None):
            title = "Animation curves" if kw == "bezier" else "Animations"
            changes.append(Change(kw, title, _lines(old_list), _lines(new_list)))
    return changes


def _lines(values):
    if not values:
        return None
    return f"{len(values)} line{'s' if len(values) != 1 else ''}"


def import_setup(cm, state: dict) -> str:
    """Back up first, then stage ``state`` as the complete managed setup. Returns the backup id."""
    backup_id = cm.create_backup("Before import")
    cm.import_state(state)
    return backup_id
