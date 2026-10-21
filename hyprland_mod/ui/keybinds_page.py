"""Keyboard shortcuts: the user's own binds (read-only) and Hyprland Mod's editable ones."""

import json
import os
import re

from gi.repository import Adw, Gdk, Gio, GLib, Gtk

from hyprland_mod.core import actions as core_actions
from hyprland_mod.core import items as core_items

# GDK modifier keyvals -> Hyprland modifier names (for live display while held)
_MOD_KEYVALS = {
    "Super_L": "SUPER", "Super_R": "SUPER", "Meta_L": "SUPER", "Meta_R": "SUPER",
    "Hyper_L": "SUPER", "Hyper_R": "SUPER",
    "Shift_L": "SHIFT", "Shift_R": "SHIFT",
    "Control_L": "CTRL", "Control_R": "CTRL",
    "Alt_L": "ALT", "Alt_R": "ALT", "ISO_Level3_Shift": "ALT",
}
_MOD_MASKS = (("SUPER", Gdk.ModifierType.SUPER_MASK), ("SHIFT", Gdk.ModifierType.SHIFT_MASK),
              ("CTRL", Gdk.ModifierType.CONTROL_MASK), ("ALT", Gdk.ModifierType.ALT_MASK))
_MOD_LABELS = {"SUPER": "Super", "SHIFT": "Shift", "CTRL": "Ctrl", "ALT": "Alt", "CAPS": "Caps Lock",
               "MOD2": "Num Lock", "MOD3": "Mod3", "MOD5": "AltGr"}
_KEY_LABELS = {
    "return": "Enter", "kp_enter": "Enter", "space": "Space", "tab": "Tab", "escape": "Esc",
    "backspace": "Backspace", "delete": "Delete", "insert": "Insert", "home": "Home", "end": "End",
    "page_up": "Page Up", "prior": "Page Up", "page_down": "Page Down", "next": "Page Down",
    "left": "←", "right": "→", "up": "↑", "down": "↓", "print": "Print Screen",
    "grave": "`", "minus": "-", "equal": "=", "comma": ",", "period": ".", "slash": "/",
    "backslash": "\\", "semicolon": ";", "apostrophe": "'", "bracketleft": "[", "bracketright": "]",
    "mouse:272": "Left Click", "mouse:273": "Right Click", "mouse:274": "Middle Click",
    "mouse_down": "Scroll Down", "mouse_up": "Scroll Up",
    "xf86audioraisevolume": "Volume Up", "xf86audiolowervolume": "Volume Down",
    "xf86audiomute": "Mute", "xf86audioplay": "Play", "xf86audionext": "Next Track",
    "xf86audioprev": "Previous Track", "xf86monbrightnessup": "Brightness Up",
    "xf86monbrightnessdown": "Brightness Down",
}
_DIRECTION_LABELS = [("left", "Left"), ("right", "Right"), ("up", "Up"), ("down", "Down")]
_CATEGORY_ORDER = ["Apps", "Window", "Focus", "Workspaces", "Layout", "Groups", "Mouse", "System"]
_FIELD_CODE_RE = re.compile(r"\s*%[fFuUdDnNickvm]")


# ---------------------------------------------------------------- formatting helpers

def key_label(key):
    k = str(key or "")
    if k.lower() in _KEY_LABELS:
        return _KEY_LABELS[k.lower()]
    if k.lower().startswith("xf86"):
        return k[4:]
    return k.upper() if len(k) == 1 else k


def combo_labels(mods, key):
    return [_MOD_LABELS.get(m, m.title()) for m in mods] + ([key_label(key)] if key else [])


def combo_text(mods, key):
    return " + ".join(combo_labels(mods, key))


def gdk_to_hypr_key(keyval):
    """Hyprland key name for an unshifted GDK keyval (letters upper-case, rest xkb names)."""
    name = Gdk.keyval_name(keyval) or ""
    if len(name) == 1 and name.isalpha():
        return name.upper()
    return name


