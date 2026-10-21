"""Window rules: make apps open floating, sized, on a workspace and so on."""

import re

from gi.repository import Adw, GLib, Gtk

from .keybinds_page import PickerPage, app_entries, installed_apps

# (effect, title, subtitle, phrase used in the summary sentence)
PLACEMENT_SWITCHES = [
    ("float", "Float", "Open as a free-floating window", "floating"),
    ("center", "Center", "Place it in the middle of the screen", "centered"),
    ("pin", "Pin", "Keep it visible on every workspace", "pinned to every workspace"),
    ("fullscreen", "Fullscreen", "Cover the whole screen", "fullscreen"),
    ("maximize", "Maximize", "Fill the screen, keeping the bar visible", "maximized"),
    ("no_initial_focus", "Don't Focus When Opened", "Keep typing where you were", "in the background"),
    ("keep_aspect_ratio", "Keep Aspect Ratio", "Resizing keeps the window's proportions", None),
]
APPEARANCE_SWITCHES = [
    ("no_blur", "No Blur", None, "blur"),
    ("no_shadow", "No Shadow", None, "shadow"),
    ("no_border", "No Border", None, "border"),
    ("no_rounding", "Square Corners", None, "rounded corners"),
    ("no_dim", "Never Dim", "Stay bright when another window is focused", "dimming"),
    ("no_anim", "No Animations", None, "animations"),
]
VALUE_EFFECTS = {"size", "workspace", "opacity"}
KNOWN_EFFECTS = {e for e, *_ in PLACEMENT_SWITCHES + APPEARANCE_SWITCHES} | VALUE_EFFECTS

_apps_cache = None


def _apps():
    global _apps_cache
    if _apps_cache is None:
        try:
            _apps_cache = installed_apps()
        except Exception:
            _apps_cache = []
    return _apps_cache


def plain_pattern(pattern):
    """``^(org\\.gnome\\.Calculator)$`` -> ``org.gnome.Calculator`` (best effort)."""
    text = str(pattern or "")
    m = re.match(r"^\^?\(?(.*?)\)?\$?$", text)
    text = m.group(1) if m else text
    return text.replace("\\", "")


def app_for_class(pattern):
    plain = plain_pattern(pattern).lower()
    if not plain:
        return None
    for a in _apps():
        if a["wm_class"] and a["wm_class"].lower() == plain or a["id"].lower() == plain:
            return a
    return None


def target_name(match):
    if match.get("class"):
        app = app_for_class(match["class"])
        if app:
            return app["name"]
        plain = plain_pattern(match["class"])
        return plain[:1].upper() + plain[1:]
    if match.get("title"):
        return f"Windows titled “{plain_pattern(match['title'])}”"
    return "Matching windows"


def _join(parts):
    if len(parts) <= 1:
        return "".join(parts)
    return ", ".join(parts[:-1]) + " and " + parts[-1]


def _pct(text):
    try:
        return f"{round(float(text) * 100)}%"
    except ValueError:
        return text


def summary(rule):
    """``"Firefox opens floating, centered, at 1200×800"``."""
    eff = rule.get("effects") or {}
    states = [phrase for key, _t, _s, phrase in PLACEMENT_SWITCHES if eff.get(key) is True and phrase]
    extras = []
    if eff.get("size"):
        w, _, h = str(eff["size"]).partition(" ")
        extras.append(f"at {w}×{h}" if h else f"at size {w}")
    if eff.get("workspace"):
        ws, _, silent = str(eff["workspace"]).partition(" ")
        extras.append(f"on workspace {ws}" + (" without switching to it" if silent else ""))
    if eff.get("opacity"):
        active, _, inactive = str(eff["opacity"]).partition(" ")
        extras.append(f"at {_pct(active)} opacity" + (f" ({_pct(inactive)} when unfocused)" if inactive else ""))
    without = [phrase for key, _t, _s, phrase in APPEARANCE_SWITCHES if eff.get(key) is True]
    if without:
        extras.append("without " + _join(without))
    if eff.get("keep_aspect_ratio") is True:
        extras.append("keeping its aspect ratio")
    extra = [k for k in eff if k not in KNOWN_EFFECTS]
    if extra:
        extras.append(f"with {len(extra)} more setting{'s' if len(extra) != 1 else ''}")
    parts = [p for p in (_join(states), ", ".join(extras)) if p]
    sentence = " ".join(parts)
    who = target_name(rule.get("match") or {})
    return f"{who} opens {sentence}" if sentence else f"{who}: no effects yet"


# ---------------------------------------------------------------- builder dialog

