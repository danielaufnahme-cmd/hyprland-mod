"""Presets: a gallery of ready-made looks plus the user's own saved presets."""

import math
from pathlib import Path

import cairo
from gi.repository import Adw, Gdk, Gio, GLib, Gtk, Pango

from .. import presets
from . import colors
from .preview import LivePreview
from .rows import gradient_pattern

CSS_PATH = Path(__file__).resolve().parent.parent.parent / "styles" / "presets.css"
_css_loaded = False

FILTERS = [("all", "All"), ("dark", "Dark"), ("light", "Light"), ("minimal", "Minimal"),
           ("colorful", "Colorful"), ("animated", "Animated")]


def load_css():
    """Install styles/presets.css once per process (shared with the transfer page)."""
    global _css_loaded
    if _css_loaded or not CSS_PATH.exists():
        return
    _css_loaded = True
    provider = Gtk.CssProvider()
    provider.load_from_path(str(CSS_PATH))
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider,
                                              Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)


def toast(overlay, text, timeout=3, button=None, on_button=None):
    t = Adw.Toast(title=GLib.markup_escape_text(text), timeout=timeout)
    if button:
        t.set_button_label(button)
        t.connect("button-clicked", lambda *_: on_button())
    overlay.add_toast(t)
    return t


def file_filter(name, patterns):
    f = Gtk.FileFilter(name=name)
    for p in patterns:
        f.add_pattern(p)
    return f


def filter_list(*filters):
    store = Gio.ListStore.new(Gtk.FileFilter)
    for f in filters:
        store.append(f)
    return store


def dismissed(err):
    return isinstance(err, GLib.Error) and err.matches(Gtk.DialogError.quark(), Gtk.DialogError.DISMISSED)


def _hex(text):
    c = Gdk.RGBA()
    c.parse(text)
    return (c.red, c.green, c.blue, c.alpha)