def app_command(info):
    """Desktop entry command line without field codes (%U, %f...)."""
    cmd = info.get_commandline() or info.get_executable() or ""
    return _FIELD_CODE_RE.sub("", cmd).strip()


def installed_apps():
    """Visible desktop apps as dicts: name, command, wm_class, gicon, id (sorted by name)."""
    out, seen = [], set()
    for info in Gio.AppInfo.get_all():
        try:
            if not info.should_show():
                continue
            cmd = app_command(info)
            name = info.get_display_name() or info.get_name()
        except Exception:
            continue
        if not cmd or not name or (name, cmd) in seen:
            continue
        seen.add((name, cmd))
        app_id = (info.get_id() or "").removesuffix(".desktop")
        wm_class = None
        if isinstance(info, Gio.DesktopAppInfo):
            wm_class = info.get_startup_wm_class()
        out.append({"name": name, "command": cmd, "wm_class": wm_class or app_id, "gicon": info.get_icon(),
                    "id": app_id, "description": info.get_description() or ""})
    out.sort(key=lambda a: a["name"].lower())
    return out


def bind_title(b):
    """``(title, subtitle)`` for one bind dict as returned by ``cm.list_binds()``."""
    action = core_actions.BY_ID.get(b.get("action") or "")
    arg = b.get("arg")
    desc = (b.get("description") or "").strip()
    if action is None:
        title = desc or "Custom action"
        if b.get("raw"):
            sub = b["raw"]
        else:
            sub = "Defined in your Lua config"
    else:
        what = action["label"]
        if arg:
            kind = (action.get("arg") or {}).get("type")
            if kind == "direction":
                what = f"{what.replace(' in direction', '')} {arg}"
            elif kind == "workspace":
                what = f"{what} {arg}"
            else:
                what = f"{what}: {arg}"
        title, sub = (desc, what) if desc else (what, "")
    if b.get("submap"):
        sub = (sub + " · " if sub else "") + f"only in “{b['submap']}” mode"
    return title, sub


def _fake_user_binds():
    """Dev only: ``HYPRLAND_MOD_FAKE_BINDS=<hyprctl -j binds json>`` stands in for the user's binds."""
    path = os.environ.get("HYPRLAND_MOD_FAKE_BINDS")
    if not path:
        return None
    try:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
    except (OSError, ValueError):
        return []
    out = []
    for i, item in enumerate(data if isinstance(data, list) else []):
        if not isinstance(item, dict) or not isinstance(item.get("key"), str):
            continue
        mask = int(item.get("modmask") or 0)
        dispatcher, arg = str(item.get("dispatcher") or ""), str(item.get("arg") or "")
        action, action_arg = (None, None) if dispatcher == "__lua" else core_actions.match_conf(dispatcher, arg)
        out.append({"id": f"user-{i}", "mods": [m for m in core_items.MOD_ORDER if mask & core_items.MOD_BITS[m]],
                    "key": item["key"], "action": action, "arg": action_arg,
                    "raw": "" if dispatcher == "__lua" else f"{dispatcher} {arg}".strip(),
                    "description": str(item.get("description") or ""), "source": "user",
                    "submap": str(item.get("submap") or "")})
    return out


def all_binds(cm):
    fake = _fake_user_binds()
    binds = list(cm.list_binds())
    if fake is None:
        return binds
    return fake + [b for b in binds if b.get("source") == "mod"]


def conflicts(cm, mods, key, exclude_id=None):
    if not key:
        return []
    if os.environ.get("HYPRLAND_MOD_FAKE_BINDS") is None:
        try:
            return list(cm.find_conflicts(mods, key, exclude_id))
        except ValueError:
            return []
    target = core_items.combo(list(mods), str(key))
    return [b for b in all_binds(cm) if b["id"] != exclude_id and not b.get("submap")
            and core_items.combo(b["mods"], b["key"]) == target]


# ---------------------------------------------------------------- widgets

