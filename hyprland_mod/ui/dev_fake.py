"""In-memory stand-in for hyprland_mod.core (dev only, HYPRLAND_MOD_FAKE_CORE=1)."""

import datetime
import json
import os
from collections import namedtuple

ApplyResult = namedtuple("ApplyResult", "ok errors live_applied")


def _o(key, label, type_, default, description="", **kw):
    d = {"key": key, "label": label, "type": type_, "default": default,
         "description": description, "advanced": kw.pop("advanced", False)}
    d.update(kw)
    return d


_SCHEMA = [
    {"id": "general", "title": "General", "icon": "view-grid-symbolic", "order": 0,
     "description": "Gaps, borders and how windows are laid out",
     "groups": [
         {"title": "Gaps", "description": "Space around and between tiled windows", "options": [
             _o("general:gaps_in", "Inner gaps", "int", 5, "Space between neighbouring windows", min=0, max=60, unit="px"),
             _o("general:gaps_out", "Outer gaps", "int", 20, "Space between windows and screen edges", min=0, max=120, unit="px"),
             _o("general:gaps_workspaces", "Workspace gaps", "int", 0, "Gap between workspaces while switching", min=0, max=200, unit="px", advanced=True),
         ]},
         {"title": "Borders", "options": [
             _o("general:border_size", "Border width", "int", 1, "Thickness of the window border", min=0, max=20, unit="px"),
             _o("general:col.active_border", "Active border", "gradient", "rgba(33ccffee) rgba(00ff99ee) 45deg", "Border of the focused window"),
             _o("general:col.inactive_border", "Inactive border", "gradient", "rgba(595959aa)", "Border of unfocused windows"),
             _o("general:resize_on_border", "Resize by dragging borders", "bool", False, "Grab a window edge to resize it"),
             _o("general:extend_border_grab_area", "Border grab area", "int", 15, "Extra area around the border for grabbing", min=0, max=50, unit="px", advanced=True),
         ]},
         {"title": "Layout", "options": [
             _o("general:layout", "Tiling layout", "enum", "dwindle", "How new windows are arranged",
                choices=[{"value": "dwindle", "label": "Dwindle"}, {"value": "master", "label": "Master"}]),
             _o("general:allow_tearing", "Allow tearing", "bool", False, "Lower latency for games that request it", advanced=True),
         ]},
     ]},
    {"id": "decoration", "title": "Decoration", "icon": "preferences-desktop-appearance-symbolic", "order": 1,
     "description": "Rounded corners, opacity, shadows and blur",
     "groups": [
         {"title": "Corners & Opacity", "options": [
             _o("decoration:rounding", "Corner radius", "int", 10, "Roundness of window corners", min=0, max=40, unit="px"),
             _o("decoration:active_opacity", "Active opacity", "float", 1.0, "Opacity of the focused window", min=0.1, max=1.0, step=0.05, digits=2),
             _o("decoration:inactive_opacity", "Inactive opacity", "float", 1.0, "Opacity of unfocused windows", min=0.1, max=1.0, step=0.05, digits=2),
             _o("decoration:dim_inactive", "Dim inactive windows", "bool", False, "Darken windows that are not focused"),
             _o("decoration:dim_strength", "Dim strength", "float", 0.5, "How much to darken inactive windows", min=0.0, max=1.0, step=0.05, digits=2),
         ]},
         {"title": "Shadow", "options": [
             _o("decoration:shadow:enabled", "Drop shadow", "bool", True, "Draw a soft shadow behind windows"),
             _o("decoration:shadow:range", "Shadow size", "int", 4, "How far the shadow spreads", min=0, max=100, unit="px"),
             _o("decoration:shadow:render_power", "Shadow falloff", "int", 3, "Sharpness of the shadow edge (1–4)", min=1, max=4, advanced=True),
             _o("decoration:shadow:color", "Shadow color", "color", "rgba(1a1a1aee)", "Color of the shadow"),
         ]},
         {"title": "Blur", "options": [
             _o("decoration:blur:enabled", "Background blur", "bool", True, "Blur what's behind transparent windows"),
             _o("decoration:blur:size", "Blur size", "int", 3, "Blur radius", min=1, max=20),
             _o("decoration:blur:passes", "Blur passes", "int", 1, "More passes give a smoother blur", min=1, max=8, advanced=True),
             _o("decoration:blur:vibrancy", "Vibrancy", "float", 0.17, "Color saturation boost of blurred content", min=0, max=1, step=0.01, digits=2, advanced=True),
         ]},
     ]},
    {"id": "animations", "title": "Animations", "icon": "power-profile-performance-symbolic", "order": 2,
     "description": "Motion when windows open, close and move",
     "groups": [
         {"title": "Animations", "options": [
             _o("animations:enabled", "Enable animations", "bool", True, "Animate windows, workspaces and fades"),
             _o("animations:first_launch_animation", "First launch animation", "bool", True, "Fade in on the first launch", advanced=True),
         ]},
     ]},
    {"id": "input", "title": "Input", "icon": "input-keyboard-symbolic", "order": 3,
     "description": "Keyboard, mouse and touchpad",
     "groups": [
         {"title": "Keyboard", "options": [
             _o("input:kb_layout", "Keyboard layout", "string", "us", "Layout code, e.g. us, de, es"),
             _o("input:repeat_rate", "Repeat rate", "int", 25, "Characters per second when holding a key", min=1, max=100),
             _o("input:repeat_delay", "Repeat delay", "int", 600, "Delay before a held key repeats", min=100, max=2000, step=50, unit="ms"),
         ]},
         {"title": "Mouse", "options": [
             _o("input:sensitivity", "Pointer speed", "float", 0.0, "Pointer sensitivity", min=-1.0, max=1.0, step=0.05, digits=2),
             _o("input:follow_mouse", "Focus follows mouse", "enum", "1", "When moving the pointer changes focus",
                choices=[{"value": "0", "label": "Never"}, {"value": "1", "label": "Always"},
                         {"value": "2", "label": "Cursor only"}, {"value": "3", "label": "Loose"}]),
             _o("input:natural_scroll", "Natural scrolling", "bool", False, "Content follows the finger"),
         ]},
     ]},
    {"id": "misc", "title": "Miscellaneous", "icon": "preferences-other-symbolic", "order": 4,
     "description": "Everything else",
     "groups": [
         {"title": "Behaviour", "options": [
             _o("misc:disable_hyprland_logo", "Hide Hyprland logo", "bool", False, "Don't show the default wallpaper logo"),
             _o("misc:vfr", "Variable frame rate", "bool", True, "Save power by rendering only when needed"),
             _o("cursor:inactive_timeout", "Hide cursor after", "int", 0, "Seconds of inactivity, 0 = never", min=0, max=60, unit="s"),
             _o("misc:background_color", "Background color", "color", "rgba(111111ff)", "Shown when no wallpaper is set", advanced=True),
         ]},
     ]},
]