def _luma(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def _rounded(cr, x, y, w, h, r):
    r = max(0.0, min(r, w / 2, h / 2))
    if r < 0.5:
        cr.rectangle(x, y, w, h)
        return
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


def _num(text, default=0.0):
    try:
        return float(str(text).split()[0])
    except (ValueError, IndexError, TypeError):
        return default


class PresetThumbnail(Gtk.DrawingArea):
    """Small cairo mock desktop drawn from a preset's own values (no ConfigManager needed)."""

    VIRTUAL_H = 210

    def __init__(self, preset):
        super().__init__(content_height=128, hexpand=True)
        self.add_css_class("preset-thumb")
        self.set_overflow(Gtk.Overflow.HIDDEN)
        self.set_can_target(False)
        self.preset = preset
        self.set_draw_func(self._draw)

    def _val(self, key, default):
        v = self.preset.settings.get(key)
        return default if v is None else v

    def _draw(self, _area, cr, w, h):
        if w <= 0 or h <= 0:
            return
        get = self._val
        meta = self.preset.preview or {}
        wall = [_hex(c) for c in meta.get("wallpaper") or ["#26384a", "#3b6074"]]
        s = h / self.VIRTUAL_H

        # wallpaper
        bg = cairo.LinearGradient(0, 0, w, h)
        for i, c in enumerate(wall):
            bg.add_color_stop_rgb(i / max(1, len(wall) - 1), *c[:3])
        cr.set_source(bg)
        cr.paint()
        glow = cairo.RadialGradient(w * 0.85, h * 0.1, 0, w * 0.85, h * 0.1, w * 0.7)
        glow.add_color_stop_rgba(0, 1, 1, 1, 0.16)
        glow.add_color_stop_rgba(1, 1, 1, 1, 0)
        cr.set_source(glow)
        cr.paint()
        avg = tuple(sum(c[i] for c in wall) / len(wall) for i in range(3))
        light_wall = _luma(avg) > 0.6

        # top bar
        bar_h = 16 * s
        _rounded(cr, 8 * s, 5 * s, w - 16 * s, bar_h, 5 * s)
        cr.set_source_rgba(*((1, 1, 1, 0.45) if light_wall else (0, 0, 0, 0.32)))
        cr.fill()
        for i in range(3):
            cr.arc(20 * s + i * 11 * s, 5 * s + bar_h / 2, (3 if i == 0 else 2.3) * s, 0, 2 * math.pi)
            cr.set_source_rgba(*((0.2, 0.2, 0.2, 0.8 if i == 0 else 0.35) if light_wall
                                 else (1, 1, 1, 0.85 if i == 0 else 0.35)))
            cr.fill()

        go = max(0.0, _num(get("general:gaps_out", "20"))) * s * 1.1
        gi = max(0.0, _num(get("general:gaps_in", "5"))) * s * 1.1
        top = 5 * s + bar_h + 2 * s
        ax, ay, aw, ah = go, top + go, w - 2 * go, h - top - 2 * go
        if aw < 30 or ah < 30:
            return
        left_w = (aw - 2 * gi) * 0.56
        rx, rw = ax + left_w + 2 * gi, aw - left_w - 2 * gi
        th = (ah - 2 * gi) * 0.5
        wins = [((rx, ay + th + 2 * gi, rw, ah - th - 2 * gi), False),
                ((rx, ay, rw, th), False),
                ((ax, ay, left_w, ah), True)]
        for rect, active in wins:
            self._window(cr, rect, active, s, wall, avg)

    def _window(self, cr, rect, active, s, wall, avg):
        get = self._val
        x, y, w, h = rect
        r = max(0.0, _num(get("decoration:rounding", "10"))) * s * 1.15
        bs = max(0.0, _num(get("general:border_size", "2"))) * s * 1.2
        if bs > 0:
            bs = max(1.0, bs)
        op = max(0.0, min(1.0, _num(get("decoration:active_opacity" if active else "decoration:inactive_opacity", "1"), 1)))
        blur = presets_bool(get("decoration:blur:enabled", "true"))

        # shadow
        if presets_bool(get("decoration:shadow:enabled", "true")):
            rng = _num(get("decoration:shadow:range", "4")) * s
            sc = colors.parse_color(get("decoration:shadow:color", "rgba(1a1a1aee)"))
            off = [_num(p) * s for p in str(get("decoration:shadow:offset", "0 0")).split()[:2]] + [0, 0]
            if presets_bool(get("decoration:shadow:sharp", "false")):
                _rounded(cr, x + off[0], y + off[1], w + rng, h + rng, r)
                cr.set_source_rgba(sc[0], sc[1], sc[2], sc[3] * 0.85)
                cr.fill()
            elif rng > 0.3:
                steps = max(4, min(12, int(rng)))
                for i in range(steps, 0, -1):
                    grow = rng * i / steps
                    a = sc[3] * (1 - i / (steps + 1)) ** 2 * 0.45
                    _rounded(cr, x - grow + off[0], y - grow + off[1], w + 2 * grow, h + 2 * grow, r + grow)
                    cr.set_source_rgba(sc[0], sc[1], sc[2], a)
                    cr.fill()

        # border ring (even-odd so translucent windows don't show the border fill)
        if bs > 0:
            key = "general:col.active_border" if active else "general:col.inactive_border"
            cols, deg = colors.parse_gradient(get(key, "rgba(ffffffff)"))
            _rounded(cr, x, y, w, h, r)
            _rounded(cr, x + bs, y + bs, w - 2 * bs, h - 2 * bs, max(0.0, r - bs))
            cr.set_fill_rule(cairo.FILL_RULE_EVEN_ODD)
            cr.set_source(gradient_pattern(cols, deg, x, y, w, h))
            cr.fill()
            cr.set_fill_rule(cairo.FILL_RULE_WINDING)

        ix, iy, iw, ih = x + bs, y + bs, w - 2 * bs, h - 2 * bs
        if iw <= 2 or ih <= 2:
            return
        cr.save()
        _rounded(cr, ix, iy, iw, ih, max(0.0, r - bs))
        cr.clip()
        meta = self.preset.preview or {}
        base = _hex(meta["window"]) if meta.get("window") else (0.12, 0.13, 0.15, 1)
        if op < 0.999 and blur:
            # frosted: a smoothed wash of the wallpaper behind the glass
            cr.set_source_rgba(*avg, 0.85)
            cr.paint()
            cr.set_source_rgba(1, 1, 1, 0.08)
            cr.paint()
        cr.set_source_rgba(base[0], base[1], base[2], op)
        cr.paint()
        light = _luma(base) > 0.6
        ink = (0.15, 0.15, 0.18) if light else (0.92, 0.93, 0.95)
        accent = colors.parse_gradient(get("general:col.active_border", "rgba(3584e4ff)"))[0][0]
        # title line + a few content lines
        cr.set_source_rgba(*ink, 0.07)
        cr.rectangle(ix, iy, iw, 14 * s)
        cr.fill()
        pad = 8 * s + max(0, r - bs) * 0.3
        yy = iy + 22 * s
        for i, frac in enumerate((0.5, 0.72, 0.38, 0.62, 0.45, 0.66, 0.3)):
            if yy + 5 * s > iy + ih - 6 * s:
                break
            lw = (iw - 2 * pad) * frac
            _rounded(cr, ix + pad, yy, max(2, lw), 4.5 * s, 2.25 * s)
            if i == 0 and active and _luma(accent) > 0.05:
                cr.set_source_rgba(accent[0], accent[1], accent[2], 0.9 * op + 0.1)
            else:
                cr.set_source_rgba(*ink, 0.28 * op + 0.08)
            cr.fill()
            yy += 11 * s
        if not active and presets_bool(get("decoration:dim_inactive", "false")):
            cr.set_source_rgba(0, 0, 0, max(0.0, min(1.0, _num(get("decoration:dim_strength", "0.5")))) * 0.7)
            cr.paint()
        cr.restore()


def presets_bool(text):
    return str(text).strip().lower() in ("1", "true", "yes", "on")


class PresetCard(Gtk.FlowBoxChild):
    def __init__(self, preset, page):
        super().__init__()
        self.preset = preset
        self.add_css_class("preset-child")
        self.set_tooltip_text(preset.description or f"Try “{preset.name}”")
        self.update_property([Gtk.AccessibleProperty.LABEL],
                             [f"{preset.name}. {preset.description}"])

        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, width_request=200)
        card.add_css_class("preset-card")
        self.card = card

        ov = Gtk.Overlay(child=PresetThumbnail(preset))
        self.badge = Gtk.Label(label="In use", halign=Gtk.Align.START, valign=Gtk.Align.START,
                               can_target=False, visible=False)
        self.badge.add_css_class("preset-badge")
        ov.add_overlay(self.badge)
        card.append(ov)

        body = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4,
                       margin_start=14, margin_end=10, margin_top=10, margin_bottom=14)
        top = Gtk.Box(spacing=6)
        name = Gtk.Label(label=preset.name, xalign=0, hexpand=True, ellipsize=Pango.EllipsizeMode.END,
                         max_width_chars=10)
        name.add_css_class("preset-name")
        top.append(name)
        if not preset.builtin:
            top.append(page.user_menu_button(preset))
        body.append(top)
        if preset.description:
            desc = Gtk.Label(label=preset.description, xalign=0, wrap=True, lines=2, ellipsize=Pango.EllipsizeMode.END,
                             natural_wrap_mode=Gtk.NaturalWrapMode.WORD, width_chars=12, max_width_chars=12)
            desc.add_css_class("preset-desc")
            body.append(desc)
        tags = list(preset.tags[:4]) or ([] if preset.builtin else ["mine"])
        if tags:
            wrap = Adw.WrapBox(child_spacing=4, line_spacing=4, margin_top=4, natural_line_length=160)
            for t in tags:
                lbl = Gtk.Label(label=t)
                lbl.add_css_class("preset-tag")
                wrap.append(lbl)
            body.append(wrap)
        card.append(body)
        self.set_child(card)

    def set_state(self, state):
        """state: None, "current" or "trying"."""
        was = self.badge.get_visible()
        self.badge.set_visible(state is not None)
        if state:
            self.badge.set_label("Previewing" if state == "trying" else "In use")
            if not was:
                self.badge.remove_css_class("pop")
                self.badge.add_css_class("pop")
            self.card.add_css_class("current")
        else:
            self.card.remove_css_class("current")

    def matches_filter(self, tag, query):
        p = self.preset
        if tag != "all" and tag not in p.tags:
            return False
        if query:
            hay = " ".join([p.name, p.description, p.author, *p.tags]).lower()
            return all(word in hay for word in query.split())
        return True