class RuleDialog(Adw.Dialog):
    def __init__(self, cm, rule=None, on_saved=None):
        super().__init__(title="Edit Rule" if rule else "New Window Rule", content_width=560, content_height=720)
        self.cm = cm
        self.rule = rule
        self.on_saved = on_saved
        self._loading = True
        self._switches = {}

        self.nav = Adw.NavigationView()
        self.toasts = Adw.ToastOverlay(child=self.nav)
        self.set_child(self.toasts)

        tv = Adw.ToolbarView()
        header = Adw.HeaderBar(show_start_title_buttons=False, show_end_title_buttons=False)
        cancel = Gtk.Button(label="Cancel")
        cancel.connect("clicked", lambda *_: self.close())
        header.pack_start(cancel)
        self.save_btn = Gtk.Button(label="Save" if rule else "Add")
        self.save_btn.add_css_class("suggested-action")
        self.save_btn.connect("clicked", lambda *_: self._save())
        header.pack_end(self.save_btn)
        tv.add_top_bar(header)
        prefs = Adw.PreferencesPage()
        tv.set_content(prefs)
        self.nav.add(Adw.NavigationPage(title=self.get_title(), tag="form", child=tv))

        # live sentence
        sum_group = Adw.PreferencesGroup()
        card = Gtk.Box(spacing=12)
        card.add_css_class("rule-summary")
        icon = Gtk.Image(icon_name="focus-windows-symbolic", valign=Gtk.Align.START)
        icon.add_css_class("rule-summary-icon")
        card.append(icon)
        self.summary_label = Gtk.Label(xalign=0, wrap=True, hexpand=True)
        card.append(self.summary_label)
        sum_group.add(card)
        prefs.add(sum_group)

        # match
        match = Adw.PreferencesGroup(
            title="Which Windows",
            description="Class and title are patterns: ^(firefox)$ matches exactly “firefox”, "
                        "firefox matches anything containing it")
        self.app_row = Adw.ActionRow(title="App", subtitle="Fill in the class from an installed app",
                                     activatable=True)
        self.app_row.add_prefix(Gtk.Image(icon_name="view-app-grid-symbolic"))
        self.app_row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
        self.app_row.connect("activated", lambda *_: self._pick_app())
        match.add(self.app_row)
        self.class_row = Adw.EntryRow(title="Window class")
        self.title_row = Adw.EntryRow(title="Window title (optional)")
        for r in (self.class_row, self.title_row):
            r.connect("changed", lambda *_: self._changed())
            match.add(r)
        prefs.add(match)

        # placement
        place = Adw.PreferencesGroup(title="When It Opens")
        for key, title, sub, _p in PLACEMENT_SWITCHES[:2]:
            place.add(self._switch(key, title, sub))
        self.size_exp = Adw.ExpanderRow(title="Size", subtitle="Open at a fixed size (floating windows)",
                                        show_enable_switch=True, enable_expansion=False)
        self.size_w = Adw.SpinRow.new_with_range(100, 7680, 10)
        self.size_w.set_title("Width")
        self.size_w.set_value(1200)
        self.size_h = Adw.SpinRow.new_with_range(100, 4320, 10)
        self.size_h.set_title("Height")
        self.size_h.set_value(800)
        self.size_exp.add_row(self.size_w)
        self.size_exp.add_row(self.size_h)
        place.add(self.size_exp)
        self.ws_exp = Adw.ExpanderRow(title="Workspace", subtitle="Always open on a certain workspace",
                                      show_enable_switch=True, enable_expansion=False)
        self.ws_num = Adw.SpinRow.new_with_range(1, 20, 1)
        self.ws_num.set_title("Workspace number")
        self.ws_silent = Adw.SwitchRow(title="Stay where I am", subtitle="Don't switch to that workspace")
        self.ws_exp.add_row(self.ws_num)
        self.ws_exp.add_row(self.ws_silent)
        place.add(self.ws_exp)
        for key, title, sub, _p in PLACEMENT_SWITCHES[2:]:
            place.add(self._switch(key, title, sub))
        prefs.add(place)

        # appearance
        look = Adw.PreferencesGroup(title="Appearance")
        self.op_exp = Adw.ExpanderRow(title="Opacity", subtitle="Make the window see-through",
                                      show_enable_switch=True, enable_expansion=False)
        self.op_active = Adw.SpinRow.new_with_range(10, 100, 5)
        self.op_active.set_title("When focused (%)")
        self.op_active.set_value(95)
        self.op_inactive = Adw.SpinRow.new_with_range(10, 100, 5)
        self.op_inactive.set_title("When unfocused (%)")
        self.op_inactive.set_value(85)
        self.op_exp.add_row(self.op_active)
        self.op_exp.add_row(self.op_inactive)
        look.add(self.op_exp)
        for key, title, sub, _p in APPEARANCE_SWITCHES:
            look.add(self._switch(key, title, sub))
        prefs.add(look)

        for w, sig in ((self.size_exp, "notify::enable-expansion"), (self.size_w, "notify::value"),
                       (self.size_h, "notify::value"), (self.ws_exp, "notify::enable-expansion"),
                       (self.ws_num, "notify::value"), (self.ws_silent, "notify::active"),
                       (self.op_exp, "notify::enable-expansion"), (self.op_active, "notify::value"),
                       (self.op_inactive, "notify::value")):
            w.connect(sig, lambda *_: self._changed())

        # name
        name_group = Adw.PreferencesGroup()
        self.name_row = Adw.EntryRow(title="Rule name (optional)")
        self.name_row.connect("changed", lambda *_: self._changed())
        name_group.add(self.name_row)
        prefs.add(name_group)

        if rule:
            rm_group = Adw.PreferencesGroup()
            rm = Gtk.Button(label="Remove Rule", halign=Gtk.Align.CENTER)
            rm.add_css_class("destructive-action")
            rm.add_css_class("pill")
            rm.connect("clicked", lambda *_: self._remove())
            rm_group.add(rm)
            prefs.add(rm_group)
            self._load(rule)
        self._loading = False
        self._changed()

    def _switch(self, key, title, subtitle):
        row = Adw.SwitchRow(title=title)
        if subtitle:
            row.set_subtitle(subtitle)
        row.connect("notify::active", lambda *_: self._changed())
        self._switches[key] = row
        return row

    def _load(self, rule):
        m, eff = rule.get("match") or {}, rule.get("effects") or {}
        self.class_row.set_text(str(m.get("class") or ""))
        self.title_row.set_text(str(m.get("title") or ""))
        self.name_row.set_text(rule.get("name") or "")
        for key, row in self._switches.items():
            row.set_active(eff.get(key) is True)
        if eff.get("size"):
            w, _, h = str(eff["size"]).partition(" ")
            try:
                self.size_w.set_value(float(w))
                self.size_h.set_value(float(h))
            except ValueError:
                pass
            self.size_exp.set_enable_expansion(True)
        if eff.get("workspace"):
            ws, _, silent = str(eff["workspace"]).partition(" ")
            try:
                self.ws_num.set_value(int(ws))
            except ValueError:
                pass
            self.ws_silent.set_active(silent == "silent")
            self.ws_exp.set_enable_expansion(True)
        if eff.get("opacity"):
            a, _, i = str(eff["opacity"]).partition(" ")
            try:
                self.op_active.set_value(float(a) * 100)
                self.op_inactive.set_value(float(i or a) * 100)
            except ValueError:
                pass
            self.op_exp.set_enable_expansion(True)

    def _pick_app(self):
        self.nav.push(PickerPage("Choose an App", app_entries(_apps()), self._set_app, placeholder="Search apps"))

    def _set_app(self, index):
        app = _apps()[index]
        self.class_row.set_text(f"^({re.escape(app['wm_class'])})$")

    def _data(self):
        match = dict(self.rule.get("match") or {}) if self.rule else {}
        for key, row in (("class", self.class_row), ("title", self.title_row)):
            text = row.get_text().strip()
            if text:
                match[key] = text
            else:
                match.pop(key, None)
        eff = {k: v for k, v in (self.rule.get("effects") or {}).items() if k not in KNOWN_EFFECTS} \
            if self.rule else {}
        for key, row in self._switches.items():
            if row.get_active():
                eff[key] = True
        if self.size_exp.get_enable_expansion():
            eff["size"] = f"{int(self.size_w.get_value())} {int(self.size_h.get_value())}"
        if self.ws_exp.get_enable_expansion():
            eff["workspace"] = str(int(self.ws_num.get_value())) + (" silent" if self.ws_silent.get_active() else "")
        if self.op_exp.get_enable_expansion():
            a, i = self.op_active.get_value() / 100, self.op_inactive.get_value() / 100
            eff["opacity"] = f"{a:g} {i:g}"
        return {"name": self.name_row.get_text().strip(), "match": match, "effects": eff,
                "enabled": self.rule.get("enabled", True) if self.rule else True}

    def _changed(self):
        if self._loading:
            return
        data = self._data()
        self.summary_label.set_label(summary(data) if data["match"] else "Choose an app or enter a window class")
        self.save_btn.set_sensitive(bool(data["match"]) and bool(data["effects"]))

    def _save(self):
        data = self._data()
        try:
            if self.rule:
                self.cm.update_window_rule(self.rule["id"], data)
            else:
                self.cm.add_window_rule(data)
        except (ValueError, KeyError) as e:
            self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(str(e)), timeout=4))
            return
        self.close()
        if self.on_saved:
            self.on_saved("Rule saved" if self.rule else "Rule added")

    def _remove(self):
        try:
            self.cm.remove_window_rule(self.rule["id"])
        except KeyError as e:
            self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(str(e)), timeout=4))
            return
        self.close()
        if self.on_saved:
            self.on_saved("Rule removed")