class Keycaps(Gtk.Box):
    """``Super`` ``+`` ``Shift`` ``+`` ``T`` drawn as little keys."""

    def __init__(self, mods=(), key="", large=False, **kwargs):
        super().__init__(spacing=6 if large else 4, valign=Gtk.Align.CENTER, **kwargs)
        self.add_css_class("keycaps")
        if large:
            self.add_css_class("large")
        self.set(mods, key)

    def set(self, mods, key):
        child = self.get_first_child()
        while child:
            nxt = child.get_next_sibling()
            self.remove(child)
            child = nxt
        for i, text in enumerate(combo_labels(mods, key)):
            if i:
                plus = Gtk.Label(label="+")
                plus.add_css_class("keycap-plus")
                self.append(plus)
            cap = Gtk.Label(label=text)
            cap.add_css_class("keycap")
            self.append(cap)


class PickerPage(Adw.NavigationPage):
    """Searchable, grouped list that calls ``on_pick(value)`` and pops itself.

    ``entries``: ``[(group, title, subtitle, icon, value)]``; icon is a name or a Gio.Icon.
    """

    def __init__(self, title, entries, on_pick, placeholder="Search", selected=None):
        super().__init__(title=title)
        self._on_pick = on_pick
        self._rows = []  # (row, group, haystack)
        tv = Adw.ToolbarView()
        tv.add_top_bar(Adw.HeaderBar())
        self.search = Gtk.SearchEntry(placeholder_text=placeholder, hexpand=True)
        self.search.connect("search-changed", lambda *_: self._filter())
        self.search.connect("activate", lambda *_: self._activate_first())
        bar = Gtk.Box(margin_start=12, margin_end=12, margin_bottom=6, margin_top=2)
        bar.append(self.search)
        tv.add_top_bar(bar)

        self._stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=150)
        prefs = Adw.PreferencesPage()
        self._groups = {}
        for group, title_, subtitle, icon, value in entries:
            g = self._groups.get(group)
            if g is None:
                g = Adw.PreferencesGroup(title=GLib.markup_escape_text(group or ""))
                self._groups[group] = g
                prefs.add(g)
            row = Adw.ActionRow(title=GLib.markup_escape_text(title_), activatable=True)
            if subtitle:
                row.set_subtitle(GLib.markup_escape_text(subtitle))
                row.set_subtitle_lines(1)
            if icon is not None:
                img = Gtk.Image.new_from_gicon(icon) if isinstance(icon, Gio.Icon) else Gtk.Image(icon_name=icon)
                img.set_pixel_size(32 if isinstance(icon, Gio.Icon) else 16)
                if isinstance(icon, Gio.Icon):
                    img.add_css_class("lowres-icon")
                row.add_prefix(img)
            if selected is not None and value == selected:
                check = Gtk.Image(icon_name="object-select-symbolic")
                check.add_css_class("accent")
                row.add_suffix(check)
            row.connect("activated", lambda _r, v=value: self._pick(v))
            g.add(row)
            self._rows.append((row, group, f"{title_} {subtitle or ''} {group or ''}".lower()))
        self._stack.add_named(prefs, "list")
        empty = Adw.StatusPage(icon_name="system-search-symbolic", title="No Results Found")
        empty.add_css_class("compact")
        self._stack.add_named(empty, "empty")
        tv.set_content(self._stack)
        self.set_child(tv)
        self.connect("shown", lambda *_: self.search.grab_focus())

    def _filter(self):
        tokens = self.search.get_text().lower().split()
        visible_groups = set()
        for row, group, hay in self._rows:
            vis = all(t in hay for t in tokens)
            row.set_visible(vis)
            if vis:
                visible_groups.add(group)
        for group, g in self._groups.items():
            g.set_visible(group in visible_groups)
        self._stack.set_visible_child_name("list" if visible_groups else "empty")

    def _activate_first(self):
        for row, _g, _h in self._rows:
            if row.get_visible():
                row.emit("activated")
                return

    def _pick(self, value):
        self._on_pick(value)
        nav = self.get_ancestor(Adw.NavigationView)
        if nav is not None:
            nav.pop()


