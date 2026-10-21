"""Sidebar: title, search and page list."""

from gi.repository import Adw, Gdk, Gtk

FIXED_PAGES = [
    ("presets", "Presets", "starred-symbolic"),
    ("bar", "Bar", "view-continuous-symbolic"),
    ("keybinds", "Keyboard Shortcuts", "preferences-desktop-keyboard-symbolic"),
    ("rules", "Window Rules", "focus-windows-symbolic"),
    ("monitors", "Displays", "view-dual-symbolic"),
    ("transfer", "Import & Export", "document-send-symbolic"),
    ("backups", "Backups", "document-open-recent-symbolic"),
]

# Extra words global search matches for the fixed pages: (id, title, icon, description, keywords)
_KEYWORDS = {
    "presets": ("Ready-made looks and your saved presets", "theme look style gallery"),
    "bar": ("Design and run your top bar", "waybar panel status clock tray widgets modules"),
    "keybinds": ("Add shortcuts that launch apps, switch workspaces and more",
                 "keybind keybinding bind hotkey key shortcut keyboard launch"),
    "rules": ("Make apps open floating, sized or on a workspace",
              "window rule windowrule float size workspace opacity app class"),
    "monitors": ("Arrange screens, resolution, refresh rate and scale",
                 "display monitor screen resolution refresh rate hz scale scaling rotation arrange"),
    "transfer": ("Move your setup to another computer", "import export share file"),
    "backups": ("Snapshots of your configuration", "backup restore snapshot undo"),
}
PAGE_INFO = [(pid, title, icon, *_KEYWORDS.get(pid, ("", ""))) for pid, title, icon in FIXED_PAGES]


def _icon(name):
    theme = Gtk.IconTheme.get_for_display(Gdk.Display.get_default())
    return name if name and theme.has_icon(name) else "preferences-other-symbolic"


class Sidebar(Adw.NavigationPage):
    def __init__(self, pages, on_select, on_search, menu_model):
        super().__init__(title="Hyprland Mod", tag="sidebar")
        self._on_select = on_select
        self._on_search = on_search
        self._ids = []
        self._silent = False

        header = Adw.HeaderBar()
        header.set_title_widget(Adw.WindowTitle(title="Hyprland Mod"))
        menu = Gtk.MenuButton(icon_name="open-menu-symbolic", menu_model=menu_model,
                              tooltip_text="Main Menu", primary=True)
        header.pack_end(menu)

        self.search = Gtk.SearchEntry(placeholder_text="Search settings", hexpand=True)
        self.search.add_css_class("sidebar-search")
        self.search.connect("search-changed", lambda e: self._on_search(e.get_text()))
        self.search.connect("stop-search", lambda e: e.set_text(""))
        search_box = Gtk.Box(margin_start=12, margin_end=12, margin_bottom=6)
        search_box.append(self.search)

        self.list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        self.list.add_css_class("navigation-sidebar")
        for p in pages:
            self._add_row(p["id"], p.get("title", p["id"]), _icon(p.get("icon")))
        self._first_fixed = len(self._ids)
        for pid, title, icon in FIXED_PAGES:
            self._add_row(pid, title, icon)
        self.list.set_header_func(self._header)
        self.list.connect("row-selected", self._selected)

        self.scroller = scroller = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER, vexpand=True, child=self.list)
        tv = Adw.ToolbarView()
        tv.add_top_bar(header)
        tv.add_top_bar(search_box)
        tv.set_content(scroller)
        self.set_child(tv)

    def _add_row(self, pid, title, icon):
        box = Gtk.Box(spacing=12)
        box.append(Gtk.Image(icon_name=icon))
        box.append(Gtk.Label(label=title, xalign=0, hexpand=True, ellipsize=3))
        row = Gtk.ListBoxRow(child=box)
        row.add_css_class("sidebar-row")
        self.list.append(row)
        self._ids.append(pid)

    def _header(self, row, _before):
        if row.get_index() == self._first_fixed and self._first_fixed > 0:
            sep = Gtk.Separator()
            sep.add_css_class("sidebar-separator")
            row.set_header(sep)
        else:
            row.set_header(None)

    def _selected(self, _lb, row):
        if row is None or self._silent:
            return
        self._on_select(self._ids[row.get_index()])

    def select(self, pid, notify=False):
        self._silent = not notify
        try:
            if pid in self._ids:
                self.list.select_row(self.list.get_row_at_index(self._ids.index(pid)))
            else:
                self.list.unselect_all()
        finally:
            self._silent = False

    def scroll_to_end(self):
        adj = self.scroller.get_vadjustment()
        adj.set_value(adj.get_upper() - adj.get_page_size())
