"""Backups: create snapshots and restore them."""

import datetime

from gi.repository import Adw, GLib, Gtk


def _when(created):
    try:
        if isinstance(created, (int, float)):
            dt = datetime.datetime.fromtimestamp(created)
        else:
            dt = datetime.datetime.fromisoformat(str(created))
    except (ValueError, TypeError, OSError):
        return str(created)
    now = datetime.datetime.now()
    if dt.date() == now.date():
        day = "Today"
    elif dt.date() == (now - datetime.timedelta(days=1)).date():
        day = "Yesterday"
    elif dt.year == now.year:
        day = dt.strftime("%A, %-d %B")
    else:
        day = dt.strftime("%-d %B %Y")
    return f"{day} at {dt.strftime('%H:%M')}"


class BackupsPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        self.cm = cm
        self.toast_overlay = toast_overlay
        prefs = Adw.PreferencesPage(
            description="A backup is a snapshot of your whole Hyprland configuration. "
                        "Restore one any time to go back.")
        self.set_child(prefs)

        create = Adw.PreferencesGroup(title="New Backup")
        self.label_row = Adw.EntryRow(title="Name (optional)")
        self.label_row.connect("entry-activated", lambda *_: self._create())
        btn = Gtk.Button(label="Create Backup", valign=Gtk.Align.CENTER)
        btn.add_css_class("suggested-action")
        btn.connect("clicked", lambda *_: self._create())
        self.label_row.add_suffix(btn)
        create.add(self.label_row)
        prefs.add(create)

        self.list_group = Adw.PreferencesGroup(title="Saved Backups")
        prefs.add(self.list_group)
        self._rows = []
        self.refresh()

    def _toast(self, text):
        self.toast_overlay.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=3))

    def refresh(self):
        for r in self._rows:
            self.list_group.remove(r)
        self._rows = []
        try:
            backups = list(self.cm.list_backups())
        except Exception as e:  # core error: show instead of crashing
            backups = []
            self._toast(f"Couldn't read backups: {e}")
        self.list_group.set_description(
            f"{len(backups)} backup{'s' if len(backups) != 1 else ''}" if backups else None)
        if not backups:
            row = Adw.ActionRow(title="No backups yet",
                                subtitle="Create one before experimenting — it only takes a second")
            row.add_css_class("empty-row")
            row.add_prefix(Gtk.Image(icon_name="document-open-recent-symbolic"))
            self.list_group.add(row)
            self._rows.append(row)
            return
        for b in backups:
            label = (b.get("label") or "").strip()
            files = b.get("files") or []
            when = _when(b.get("created"))
            row = Adw.ActionRow(title=GLib.markup_escape_text(label or when),
                                subtitle=GLib.markup_escape_text(
                                    (when + " · " if label else "")
                                    + f"{len(files)} file{'s' if len(files) != 1 else ''}"))
            icon = Gtk.Image(icon_name="document-open-recent-symbolic")
            icon.add_css_class("backup-icon")
            row.add_prefix(icon)
            restore = Gtk.Button(label="Restore", valign=Gtk.Align.CENTER)
            restore.connect("clicked", lambda _b, bk=b, w=when: self._confirm(bk, w))
            row.add_suffix(restore)
            self.list_group.add(row)
            self._rows.append(row)

    def _create(self):
        label = self.label_row.get_text().strip()
        try:
            self.cm.create_backup(label)
        except Exception as e:
            self._toast(f"Couldn't create backup: {e}")
            return
        self.label_row.set_text("")
        self.refresh()
        self._toast("Backup created")

    def _confirm(self, backup, when):
        body = f"Your configuration will be replaced with the backup from {when}."
        if getattr(self.cm, "is_dirty", False):
            body += " Unsaved changes will be lost."
        dlg = Adw.AlertDialog(heading="Restore Backup?", body=body)
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("restore", "Restore")
        dlg.set_response_appearance("restore", Adw.ResponseAppearance.DESTRUCTIVE)
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")
        dlg.connect("response", lambda _d, r: r == "restore" and self._restore(backup))
        dlg.present(self.get_root())

    def _restore(self, backup):
        try:
            res = self.cm.restore_backup(backup["id"])
        except Exception as e:
            self._toast(f"Couldn't restore backup: {e}")
            return
        if res is False or (hasattr(res, "ok") and not res.ok):
            errs = getattr(res, "errors", None) or []
            self._toast(f"Couldn't restore backup: {errs[0]}" if errs else "Couldn't restore backup")
            return
        self.refresh()
        self._toast("Backup restored")
