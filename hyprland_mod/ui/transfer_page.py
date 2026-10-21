"""Import & Export: save the whole setup to a file, or bring one in."""

import datetime

from gi.repository import Adw, GLib, Gtk, Pango

from .. import presets
from .presets_page import dismissed, file_filter, filter_list, load_css, toast

SHOWN_CHANGES = 60


def _short(text):
    if text is None:
        return "your own config"
    text = str(text)
    return text if len(text) <= 38 else text[:36] + "…"


def _icon_tile(name):
    img = Gtk.Image(icon_name=name, valign=Gtk.Align.CENTER)
    img.add_css_class("transfer-icon")
    return img


class TransferPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        load_css()
        self.cm = cm
        self.toast_overlay = toast_overlay
        prefs = Adw.PreferencesPage(
            description="Move your whole Hyprland Mod setup to another computer, keep a copy "
                        "somewhere safe, or try someone else’s.")
        self.set_child(prefs)

        export = Adw.PreferencesGroup(title="Export Your Setup")
        self.export_row = Adw.ActionRow(title="Save to a file")
        self.export_row.add_prefix(_icon_tile("document-save-symbolic"))
        btn = Gtk.Button(label="Export…", valign=Gtk.Align.CENTER)
        btn.add_css_class("suggested-action")
        btn.connect("clicked", lambda *_: self.export_dialog())
        self.export_row.add_suffix(btn)
        self.export_row.set_activatable_widget(btn)
        export.add(self.export_row)
        prefs.add(export)

        imp = Adw.PreferencesGroup(
            title="Import a Setup",
            description="A backup of your current configuration is made first, so you can always "
                        "go back from the Backups page. Imported settings are staged: review them "
                        "in the preview, then press Apply — or Discard to cancel.")
        row = Adw.ActionRow(title="Open a .hyprmod file",
                            subtitle="Replaces everything you changed with Hyprland Mod; "
                                     "your own settings stay as they are")
        row.add_prefix(_icon_tile("document-open-symbolic"))
        ibtn = Gtk.Button(label="Import…", valign=Gtk.Align.CENTER)
        ibtn.connect("clicked", lambda *_: self.import_dialog())
        row.add_suffix(ibtn)
        row.set_activatable_widget(ibtn)
        imp.add(row)
        prefs.add(imp)

        info = Adw.PreferencesGroup(title="What’s Included")
        for icon, title, sub in (
                ("preferences-desktop-appearance-symbolic", "Settings changed in Hyprland Mod",
                 "Look, input, layouts and everything else you set here — including unapplied changes"),
                ("media-playback-start-symbolic", "Animation curves and speeds", "Bezier and animation lines"),
                ("security-high-symbolic", "Nothing that runs programs",
                 "Commands, key bindings and startup apps are never exported or imported")):
            r = Adw.ActionRow(title=title, subtitle=sub)
            r.add_prefix(Gtk.Image(icon_name=icon, valign=Gtk.Align.CENTER))
            info.add(r)
        prefs.add(info)
        bottom = Adw.PreferencesGroup()
        bottom.set_size_request(-1, 40)
        prefs.add(bottom)

        cm.subscribe(lambda *_: self._update_summary())
        self._update_summary()

    def _update_summary(self):
        try:
            state = self.cm.export_state()
        except Exception:
            state = {}
        n = len(state.get("settings", {}))
        lines = sum(len(v) for v in state.get("lists", {}).values())
        if not n and not lines:
            text = "Nothing changed with Hyprland Mod yet — the file would be empty"
        else:
            text = f"{n} setting{'s' if n != 1 else ''}"
            if lines:
                text += f" and {lines} animation line{'s' if lines != 1 else ''}"
        self.export_row.set_subtitle(text)

    # -- export ----------------------------------------------------------

    def export_dialog(self):
        name = f"hyprland-mod-setup-{datetime.date.today().isoformat()}{presets.SETUP_EXT}"
        dlg = Gtk.FileDialog(title="Export Setup", initial_name=name,
                             filters=filter_list(file_filter("Hyprland Mod setups", ["*.hyprmod"])))

        def done(d, res):
            try:
                f = d.save_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't export: {e.message}", 5)
                return
            self.export_to(f.get_path())
        dlg.save(self.get_root(), None, done)

    def export_to(self, path):
        try:
            presets.export_setup(self.cm, path)
        except OSError as e:
            toast(self.toast_overlay, f"Couldn't export: {e.strerror or e}", 5)
            return False
        toast(self.toast_overlay, f"Setup exported to {GLib.path_get_basename(path)}")
        return True

    # -- import ----------------------------------------------------------

    def import_dialog(self):
        dlg = Gtk.FileDialog(title="Import Setup", filters=filter_list(
            file_filter("Hyprland Mod setups", ["*.hyprmod", "*.json"]), file_filter("All files", ["*"])))

        def done(d, res):
            try:
                f = d.open_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't open the file: {e.message}", 5)
                return
            self.review(f.get_path())
        dlg.open(self.get_root(), None, done)

    def _alert(self, heading, body):
        dlg = Adw.AlertDialog(heading=heading, body=body)
        dlg.add_response("ok", "OK")
        dlg.present(self.get_root())
        return dlg

    def review(self, path):
        """Validate the file and show what would change before anything is staged."""
        try:
            state = presets.read_setup(path)
            changes = presets.setup_changes(self.cm, state)
        except presets.PresetError as e:
            return self._alert("Couldn't Import Setup", str(e))
        if not changes:
            return self._alert("Nothing to Import", "This setup is the same as what you have now.")
        n = len(changes)
        body = f"{n} setting{'s' if n != 1 else ''} will change. A backup is made first, and nothing " \
               "is saved until you press Apply."
        if self.cm.is_dirty:
            body += " Your unapplied changes will be replaced."
        dlg = Adw.AlertDialog(heading="Import This Setup?", body=body)
        dlg.set_extra_child(self._change_list(changes))
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("import", "Import")
        dlg.set_response_appearance("import", Adw.ResponseAppearance.SUGGESTED)
        dlg.set_default_response("import")
        dlg.set_close_response("cancel")
        dlg.connect("response", lambda _d, r: r == "import" and self._import(state, n))
        dlg.present(self.get_root())
        return dlg

    def _change_list(self, changes):
        lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        lb.add_css_class("boxed-list")
        lb.add_css_class("change-list")
        for ch in changes[:SHOWN_CHANGES]:
            box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2)
            name = Gtk.Label(label=ch.label, xalign=0, ellipsize=Pango.EllipsizeMode.END)
            name.add_css_class("heading")
            name.set_tooltip_text(ch.key)
            box.append(name)
            vals = Gtk.Box(spacing=6)
            old = Gtk.Label(label=_short(ch.old), xalign=0, ellipsize=Pango.EllipsizeMode.END)
            old.add_css_class("change-old")
            old.add_css_class("mono-value")
            arrow = Gtk.Image(icon_name="go-next-symbolic", pixel_size=12)
            arrow.add_css_class("dim-label")
            new = Gtk.Label(label=_short(ch.new), xalign=0, ellipsize=Pango.EllipsizeMode.END)
            new.add_css_class("change-new")
            new.add_css_class("mono-value")
            vals.append(old)
            vals.append(arrow)
            vals.append(new)
            box.append(vals)
            lb.append(box)
        if len(changes) > SHOWN_CHANGES:
            more = Gtk.Label(label=f"and {len(changes) - SHOWN_CHANGES} more", xalign=0)
            more.add_css_class("dim-label")
            lb.append(more)
        sw = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER, propagate_natural_height=True,
                                max_content_height=300, child=lb)
        return sw

    def _import(self, state, n):
        try:
            presets.import_setup(self.cm, state)
        except Exception as e:
            self._alert("Couldn't Import Setup", str(e))
            return
        toast(self.toast_overlay, f"Imported {n} change{'s' if n != 1 else ''} — Apply to keep them", 5)