def action_entries():
    order = {c: i for i, c in enumerate(_CATEGORY_ORDER)}
    acts = sorted((a for a in core_actions.ACTIONS if core_actions.available(a)),
                  key=lambda a: (order.get(a["category"], 99), core_actions.ACTIONS.index(a)))
    return [(a["category"], a["label"], None, None, a["id"]) for a in acts]


def app_entries(apps):
    return [("", a["name"], a["description"], a["gicon"] or "application-x-executable-symbolic", i)
            for i, a in enumerate(apps)]


# ---------------------------------------------------------------- add / edit dialog

class BindDialog(Adw.Dialog):
    def __init__(self, cm, bind=None, on_saved=None):
        super().__init__(title="Edit Shortcut" if bind else "Add Shortcut", content_width=540, content_height=660)
        self.cm = cm
        self.bind = bind
        self.on_saved = on_saved
        self.mods = list(bind["mods"]) if bind else []
        self.key = bind["key"] if bind else ""
        self.action_id = bind["action"] if bind else None
        self._recording = False
        self._held = set()
        self._apps = None
        self._app_name = None
        self._conflicts = []
        self._inhibited = None

        self.nav = Adw.NavigationView()
        self.toasts = Adw.ToastOverlay(child=self.nav)
        self.set_child(self.toasts)

        tv = Adw.ToolbarView()
        header = Adw.HeaderBar(show_start_title_buttons=False, show_end_title_buttons=False)
        cancel = Gtk.Button(label="Cancel")
        cancel.connect("clicked", lambda *_: self.close())
        header.pack_start(cancel)
        self.save_btn = Gtk.Button(label="Save" if bind else "Add")
        self.save_btn.add_css_class("suggested-action")
        self.save_btn.connect("clicked", lambda *_: self._save())
        header.pack_end(self.save_btn)
        tv.add_top_bar(header)
        self.banner = Adw.Banner(button_label="Save Anyway")
        self.banner.add_css_class("conflict-banner")
        self.banner.connect("button-clicked", lambda *_: self._save(force=True))
        tv.add_top_bar(self.banner)

        prefs = Adw.PreferencesPage()
        tv.set_content(prefs)
        self.nav.add(Adw.NavigationPage(title=self.get_title(), tag="form", child=tv))

        # shortcut recorder
        rec_group = Adw.PreferencesGroup()
        self.recorder = Gtk.Button(hexpand=True)
        self.recorder.add_css_class("shortcut-recorder")
        self.recorder.connect("clicked", lambda *_: self._start_recording())
        self.rec_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=150)
        empty = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, valign=Gtk.Align.CENTER)
        empty_icon = Gtk.Image(icon_name="input-keyboard-symbolic", pixel_size=28)
        empty_icon.add_css_class("dim-label")
        empty.append(empty_icon)
        empty.append(Gtk.Label(label="Click to set a shortcut"))
        self.rec_stack.add_named(empty, "empty")
        listening = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, valign=Gtk.Align.CENTER)
        self.live_caps = Keycaps(large=True, halign=Gtk.Align.CENTER)
        self.listen_label = Gtk.Label(label="Press a key combination…")
        self.listen_label.add_css_class("title-4")
        listening.append(self.listen_label)
        listening.append(self.live_caps)
        hint = Gtk.Label(label="Esc to cancel")
        hint.add_css_class("dim-label")
        hint.add_css_class("caption")
        listening.append(hint)
        self.rec_stack.add_named(listening, "listening")
        keys = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=8, valign=Gtk.Align.CENTER)
        self.caps = Keycaps(large=True, halign=Gtk.Align.CENTER)
        keys.append(self.caps)
        change = Gtk.Label(label="Click to change")
        change.add_css_class("dim-label")
        change.add_css_class("caption")
        keys.append(change)
        self.rec_stack.add_named(keys, "keys")
        self.recorder.set_child(self.rec_stack)
        rec_group.add(self.recorder)
        prefs.add(rec_group)

        # manual fallback: some combinations never reach an app (Hyprland grabs them first)
        manual = Adw.ExpanderRow(title="Type the keys instead",
                                 subtitle="For combinations that can't be pressed here")
        mod_row = Adw.ActionRow(title="Modifiers")
        mod_box = Gtk.Box(spacing=4, valign=Gtk.Align.CENTER)
        mod_box.add_css_class("linked")
        self.mod_toggles = {}
        for m in ("SUPER", "SHIFT", "CTRL", "ALT"):
            t = Gtk.ToggleButton(label=_MOD_LABELS[m])
            t.connect("toggled", self._on_mod_toggled, m)
            mod_box.append(t)
            self.mod_toggles[m] = t
        mod_row.add_suffix(mod_box)
        manual.add_row(mod_row)
        self.key_entry = Adw.EntryRow(title="Key (e.g. T, Return, F5, XF86AudioMute)")
        self.key_entry.connect("changed", self._on_key_typed)
        manual.add_row(self.key_entry)
        manual_group = Adw.PreferencesGroup()
        manual_group.add(manual)
        prefs.add(manual_group)

        # action + argument
        act_group = Adw.PreferencesGroup(title="Action")
        self.action_row = Adw.ActionRow(title="What it does", activatable=True)
        self.action_value = Gtk.Label(ellipsize=3, max_width_chars=26)
        self.action_value.add_css_class("dim-label")
        self.action_row.add_suffix(self.action_value)
        self.action_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
        self.action_row.connect("activated", lambda *_: self._pick_action())
        act_group.add(self.action_row)

        self.ws_row = Adw.SpinRow.new_with_range(1, 20, 1)
        self.ws_row.set_title("Workspace")
        self.dir_row = Adw.ComboRow(title="Direction", model=Gtk.StringList.new([t for _v, t in _DIRECTION_LABELS]))
        self.app_row = Adw.ActionRow(title="App", activatable=True)
        self.app_value = Gtk.Label(ellipsize=3, max_width_chars=26)
        self.app_value.add_css_class("dim-label")
        self.app_row.add_suffix(self.app_value)
        self.app_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
        self.app_row.connect("activated", lambda *_: self._pick_app())
        self.app_cmd = ""
        self.text_row = Adw.EntryRow()
        for r in (self.ws_row, self.dir_row, self.app_row, self.text_row):
            r.set_visible(False)
            act_group.add(r)
        for sig, w in (("notify::value", self.ws_row), ("notify::selected", self.dir_row),
                       ("changed", self.text_row)):
            w.connect(sig, lambda *_: self._validate())
        self.desc_row = Adw.EntryRow(title="Name (optional)")
        act_group.add(self.desc_row)
        prefs.add(act_group)

        if bind:
            rm_group = Adw.PreferencesGroup()
            rm = Gtk.Button(label="Remove Shortcut", halign=Gtk.Align.CENTER)
            rm.add_css_class("destructive-action")
            rm.add_css_class("pill")
            rm.connect("clicked", lambda *_: self._remove())
            rm_group.add(rm)
            prefs.add(rm_group)

        keyc = Gtk.EventControllerKey(propagation_phase=Gtk.PropagationPhase.CAPTURE)
        keyc.connect("key-pressed", self._on_key_pressed)
        keyc.connect("key-released", self._on_key_released)
        self.add_controller(keyc)
        self.connect("closed", lambda *_: self._stop_recording())

        if bind:
            self.desc_row.set_text(bind.get("description") or "")
            self._load_arg(bind.get("arg"))
        self._sync()

    # -- recorder --------------------------------------------------------

    def _start_recording(self):
        self._recording = True
        self._held = set()
        self.live_caps.set([], "")
        self.listen_label.set_visible(True)
        self.recorder.add_css_class("recording")
        self.rec_stack.set_visible_child_name("listening")
        # ask the compositor to pass Super+... to us while listening
        surface = self.get_native().get_surface() if self.get_native() else None
        if isinstance(surface, Gdk.Toplevel):
            try:
                surface.inhibit_system_shortcuts(None)
                self._inhibited = surface
            except Exception:
                self._inhibited = None
        self.recorder.grab_focus()

    def _stop_recording(self):
        self._recording = False
        self.recorder.remove_css_class("recording")
        if self._inhibited is not None:
            try:
                self._inhibited.restore_system_shortcuts()
            except Exception:
                pass
            self._inhibited = None

    def _held_mods(self, state):
        mods = set(self._held)
        for name, mask in _MOD_MASKS:
            if state & mask:
                mods.add(name)
        return [m for m in core_items.MOD_ORDER if m in mods]

    def _on_key_pressed(self, ctrl, keyval, keycode, state):
        if not self._recording:
            return False
        name = Gdk.keyval_name(keyval) or ""
        if name in _MOD_KEYVALS:
            self._held.add(_MOD_KEYVALS[name])
            mods = self._held_mods(state)
            self.listen_label.set_visible(not mods)
            self.live_caps.set(mods, "")
            return True
        if name == "Escape" and not self._held_mods(state):
            self._stop_recording()
            self._sync()
            return True
        # the unshifted key: Shift+1 must record "1", not "exclam"
        base = keyval
        try:
            ok, kv, *_ = self.get_display().translate_key(keycode, 0, ctrl.get_group())
            if ok and kv:
                base = kv
        except Exception:
            pass
        self.mods = self._held_mods(state)
        self.key = gdk_to_hypr_key(base)
        self._stop_recording()
        self._sync()
        return True

    def _on_key_released(self, _ctrl, keyval, _keycode, state):
        if not self._recording:
            return
        name = Gdk.keyval_name(keyval) or ""
        if name in _MOD_KEYVALS:
            self._held.discard(_MOD_KEYVALS[name])
            mods = [m for m in self._held_mods(state) if m != _MOD_KEYVALS[name] or m in self._held]
            self.listen_label.set_visible(not mods)
            self.live_caps.set(mods, "")

    def _on_mod_toggled(self, btn, mod):
        if getattr(self, "_syncing", False):
            return
        mods = set(self.mods)
        (mods.add if btn.get_active() else mods.discard)(mod)
        self.mods = [m for m in core_items.MOD_ORDER if m in mods]
        self._sync()

    def _on_key_typed(self, entry):
        if getattr(self, "_syncing", False):
            return
        self.key = entry.get_text().strip()
        self._sync()

    # -- action ----------------------------------------------------------

    def _pick_action(self):
        self.nav.push(PickerPage("Choose an Action", action_entries(), self._set_action,
                                 placeholder="Search actions", selected=self.action_id))

    def _set_action(self, action_id):
        if action_id != self.action_id:
            self.action_id = action_id
            self._load_arg(None)
        self._sync()

    def _pick_app(self):
        if self._apps is None:
            self._apps = installed_apps()
        self.nav.push(PickerPage("Choose an App", app_entries(self._apps), self._set_app,
                                 placeholder="Search apps"))

    def _set_app(self, index):
        app = self._apps[index]
        self.app_cmd = app["command"]
        self._app_name = app["name"]
        self.app_value.set_label(app["name"])
        if not self.desc_row.get_text().strip():
            self.desc_row.set_text(f"Open {app['name']}")
        self._validate()

    def _arg_spec(self):
        action = core_actions.BY_ID.get(self.action_id or "")
        return (action or {}).get("arg")

    def _load_arg(self, arg):
        spec = self._arg_spec()
        kind = spec["type"] if spec else None
        self.ws_row.set_visible(kind == "workspace")
        self.dir_row.set_visible(kind == "direction")
        self.app_row.set_visible(kind == "app")
        self.text_row.set_visible(kind in ("command", "string", "int"))
        if kind == "workspace":
            try:
                self.ws_row.set_value(int(arg or 1))
            except ValueError:
                # "e+1", "name:web"... can't be shown in a spin: fall back to text
                self.ws_row.set_visible(False)
                self.text_row.set_visible(True)
                self.text_row.set_title(spec["label"])
                self.text_row.set_text(arg)
        elif kind == "direction":
            ids = [v for v, _t in _DIRECTION_LABELS]
            self.dir_row.set_selected(ids.index(arg) if arg in ids else 0)
        elif kind == "app":
            self.app_cmd = arg or ""
            self.app_value.set_label(arg or "Choose…")
        elif kind:
            self.text_row.set_title(spec["label"] + (" (optional)" if spec.get("optional") else ""))
            self.text_row.set_text(arg or "")

    def _arg_value(self):
        spec = self._arg_spec()
        if not spec:
            return None
        kind = spec["type"]
        if self.text_row.get_visible():
            return self.text_row.get_text().strip() or None
        if kind == "workspace":
            return str(int(self.ws_row.get_value()))
        if kind == "direction":
            return _DIRECTION_LABELS[self.dir_row.get_selected()][0]
        if kind == "app":
            return self.app_cmd or None
        return None

    # -- state -----------------------------------------------------------

    def _sync(self):
        self._syncing = True
        try:
            for m, t in self.mod_toggles.items():
                t.set_active(m in self.mods)
            if self.key_entry.get_text().strip() != self.key:
                self.key_entry.set_text(self.key)
        finally:
            self._syncing = False
        if not self._recording:
            if self.key:
                self.caps.set(self.mods, self.key)
                self.rec_stack.set_visible_child_name("keys")
            else:
                self.rec_stack.set_visible_child_name("empty")
        action = core_actions.BY_ID.get(self.action_id or "")
        self.action_value.set_label(action["label"] if action else "Choose…")
        self._validate()

    def _validate(self):
        exclude = self.bind["id"] if self.bind else None
        self._conflicts = conflicts(self.cm, self.mods, self.key, exclude)
        if self._conflicts:
            names = [f"“{bind_title(b)[0]}”" for b in self._conflicts[:2]]
            more = len(self._conflicts) - len(names)
            listed = " and ".join(names) + (f" and {more} more" if more > 0 else "")
            self.banner.set_title(GLib.markup_escape_text(
                f"{combo_text(self.mods, self.key)} is already used by {listed}"))
        self.banner.set_revealed(bool(self._conflicts))
        spec = self._arg_spec()
        ready = bool(self.key) and bool(self.action_id)
        if ready and spec and not spec.get("optional"):
            ready = bool(self._arg_value())
        self.banner.set_button_label("Save Anyway" if ready else None)
        self.save_btn.set_sensitive(ready and not self._conflicts)

    def _save(self, force=False):
        if self._conflicts and not force:
            return
        data = {"mods": self.mods, "key": self.key, "action": self.action_id, "arg": self._arg_value(),
                "description": self.desc_row.get_text().strip()}
        try:
            if self.bind:
                self.cm.update_bind(self.bind["id"], data)
            else:
                self.cm.add_bind(data)
        except (ValueError, KeyError) as e:
            self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(str(e)), timeout=4))
            return
        self.close()
        if self.on_saved:
            self.on_saved("Shortcut saved" if self.bind else "Shortcut added")

    def _remove(self):
        try:
            self.cm.remove_bind(self.bind["id"])
        except KeyError as e:
            self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(str(e)), timeout=4))
            return
        self.close()
        if self.on_saved:
            self.on_saved("Shortcut removed")