def load_schema():
    return json.loads(json.dumps(_SCHEMA))


def _fmt_float(v):
    s = f"{float(v):.4f}".rstrip("0").rstrip(".")
    return s if s not in ("-0", "") else "0"


def format_value(opt, value):
    t = opt["type"]
    if t == "bool":
        return "true" if value else "false"
    if t == "int":
        return str(int(value))
    if t == "float":
        return _fmt_float(value)
    if t == "vec2":
        if isinstance(value, (list, tuple)):
            return f"{_fmt_float(value[0])} {_fmt_float(value[1])}"
    return str(value)


def parse_value(opt, text):
    if text is None:
        return opt["default"]
    t = opt["type"]
    try:
        if t == "bool":
            return text.strip().lower() in ("1", "true", "yes", "on")
        if t == "int":
            return int(float(text.split()[0]))
        if t == "float":
            return float(text)
        if t == "vec2":
            a, b = text.split()[:2]
            return (float(a), float(b))
    except (ValueError, IndexError):
        return opt["default"]
    return text


class ConfigManager:
    def __init__(self):
        self._opts = {o["key"]: o for p in _SCHEMA for g in p["groups"] for o in g["options"]}
        self._saved = {}
        self._pending = {}
        self._undo, self._redo = [], []
        self._subs = []
        self._backups = []
        self.hyprland_running = bool(os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"))
        self.dry_run = os.environ.get("HYPRLAND_MOD_DRY_RUN") == "1"
        self._saved.update({"general:gaps_out": "14", "decoration:rounding": "12"})
        self.create_backup("Before trying new borders")
        self._backups[0]["created"] = "2026-10-18T19:42:00"
        self.create_backup("")
        self._backups[1]["created"] = "2026-10-20T09:05:00"

    def _emit(self, event, key=None):
        for fn in list(self._subs):
            fn(event, key)

    def subscribe(self, fn):
        self._subs.append(fn)

    def get(self, key):
        if key in self._pending:
            return self._pending[key]
        return self._saved.get(key)

    def is_modified(self, key):
        opt = self._opts.get(key)
        cur = self.get(key)
        if cur is None or opt is None:
            return False
        return cur != format_value(opt, opt["default"])

    def _set_raw(self, key, value):
        if value is None or value == self._saved.get(key):
            self._pending.pop(key, None)
            if value is None and key in self._saved:
                self._pending[key] = None
        else:
            self._pending[key] = value

    def set(self, key, value):
        old = self.get(key)
        if old == value:
            return
        self._undo.append((key, old, value))
        self._redo.clear()
        self._set_raw(key, value)
        self._emit("changed", key)

    def reset(self, key):
        opt = self._opts[key]
        self.set(key, format_value(opt, opt["default"]))

    def pending(self):
        return dict(self._pending)

    @property
    def is_dirty(self):
        return bool(self._pending)

    def discard(self):
        self._pending.clear()
        self._undo.clear()
        self._redo.clear()
        self._emit("discarded")

    def apply(self):
        for k, v in self._pending.items():
            if v is None:
                self._saved.pop(k, None)
            else:
                self._saved[k] = v
        self._pending.clear()
        self._emit("applied")
        return ApplyResult(True, [], self.hyprland_running)

    @property
    def can_undo(self):
        return bool(self._undo)

    @property
    def can_redo(self):
        return bool(self._redo)

    def undo(self):
        if not self._undo:
            return
        key, old, new = self._undo.pop()
        self._redo.append((key, old, new))
        self._set_raw(key, old)
        self._emit("changed", key)

    def redo(self):
        if not self._redo:
            return
        key, old, new = self._redo.pop()
        self._undo.append((key, old, new))
        self._set_raw(key, new)
        self._emit("changed", key)

    def list_backups(self):
        return list(reversed(self._backups))

    def create_backup(self, label=""):
        b = {"id": f"b{len(self._backups) + 1}", "created": datetime.datetime.now().isoformat(timespec="seconds"),
             "label": label, "files": ["hyprland.conf"], "_data": dict(self._saved)}
        self._backups.append(b)
        self._emit("backups")
        return b

    def restore_backup(self, backup_id):
        for b in self._backups:
            if b["id"] == backup_id:
                self._saved = dict(b["_data"])
                self._pending.clear()
                self._emit("restored")
                return True
        return False
