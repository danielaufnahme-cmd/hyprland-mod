"""Search results across every schema page."""

from gi.repository import Adw, GLib, Gtk

from .rows import build_row
from .settings_page import bottom_spacer

MAX_RESULTS = 60


def _haystack(page, group, opt):
    return " ".join(str(x) for x in (
        opt.get("label", ""), opt.get("description", ""), opt.get("key", ""),
        group.get("title", ""), page.get("title", ""),
        " ".join(str(c.get("label", "")) for c in opt.get("choices", []) or []),
    )).lower()


class SearchPage(Adw.Bin):
    def __init__(self, cm, pages, on_open_page):
        super().__init__()
        self.cm = cm
        self.pages = pages
        self.on_open_page = on_open_page
        self.rows = []
        self._stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=150)
        self._empty = Adw.StatusPage(icon_name="system-search-symbolic", title="No Results Found",
                                     description="Try a different search term")
        self._empty.add_css_class("compact")
        self._stack.add_named(self._empty, "empty")
        self._results_holder = Adw.Bin()
        self._stack.add_named(self._results_holder, "results")
        self.set_child(self._stack)

    def set_query(self, query):
        tokens = query.lower().split()
        self.rows = []
        prefs = Adw.PreferencesPage()
        count = 0
        for page in self.pages:
            matches = []
            for g in page.get("groups", []):
                for opt in g.get("options", []):
                    hay = _haystack(page, g, opt)
                    if all(t in hay for t in tokens):
                        matches.append(opt)
            if not matches or count >= MAX_RESULTS:
                continue
            group = Adw.PreferencesGroup(title=GLib.markup_escape_text(page.get("title", page["id"])))
            open_btn = Gtk.Button(label="Open Page", valign=Gtk.Align.CENTER)
            open_btn.add_css_class("flat")
            open_btn.add_css_class("open-page-button")
            open_btn.connect("clicked", lambda _b, pid=page["id"]: self.on_open_page(pid))
            group.set_header_suffix(open_btn)
            for opt in matches[:MAX_RESULTS - count]:
                row = build_row(self.cm, opt)
                row.add_css_class("search-result")
                group.add(row.row)
                self.rows.append(row)
                count += 1
            prefs.add(group)
        if count:
            prefs.add(bottom_spacer())
            prefs.set_description(f"{count} matching setting{'s' if count != 1 else ''}")
            self._results_holder.set_child(prefs)
            self._stack.set_visible_child_name("results")
        else:
            self._results_holder.set_child(None)
            self._empty.set_description(f"Nothing matches “{GLib.markup_escape_text(query)}”")
            self._stack.set_visible_child_name("empty")