# ---------------------------------------------------------------- page

class KeybindsPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        self.cm = cm
        self.toast_overlay = toast_overlay
        self._rows = []  # (row, group_key, haystack)

        prefs = Adw.PreferencesPage(
            description="Shortcuts you add here are saved separately — your own config stays untouched.")
        self.set_child(prefs)

        search_group = Adw.PreferencesGroup()
        self.search = Gtk.SearchEntry(placeholder_text="Filter shortcuts", hexpand=True)
        self.search.connect("search-changed", lambda *_: self._filter())
        search_group.add(self.search)
        prefs.add(search_group)

        self.mine = Adw.PreferencesGroup(title="Your Shortcuts",
                                         description="Added in Hyprland Mod — click one to change it")
        add = Gtk.Button(valign=Gtk.Align.CENTER)
        add.set_child(Adw.ButtonContent(icon_name="list-add-symbolic", label="Add Shortcut"))
        add.add_css_class("flat")
        add.connect("clicked", lambda *_: self.open_dialog())
        self.mine.set_header_suffix(add)
        prefs.add(self.mine)

        self.theirs = Adw.PreferencesGroup(title="Shortcuts From Your Config",
                                           description="Read-only — these come from your Hyprland config")
        prefs.add(self.theirs)

        spacer = Adw.PreferencesGroup()
        spacer.set_size_request(-1, 40)
        prefs.add(spacer)
        self.refresh()

    def refresh(self):
        for row, group, _h in self._rows:
            group.remove(row)
        self._rows = []
        try:
            binds = all_binds(self.cm)
        except Exception as e:  # core error: show instead of crashing
            binds = []
            self.toast(f"Couldn't read shortcuts: {e}")
        mine = [b for b in binds if b.get("source") == "mod"]
        theirs = [b for b in binds if b.get("source") != "mod"]
        if not mine:
            row = Adw.ActionRow(title="No shortcuts yet", subtitle="Add one to launch apps, switch workspaces and more")
            row.add_css_class("empty-row")
            row.add_prefix(Gtk.Image(icon_name="input-keyboard-symbolic"))
            self.mine.add(row)
            self._rows.append((row, self.mine, None))
        for b in mine:
            row = self._bind_row(b, editable=True)
            self.mine.add(row)
        if not theirs:
            row = Adw.ActionRow(title="None found", subtitle="Your config doesn't define any shortcuts we can read")
            row.add_css_class("empty-row")
            self.theirs.add(row)
            self._rows.append((row, self.theirs, None))
        for b in theirs:
            self.theirs.add(self._bind_row(b, editable=False))
        self.theirs.set_title(f"Shortcuts From Your Config ({len(theirs)})" if theirs
                              else "Shortcuts From Your Config")
        self._filter()

    def _bind_row(self, b, editable):
        title, sub = bind_title(b)
        row = Adw.ActionRow(title=GLib.markup_escape_text(title), activatable=editable)
        row.add_css_class("bind-row")
        if sub:
            row.set_subtitle(GLib.markup_escape_text(sub))
            row.set_subtitle_lines(1)
        row.add_suffix(Keycaps(b["mods"], b["key"]))
        if editable:
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, bind=b: self.open_dialog(bind))
        else:
            row.add_css_class("readonly")
        group = self.mine if editable else self.theirs
        self._rows.append((row, group, f"{title} {sub} {combo_text(b['mods'], b['key'])} {b['key']}".lower()))
        return row

    def _filter(self):
        tokens = self.search.get_text().lower().split()
        for row, _g, hay in self._rows:
            row.set_visible(hay is None and not tokens or hay is not None and all(t in hay for t in tokens))

    def toast(self, text):
        self.toast_overlay.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=2))

    def open_dialog(self, bind=None):
        dlg = BindDialog(self.cm, bind, on_saved=self.toast)
        dlg.present(self.get_root())
        return dlg

    # -- dev helpers -------------------------------------------------------

    def dev_seed(self):
        """Screenshot helper: stage a few example shortcuts (dry-run only)."""
        for data in ({"mods": ["SUPER"], "key": "Return", "action": "launch_app", "arg": "kitty",
                      "description": "Open Terminal"},
                     {"mods": ["SUPER", "SHIFT"], "key": "S", "action": "screenshot_full", "arg": None},
                     {"mods": ["SUPER"], "key": "3", "action": "workspace", "arg": "3"},
                     {"mods": ["SUPER", "ALT"], "key": "right", "action": "move_window_direction",
                      "arg": "right"}):
            try:
                self.cm.add_bind(data)
            except ValueError:
                pass

    def dev_open(self, name):
        """Screenshot helper: an Add dialog with a conflicting shortcut already entered."""
        dlg = self.open_dialog()
        dlg.mods, dlg.key = ["SUPER", "SHIFT"], "C"
        dlg._set_action("launch_app")
        dlg.app_cmd = "firefox"
        dlg.app_value.set_label("Firefox")
        dlg.desc_row.set_text("Open Firefox")
        dlg._sync()
        if name == "picker":
            dlg._pick_action()
        return dlg