def section_header(title, count_label=None):
    box = Gtk.Box(spacing=6, valign=Gtk.Align.CENTER, hexpand=True)
    lbl = Gtk.Label(label=title, xalign=0)
    lbl.add_css_class("presets-section-title")
    box.append(lbl)
    if count_label is not None:
        count_label.add_css_class("presets-section-count")
        count_label.set_valign(Gtk.Align.CENTER)
        box.append(count_label)
    return box


def _grid():
    fb = Gtk.FlowBox(selection_mode=Gtk.SelectionMode.NONE, homogeneous=True, min_children_per_line=1,
                     max_children_per_line=4, column_spacing=18, row_spacing=18,
                     activate_on_single_click=True, valign=Gtk.Align.START)
    fb.add_css_class("preset-grid")
    return fb


class PresetsPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        load_css()
        self.cm = cm
        self.toast_overlay = toast_overlay
        self.options = presets.schema_options()
        self._tag = "all"
        self._query = ""
        self._refresh_id = 0
        self.cards = []
        try:
            self.builtin = presets.load_builtin()
        except presets.PresetError as e:  # a broken install: show the rest of the page anyway
            self.builtin = []
            GLib.idle_add(lambda: toast(self.toast_overlay, f"Couldn't load built-in presets: {e}") and False)

        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=22,
                        margin_top=24, margin_bottom=72, margin_start=18, margin_end=18)
        clamp = Adw.Clamp(maximum_size=1080, tightening_threshold=860, child=outer)
        scroller = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER, child=clamp, vexpand=True)
        self.set_child(scroller)

        # header
        head = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        head.add_css_class("presets-header")
        title = Gtk.Label(label="Presets", xalign=0)
        title.add_css_class("title-1")
        sub = Gtk.Label(label="Pick a look to preview it instantly. Press Apply to keep it, "
                              "or Discard to go back.", xalign=0, wrap=True)
        sub.add_css_class("dim-label")
        head.append(title)
        head.append(sub)
        outer.append(head)

        # live preview
        self.preview = LivePreview(cm)
        self.preview.set_options(self.options)
        self.preview.set_content_height(190)
        frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        frame.add_css_class("presets-preview")
        frame.set_overflow(Gtk.Overflow.HIDDEN)
        pov = Gtk.Overlay(child=self.preview)
        badge = Gtk.Label(label="Live preview", halign=Gtk.Align.START, valign=Gtk.Align.END, can_target=False)
        badge.add_css_class("preview-badge")
        pov.add_overlay(badge)
        frame.append(pov)
        outer.append(frame)

        # search + filter chips
        bar = Adw.WrapBox(child_spacing=6, line_spacing=8)
        self.search = Gtk.SearchEntry(placeholder_text="Search presets", width_chars=12)
        self.search.update_property([Gtk.AccessibleProperty.LABEL], ["Search presets"])
        self.search.connect("search-changed", self._on_search)
        self.search.set_margin_end(8)
        bar.append(self.search)
        first = None
        for tag, label in FILTERS:
            chip = Gtk.ToggleButton(label=label, valign=Gtk.Align.CENTER)
            chip.add_css_class("preset-chip")
            chip.add_css_class("flat")
            if first is None:
                first = chip
                chip.set_active(True)
            else:
                chip.set_group(first)
            chip.connect("toggled", lambda b, t=tag: b.get_active() and self._set_tag(t))
            bar.append(chip)
        outer.append(bar)

        # built-in section
        self.builtin_count = Gtk.Label()
        outer.append(section_header("Built-in", self.builtin_count))
        self.builtin_grid = _grid()
        self.builtin_grid.connect("child-activated", lambda _fb, child: self.try_preset(child.preset))
        for p in self.builtin:
            card = PresetCard(p, self)
            self.cards.append(card)
            self.builtin_grid.append(card)
        self.builtin_grid.set_filter_func(self._filter)
        outer.append(self.builtin_grid)
        self.no_match = Adw.StatusPage(icon_name="edit-find-symbolic", title="No Matching Presets",
                                       description="Try another word or filter", visible=False)
        self.no_match.add_css_class("compact")
        outer.append(self.no_match)

        # user section
        mine_head = Gtk.Box(spacing=6, margin_top=10)
        self.user_count = Gtk.Label()
        mine_head.append(section_header("My Presets", self.user_count))
        imp = Gtk.Button(child=Adw.ButtonContent(icon_name="document-open-symbolic", label="Import…"),
                         tooltip_text="Add a preset someone shared with you (.hyprmod-preset)")
        imp.connect("clicked", lambda *_: self.import_dialog())
        save = Gtk.Button(child=Adw.ButtonContent(icon_name="document-save-symbolic",
                                                  label="Save Current Look…"),
                          tooltip_text="Save your current look as a preset you can reuse or share")
        save.add_css_class("suggested-action")
        save.connect("clicked", lambda *_: self.save_dialog())
        mine_head.append(imp)
        mine_head.append(save)
        outer.append(mine_head)
        self.user_grid = _grid()
        self.user_grid.connect("child-activated", lambda _fb, child: self.try_preset(child.preset))
        self.user_grid.set_filter_func(self._filter)
        outer.append(self.user_grid)
        self.user_empty = self._empty_state()
        outer.append(self.user_empty)
        self.user_cards = []
        self.reload_user()

        cm.subscribe(self._on_cm_event)
        self.connect("map", lambda *_: (self.preview.update(animate=False), self._refresh_state()))

    # -- building blocks -------------------------------------------------

    def _empty_state(self):
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, halign=Gtk.Align.FILL)
        box.add_css_class("preset-empty")
        icon = Gtk.Image(icon_name="starred-symbolic", pixel_size=32)
        box.append(icon)
        t = Gtk.Label(label="No presets of your own yet")
        t.add_css_class("heading")
        box.append(t)
        d = Gtk.Label(label="Save your current look to reuse it later or share it with friends.",
                      wrap=True, justify=Gtk.Justification.CENTER)
        d.add_css_class("dim-label")
        box.append(d)
        return box

    def user_menu_button(self, preset):
        menu = Gio.Menu()
        menu.append("Export / Share…", "card.export")
        menu.append("Rename…", "card.rename")
        section = Gio.Menu()
        section.append("Delete", "card.delete")
        menu.append_section(None, section)
        btn = Gtk.MenuButton(icon_name="view-more-symbolic", menu_model=menu, valign=Gtk.Align.CENTER,
                             tooltip_text="More")
        btn.add_css_class("flat")
        btn.add_css_class("circular")
        btn.add_css_class("preset-menu-button")
        group = Gio.SimpleActionGroup()
        for name, cb in (("export", self.export_dialog), ("rename", self.rename_dialog),
                         ("delete", self.delete_dialog)):
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", lambda *_a, f=cb: f(preset))
            group.add_action(act)
        btn.insert_action_group("card", group)
        return btn

    def reload_user(self):
        for c in self.user_cards:
            self.user_grid.remove(c)
        self.user_cards = []
        user, problems = presets.load_user(self.cm)
        for p in user:
            card = PresetCard(p, self)
            self.user_cards.append(card)
            self.user_grid.append(card)
        self.user_grid.set_visible(bool(user))
        self.user_empty.set_visible(not user)
        self.user_count.set_label(str(len(user)))
        self.user_count.set_visible(bool(user))
        if problems:
            toast(self.toast_overlay, f"Skipped a damaged preset — {problems[0]}", 5)
        self._apply_filter()
        self._refresh_state()

    # -- filtering -------------------------------------------------------

    def _filter(self, child):
        return child.matches_filter(self._tag, self._query)

    def _set_tag(self, tag):
        self._tag = tag
        self._apply_filter()

    def _on_search(self, entry):
        self._query = entry.get_text().strip().lower()
        self._apply_filter()

    def _apply_filter(self):
        self.builtin_grid.invalidate_filter()
        self.user_grid.invalidate_filter()
        shown = sum(1 for c in self.cards if c.matches_filter(self._tag, self._query))
        self.builtin_count.set_label(str(shown) if shown != len(self.cards) else str(len(self.cards)))
        self.builtin_grid.set_visible(shown > 0)
        self.no_match.set_visible(shown == 0)

    # -- state -----------------------------------------------------------

    def _on_cm_event(self, _event, key=None):
        # staging a preset fires one event per key: settle once afterwards
        if not self._refresh_id:
            self._refresh_id = GLib.idle_add(self._refresh_idle)

    def _refresh_idle(self):
        self._refresh_id = 0
        self.preview.update()
        self._refresh_state()
        return GLib.SOURCE_REMOVE

    def _refresh_state(self):
        try:
            pending = set(self.cm.pending()) | set(getattr(self.cm, "pending_resets", lambda: [])())
        except Exception:
            pending = set()
        for card in self.cards + self.user_cards:
            try:
                match = card.preset.matches(self.cm, self.options)
            except Exception:
                match = False
            if not match:
                card.set_state(None)
            else:
                card.set_state("trying" if pending & set(card.preset.settings) else "current")

    # -- actions ---------------------------------------------------------

    def try_preset(self, preset):
        if preset.matches(self.cm, self.options):
            toast(self.toast_overlay, f"“{preset.name}” is already your look", 2)
            return
        before = self.cm.export_state()
        try:
            presets.stage_preset(self.cm, preset)
        except Exception as e:
            try:
                self.cm.import_state(before)
            except Exception:
                pass
            toast(self.toast_overlay, f"Couldn't use “{preset.name}”: {e}", 5)
            return
        toast(self.toast_overlay, f"Previewing “{preset.name}” — Apply to keep it", 4,
              "Undo", lambda: self.cm.import_state(before))

    def _entry_dialog(self, heading, body, response_label, fields, on_ok):
        """AlertDialog with entry rows; ``fields`` is [(title, initial, required)]."""
        dlg = Adw.AlertDialog(heading=heading, body=body)
        group = Adw.PreferencesGroup(margin_top=6)
        rows = []
        for title, initial, required in fields:
            row = Adw.EntryRow(title=title, text=initial or "")
            row.required = required
            group.add(row)
            rows.append(row)
        dlg.set_extra_child(group)
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("ok", response_label)
        dlg.set_response_appearance("ok", Adw.ResponseAppearance.SUGGESTED)
        dlg.set_default_response("ok")
        dlg.set_close_response("cancel")

        def validate(*_):
            dlg.set_response_enabled("ok", all(r.get_text().strip() or not r.required for r in rows))
        for r in rows:
            r.connect("changed", validate)
            r.connect("entry-activated", lambda *_: dlg.get_response_enabled("ok") and (
                dlg.close(), on_ok([x.get_text().strip() for x in rows])))
        validate()
        dlg.connect("response", lambda _d, resp: resp == "ok" and on_ok([x.get_text().strip() for x in rows]))
        dlg.present(self.get_root())
        rows[0].grab_focus()
        return dlg

    def save_dialog(self):
        self._entry_dialog(
            "Save Current Look", "Saves your colors, borders, gaps, blur, shadows and animations "
            "as a preset you can reuse or share.", "Save",
            [("Name", "", True), ("Short description (optional)", "", False)], self._save)

    def _save(self, values):
        name, desc = values
        try:
            preset = presets.capture(self.cm, name, desc, options=self.options)
            presets.save_user(preset, self.cm)
        except (presets.PresetError, OSError) as e:
            toast(self.toast_overlay, f"Couldn't save the preset: {e}", 5)
            return
        self.reload_user()
        toast(self.toast_overlay, f"Saved “{preset.name}” to My Presets")

    def rename_dialog(self, preset):
        self._entry_dialog("Rename Preset", None, "Rename", [("Name", preset.name, True)],
                           lambda v: self._rename(preset, v[0]))

    def _rename(self, preset, name):
        try:
            presets.rename_user(preset, name, self.cm)
        except (presets.PresetError, OSError) as e:
            toast(self.toast_overlay, f"Couldn't rename: {e}", 5)
            return
        self.reload_user()

    def delete_dialog(self, preset):
        dlg = Adw.AlertDialog(heading="Delete Preset?",
                              body=f"“{GLib.markup_escape_text(preset.name)}” will be removed from "
                                   "My Presets. Your current setup isn't affected.", body_use_markup=True)
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("delete", "Delete")
        dlg.set_response_appearance("delete", Adw.ResponseAppearance.DESTRUCTIVE)
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")
        dlg.connect("response", lambda _d, r: r == "delete" and self._delete(preset))
        dlg.present(self.get_root())

    def _delete(self, preset):
        try:
            presets.delete_user(preset, self.cm)
        except (presets.PresetError, OSError) as e:
            toast(self.toast_overlay, f"Couldn't delete: {e}", 5)
            return
        self.reload_user()
        toast(self.toast_overlay, f"Deleted “{preset.name}”", 2)

    def export_dialog(self, preset):
        dlg = Gtk.FileDialog(title="Export Preset", initial_name=f"{preset.id}{presets.PRESET_EXT}",
                             filters=filter_list(file_filter("Hyprland Mod presets", ["*.hyprmod-preset"])))

        def done(d, res):
            try:
                f = d.save_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't export: {e.message}", 5)
                return
            try:
                presets.export_preset(preset, f.get_path())
            except OSError as e:
                toast(self.toast_overlay, f"Couldn't export: {e.strerror or e}", 5)
                return
            toast(self.toast_overlay, f"Exported “{preset.name}” — share the file with anyone")
        dlg.save(self.get_root(), None, done)

    def import_dialog(self):
        dlg = Gtk.FileDialog(title="Import Preset", filters=filter_list(
            file_filter("Hyprland Mod presets", ["*.hyprmod-preset", "*.json"]),
            file_filter("All files", ["*"])))

        def done(d, res):
            try:
                f = d.open_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't open the file: {e.message}", 5)
                return
            self.import_file(f.get_path())
        dlg.open(self.get_root(), None, done)

    def import_file(self, path):
        try:
            preset = presets.import_preset(path, self.cm)
        except (presets.PresetError, OSError) as e:
            dlg = Adw.AlertDialog(heading="Couldn't Import Preset", body=str(e))
            dlg.add_response("ok", "OK")
            dlg.present(self.get_root())
            return None
        self.reload_user()
        toast(self.toast_overlay, f"Imported “{preset.name}”", 4, "Try It", lambda: self.try_preset(preset))
        return preset
