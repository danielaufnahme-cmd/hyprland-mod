"""A preferences page generated from one schema page."""

from gi.repository import Adw, GLib, Gtk

from .preview import LivePreview
from .rows import build_row

PREVIEW_PAGES = {"general", "decoration"}


def bottom_spacer():
    """Room below the last group so the floating apply bar never hides a row."""
    g = Adw.PreferencesGroup()
    g.set_size_request(-1, 40)
    return g


class SettingsPage(Adw.Bin):
    def __init__(self, cm, page, all_opts):
        super().__init__()
        self.cm = cm
        self.page_id = page["id"]
        self.title = page.get("title", self.page_id)
        self.rows = []
        self.preview = None
        self._advanced_rows = []
        self._advanced_groups = []

        prefs = Adw.PreferencesPage()
        if page.get("description"):
            prefs.set_description(page["description"])
        self.set_child(prefs)

        if self.page_id in PREVIEW_PAGES:
            self.preview = LivePreview(cm)
            self.preview.set_options(all_opts)
            frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
            frame.add_css_class("preview-card")
            frame.set_overflow(Gtk.Overflow.HIDDEN)
            ov = Gtk.Overlay(child=self.preview)
            badge = Gtk.Label(label="Live preview", halign=Gtk.Align.START, valign=Gtk.Align.END,
                              can_target=False)
            badge.add_css_class("preview-badge")
            ov.add_overlay(badge)
            frame.append(ov)
            group = Adw.PreferencesGroup()
            group.add_css_class("preview-group")
            group.add(frame)
            prefs.add(group)

        for g in page.get("groups", []):
            group = Adw.PreferencesGroup(title=GLib.markup_escape_text(g.get("title", "")))
            if g.get("description"):
                group.set_description(GLib.markup_escape_text(g["description"]))
            opts = g.get("options", [])
            for opt in opts:
                row = build_row(cm, opt)
                group.add(row.row)
                self.rows.append(row)
                if opt.get("advanced"):
                    self._advanced_rows.append(row)
            if opts and all(o.get("advanced") for o in opts):
                self._advanced_groups.append(group)
            prefs.add(group)

        if self._advanced_rows:
            adv = Adw.PreferencesGroup()
            adv.add_css_class("advanced-toggle-group")
            sw = Adw.SwitchRow(title="Show advanced options",
                               subtitle=f"{len(self._advanced_rows)} rarely needed settings for fine-tuning")
            sw.add_prefix(Gtk.Image(icon_name="emblem-system-symbolic"))
            sw.connect("notify::active", lambda r, _p: self.set_show_advanced(r.get_active()))
            self.advanced_switch = sw
            adv.add(sw)
            prefs.add(adv)
            self.set_show_advanced(False, animate=False)
        prefs.add(bottom_spacer())

    def set_show_advanced(self, show, animate=True):
        for w in self._advanced_rows + self._advanced_groups:
            w.set_visible(show)
            if show and animate:
                w.remove_css_class("reveal-in")
                w.add_css_class("reveal-in")
            else:
                w.remove_css_class("reveal-in")