# ---------------------------------------------------------------- page

class WindowRulesPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        self.cm = cm
        self.toast_overlay = toast_overlay
        self._rows = []

        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=180)
        self.set_child(self.stack)

        add = Gtk.Button(label="Add Rule", halign=Gtk.Align.CENTER)
        add.add_css_class("suggested-action")
        add.add_css_class("pill")
        add.connect("clicked", lambda *_: self.open_dialog())
        self.empty = Adw.StatusPage(
            icon_name="focus-windows-symbolic", title="No Window Rules",
            description="Choose how apps open — floating, centered, at a set size or on their own workspace",
            child=add)
        self.empty.add_css_class("rules-empty")
        self.stack.add_named(self.empty, "empty")

        prefs = Adw.PreferencesPage(
            description="Rules are applied whenever a matching window opens.")
        self.group = Adw.PreferencesGroup(title="Your Rules")
        add2 = Gtk.Button(valign=Gtk.Align.CENTER)
        add2.set_child(Adw.ButtonContent(icon_name="list-add-symbolic", label="Add Rule"))
        add2.add_css_class("flat")
        add2.connect("clicked", lambda *_: self.open_dialog())
        self.group.set_header_suffix(add2)
        prefs.add(self.group)
        spacer = Adw.PreferencesGroup()
        spacer.set_size_request(-1, 40)
        prefs.add(spacer)
        self.stack.add_named(prefs, "list")
        self.refresh()

    def refresh(self):
        for r in self._rows:
            self.group.remove(r)
        self._rows = []
        try:
            rules = list(self.cm.list_window_rules())
        except Exception as e:
            rules = []
            self.toast(f"Couldn't read window rules: {e}")
        for rule in rules:
            match = rule.get("match") or {}
            app = app_for_class(match.get("class")) if match.get("class") else None
            row = Adw.ActionRow(title=GLib.markup_escape_text(rule.get("name") or target_name(match)),
                                subtitle=GLib.markup_escape_text(summary(rule)), activatable=True)
            row.set_subtitle_lines(2)
            if app and app["gicon"]:
                icon = Gtk.Image.new_from_gicon(app["gicon"])
                icon.set_pixel_size(32)
                icon.add_css_class("lowres-icon")
            else:
                icon = Gtk.Image(icon_name="focus-windows-symbolic", pixel_size=16,
                                 halign=Gtk.Align.CENTER, valign=Gtk.Align.CENTER)
                icon.set_size_request(32, 32)
                icon.add_css_class("rule-icon")
            row.add_prefix(icon)
            sw = Gtk.Switch(active=rule.get("enabled", True), valign=Gtk.Align.CENTER,
                            tooltip_text="Turn this rule on or off")
            sw.connect("notify::active", lambda s, _p, rid=rule["id"]: self._toggle(rid, s.get_active()))
            row.add_suffix(sw)
            row.add_suffix(Gtk.Image(icon_name="go-next-symbolic"))
            row.connect("activated", lambda _r, r=rule: self.open_dialog(r))
            if not rule.get("enabled", True):
                row.add_css_class("dim-row")
            self.group.add(row)
            self._rows.append(row)
        self.group.set_description(f"{len(rules)} rule{'s' if len(rules) != 1 else ''}" if rules else None)
        self.stack.set_visible_child_name("list" if rules else "empty")

    def _toggle(self, rule_id, active):
        try:
            self.cm.update_window_rule(rule_id, {"enabled": active})
        except (ValueError, KeyError) as e:
            self.toast(f"Couldn't change rule: {e}")

    def toast(self, text):
        self.toast_overlay.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=2))

    def open_dialog(self, rule=None):
        dlg = RuleDialog(self.cm, rule, on_saved=self.toast)
        dlg.present(self.get_root())
        return dlg

    # -- dev helpers -------------------------------------------------------

    def dev_open(self, name):
        """Screenshot helper: a builder filled in for Firefox."""
        dlg = self.open_dialog()
        dlg.class_row.set_text("^(firefox)$")
        dlg._switches["float"].set_active(True)
        dlg._switches["center"].set_active(True)
        dlg.size_exp.set_enable_expansion(True)
        dlg.size_exp.set_expanded(True)
        return dlg

    def dev_seed(self):
        """Screenshot helper: stage a few example rules (dry-run only)."""
        for data in (
            {"name": "", "match": {"class": "^(firefox)$"},
             "effects": {"float": True, "center": True, "size": "1200 800"}},
            {"name": "Music on workspace 9", "match": {"class": "^(spotify)$"},
             "effects": {"workspace": "9 silent"}},
            {"name": "See-through terminal", "match": {"class": "^(kitty)$"},
             "effects": {"opacity": "0.92 0.8", "no_shadow": True}, "enabled": False},
        ):
            self.cm.add_window_rule(data)
