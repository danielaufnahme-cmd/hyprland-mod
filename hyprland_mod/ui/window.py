"""Main application window."""

from gi.repository import Adw, Gio, GLib, Graphene, Gtk

from .apply_bar import ApplyBar
from .backend import is_dry_run, pending_count
from .backups_page import BackupsPage
from .keybinds_page import KeybindsPage
from .monitors_page import MonitorsPage
from .presets_page import PresetsPage
from .search_page import SearchPage
from .settings_page import SettingsPage
from .sidebar import FIXED_PAGES, PAGE_INFO, Sidebar
from .transfer_page import TransferPage
from .window_rules_page import WindowRulesPage


def _status_window_content(title, description, icon):
    tv = Adw.ToolbarView()
    tv.add_top_bar(Adw.HeaderBar())
    tv.set_content(Adw.StatusPage(title=title, description=description, icon_name=icon))
    return tv


class MainWindow(Adw.ApplicationWindow):
    def __init__(self, app, cm=None, schema=None, error=None):
        super().__init__(application=app, title="Hyprland Mod", default_width=1120, default_height=780)
        self.set_size_request(360, 480)
        self.cm = cm
        self.ready = False

        if error is not None:
            self.set_content(_status_window_content(
                "Hyprland Mod Couldn't Start", GLib.markup_escape_text(str(error)), "dialog-error-symbolic"))
            return
        if not getattr(cm, "hyprland_running", False) and not is_dry_run(cm):
            self.set_content(_status_window_content(
                "Hyprland Mod Only Works on Hyprland",
                "Log in to a Hyprland session and open Hyprland Mod again.", "computer-fail-symbolic"))
            return

        self.ready = True
        self.schema = sorted(schema, key=lambda p: (p.get("order", 0), p.get("title", "")))
        self.all_opts = {o["key"]: o for p in self.schema for g in p.get("groups", []) for o in g.get("options", [])}
        self._titles = {p["id"]: p.get("title", p["id"]) for p in self.schema}
        self._titles.update({pid: t for pid, t, _i in FIXED_PAGES})
        self._current = self.schema[0]["id"] if self.schema else "backups"
        self._search_timer = 0

        self.toasts = Adw.ToastOverlay()

        # content side
        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=180,
                               vexpand=True, hexpand=True)
        self.settings_pages = []
        self._page_specs = {p["id"]: p for p in self.schema}
        self.search_page = SearchPage(cm, self.schema, self.open_page, extra_pages=PAGE_INFO)
        self.stack.add_named(self.search_page, "search")
        self.stack.add_named(PresetsPage(cm, self.toasts), "presets")
        self.stack.add_named(TransferPage(cm, self.toasts), "transfer")
        self.backups_page = BackupsPage(cm, self.toasts)
        self.stack.add_named(self.backups_page, "backups")
        # built on first visit (the bar page manages a process, displays read the monitors)
        self._lazy = {"bar": self._make_bar_page, "keybinds": KeybindsPage, "rules": WindowRulesPage,
                      "monitors": MonitorsPage}
        self._item_pages = {}

        self.apply_bar = ApplyBar(self.discard, self.apply)
        self.apply_bar.set_margin_bottom(18)
        overlay = Gtk.Overlay(child=self.stack)
        overlay.add_overlay(self.apply_bar)

        self.title_widget = Adw.WindowTitle()
        header = Adw.HeaderBar(title_widget=self.title_widget)
        self.undo_btn = Gtk.Button(icon_name="edit-undo-symbolic", tooltip_text="Undo (Ctrl+Z)",
                                   action_name="win.undo")
        self.redo_btn = Gtk.Button(icon_name="edit-redo-symbolic", tooltip_text="Redo (Ctrl+Shift+Z)",
                                   action_name="win.redo")
        header.pack_start(self.undo_btn)
        header.pack_start(self.redo_btn)
        content_tv = Adw.ToolbarView(content=overlay)
        content_tv.add_top_bar(header)
        self.content_page = Adw.NavigationPage(title="Settings", tag="content", child=content_tv)

        menu = Gio.Menu()
        menu.append("Keyboard Shortcuts", "win.show-help-overlay")
        menu.append("About Hyprland Mod", "app.about")
        self.sidebar = Sidebar(self.schema, self._on_sidebar_select, self._on_search_changed, menu)

        self.split = Adw.NavigationSplitView(sidebar=self.sidebar, content=self.content_page,
                                             min_sidebar_width=230, max_sidebar_width=290,
                                             sidebar_width_fraction=0.26)
        self.toasts.set_child(self.split)
        self.set_content(self.toasts)

        bp = Adw.Breakpoint.new(Adw.BreakpointCondition.parse("max-width: 680sp"))
        bp.add_setter(self.split, "collapsed", True)
        self.add_breakpoint(bp)

        self.sidebar.search.connect("activate", lambda *_: self.split.set_show_content(True))
        self._install_actions()
        cm.subscribe(self._on_cm_event)
        self._show(self._current)
        self.sidebar.select(self._current)
        self._sync_state()

    # -- actions ---------------------------------------------------------

    def _install_actions(self):
        for name, cb in (("undo", lambda *_: self._history("undo")), ("redo", lambda *_: self._history("redo")),
                         ("apply", lambda *_: self.apply()), ("discard", lambda *_: self.discard()),
                         ("search", lambda *_: self.focus_search())):
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", cb)
            self.add_action(act)
        app = self.get_application()
        app.set_accels_for_action("win.undo", ["<Control>z"])
        app.set_accels_for_action("win.redo", ["<Control><Shift>z", "<Control>y"])
        app.set_accels_for_action("win.apply", ["<Control>s"])
        app.set_accels_for_action("win.search", ["<Control>f"])
        shortcuts = Gio.SimpleAction.new("show-help-overlay", None)
        shortcuts.connect("activate", lambda *_: self._show_shortcuts())
        self.add_action(shortcuts)

    def toast(self, text, timeout=3):
        self.toasts.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=timeout))

    def _history(self, which):
        try:
            res = getattr(self.cm, which)()
        except Exception as e:
            self.toast(str(e))
            return
        errors = getattr(res, "errors", None)
        if res is not None and not getattr(res, "ok", True):
            self.toast(errors[0] if errors else f"Couldn't {which}")
        else:
            self.toast("Undone" if which == "undo" else "Redone", 2)

    def _show_shortcuts(self):
        dlg = Adw.ShortcutsDialog()
        sec = Adw.ShortcutsSection(title="General")
        for title, accel in (("Search", "<Control>f"), ("Apply changes", "<Control>s"),
                             ("Undo", "<Control>z"), ("Redo", "<Control><Shift>z")):
            sec.add(Adw.ShortcutsItem.new(title, accel))
        dlg.add(sec)
        dlg.present(self)

    def focus_search(self):
        self.split.set_show_content(False)
        self.sidebar.search.grab_focus()

    def _action(self, name):
        return self.lookup_action(name)

    # -- navigation ------------------------------------------------------

    def _make_bar_page(self, cm, toasts):
        from .bar_page import BarPage
        page = BarPage(cm, toasts)
        if hasattr(cm, "set_autostart") and hasattr(cm, "get_autostart"):
            page.set_autostart_handler(lambda: bool(cm.get_autostart("bar")),
                                       lambda on, command: cm.set_autostart("bar", command if on else None))
        return page

    def _ensure_page(self, pid):
        if pid in self._lazy and pid not in self._item_pages:
            try:
                page = self._lazy[pid](self.cm, self.toasts)
            except Exception as e:  # one broken page must not take the window down
                import traceback
                traceback.print_exc()
                page = Adw.StatusPage(icon_name="dialog-error-symbolic", title="This Page Couldn't Load",
                                      description=GLib.markup_escape_text(str(e)))
            self._item_pages[pid] = page
            self.stack.add_named(page, pid)
            return
        spec = self._page_specs.get(pid)
        if spec is not None and self.stack.get_child_by_name(pid) is None:
            sp = SettingsPage(self.cm, spec, self.all_opts)
            self.settings_pages.append(sp)
            self.stack.add_named(sp, pid)

    def _show(self, pid):
        self._ensure_page(pid)
        self.stack.set_visible_child_name(pid)
        if pid == "search":
            self.title_widget.set_title("Search")
            self.title_widget.set_subtitle("")
        else:
            self.title_widget.set_title(self._titles.get(pid, pid))
            self.title_widget.set_subtitle("")
        self.content_page.set_title(self.title_widget.get_title())

    def _on_sidebar_select(self, pid):
        self._current = pid
        if self.sidebar.search.get_text():
            self.sidebar.search.set_text("")
        self._show(pid)
        self.split.set_show_content(True)

    def open_page(self, pid):
        self.sidebar.search.set_text("")
        self._current = pid
        self.sidebar.select(pid)
        self._show(pid)
        self.split.set_show_content(True)

    def _on_search_changed(self, text):
        if self._search_timer:
            GLib.source_remove(self._search_timer)
            self._search_timer = 0
        if not text.strip():
            self.search_page.set_query("")
            self.sidebar.select(self._current)
            self._show(self._current)
            return
        self._search_timer = GLib.timeout_add(120, self._run_search, text)

    def _run_search(self, text):
        self._search_timer = 0
        self.search_page.set_query(text)
        self.sidebar.select(None)
        self._show("search")
        return GLib.SOURCE_REMOVE

    # -- state -----------------------------------------------------------

    def _all_rows(self):
        for sp in self.settings_pages:
            yield from sp.rows
        yield from self.search_page.rows

    def _on_cm_event(self, event, key=None):
        for row in self._all_rows():
            if key is None or row.key == key:
                row.refresh()
        for sp in self.settings_pages:
            if sp.preview is not None and sp.preview.handles(key):
                sp.preview.update()
        if event in ("backups", "backup_created", "restored", "backup_restored") or key is None:
            self.backups_page.refresh()
        for pid, field in (("keybinds", "binds"), ("rules", "window_rules"), ("monitors", None)):
            page = self._item_pages.get(pid)
            if page is None or not hasattr(page, "refresh"):
                continue
            if key is None and (pid != "monitors" or event != "changed") or (field and key == field):
                page.refresh()
        self._sync_state()

    def _sync_state(self):
        self._action("undo").set_enabled(bool(self.cm.can_undo))
        self._action("redo").set_enabled(bool(self.cm.can_redo))
        n = pending_count(self.cm)
        self._action("apply").set_enabled(n > 0)
        self._action("discard").set_enabled(n > 0)
        self.apply_bar.set_count(n)

    def discard(self):
        if not self.cm.is_dirty:
            return
        self.cm.discard()
        self._on_cm_event("discarded", None)
        self.toasts.add_toast(Adw.Toast(title="Changes discarded", timeout=2))

    def apply(self):
        if not self.cm.is_dirty:
            return
        try:
            res = self.cm.apply()
        except Exception as e:
            self._error_toast([str(e)])
            return
        self._on_cm_event("applied", None)
        if getattr(res, "ok", True) and not getattr(res, "errors", None):
            if getattr(res, "live_applied", True):
                title = "Changes applied"
            else:
                title = "Saved — takes effect next time Hyprland starts"
            self.toasts.add_toast(Adw.Toast(title=title, timeout=2))
        else:
            self._error_toast(list(getattr(res, "errors", []) or ["Unknown error"]))

    def _error_toast(self, errors):
        toast = Adw.Toast(title=f"Couldn't apply: {errors[0]}" if len(errors) == 1
                          else f"Couldn't apply {len(errors)} changes", timeout=6, button_label="Details")
        toast.set_priority(Adw.ToastPriority.HIGH)

        def details(*_):
            dlg = Adw.AlertDialog(heading="Some Changes Weren't Applied",
                                  body="\n".join(f"• {GLib.markup_escape_text(str(e))}" for e in errors),
                                  body_use_markup=True)
            dlg.add_response("ok", "OK")
            dlg.present(self)
        toast.connect("button-clicked", details)
        self.toasts.add_toast(toast)

    # -- dev helpers -------------------------------------------------------

    def _dev_scroll_to(self, widget):
        scroller = widget.get_ancestor(Gtk.ScrolledWindow)
        if scroller is not None:
            ok, pt = widget.compute_point(scroller.get_child(), Graphene.Point().init(0, 0))
            if ok:
                adj = scroller.get_vadjustment()
                adj.set_value(min(pt.y - 160, adj.get_upper() - adj.get_page_size()))
        return GLib.SOURCE_REMOVE

    def dev_prepare(self, page=None, search=None, sets=(), advanced=False, dialog=False, seed=False):
        if page:
            self.open_page(page)
        target = self._item_pages.get(page)
        if page in self._lazy:
            GLib.idle_add(lambda: self.sidebar.scroll_to_end() and False)
        if seed and hasattr(target, "dev_seed"):
            target.dev_seed()
        if page == "bar" and dialog == "autostart" and hasattr(target, "autostart_row"):
            target.autostart_row.set_active(True)
            GLib.timeout_add(400, self._dev_scroll_to, target.autostart_row)
        elif dialog and hasattr(target, "dev_open"):
            GLib.idle_add(lambda: target.dev_open(dialog) and False)
        for k, v in sets:
            self.cm.set(k, v)
        if advanced:
            for sp in self.settings_pages:
                if sp.page_id == self._current and getattr(sp, "advanced_switch", None):
                    sp.advanced_switch.set_active(True)
        if search:
            self.sidebar.search.set_text(search)
            self._run_search(search)
