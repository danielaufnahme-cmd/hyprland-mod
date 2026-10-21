"""Dev only: render the bar builder page offscreen to a PNG (never on a real screen).

    env -u WAYLAND_DISPLAY -u DISPLAY GDK_BACKEND=broadway BROADWAY_DISPLAY=:15 \\
        HYPRLAND_MOD_DRY_RUN=1 HYPRLAND_MOD_DATA=<tmp> HYPRLAND_MOD_WAYBAR_DIR=<tmp> \\
        python3 -m hyprland_mod.bar.devshot out.png [--dark] [--select clock] [--scroll 0.5] [--size 1000x900]
"""

import os
import sys


def main(argv):
    if os.environ.get("WAYLAND_DISPLAY") or os.environ.get("GDK_BACKEND") != "broadway":
        print("devshot only runs offscreen: unset WAYLAND_DISPLAY and use GDK_BACKEND=broadway", file=sys.stderr)
        return 2
    import gi
    gi.require_version("Gtk", "4.0")
    gi.require_version("Adw", "1")
    gi.require_version("Gsk", "4.0")
    gi.require_version("Graphene", "1.0")
    from gi.repository import Adw, Gdk, GLib, Graphene, Gsk, Gtk

    from ..ui.app import PROJECT_ROOT
    out, dark, select, scroll, size, preset = argv[1], False, None, 0.0, (1000, 900), None
    it = iter(argv[2:])
    for a in it:
        if a == "--dark":
            dark = True
        elif a == "--select":
            select = next(it)
        elif a == "--scroll":
            scroll = float(next(it))
        elif a == "--preset":
            preset = next(it)
        elif a == "--size":
            w, _, h = next(it).partition("x")
            size = (int(w), int(h))

    app = Adw.Application(application_id="io.github.HyprlandMod.BarShot")
    state = {}

    def activate(_app):
        Gtk.Settings.get_default().set_property("gtk-enable-animations", False)
        Adw.StyleManager.get_default().set_color_scheme(
            Adw.ColorScheme.FORCE_DARK if dark else Adw.ColorScheme.FORCE_LIGHT)
        provider = Gtk.CssProvider()
        provider.load_from_path(str(PROJECT_ROOT / "styles" / "style.css"))
        Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider,
                                                  Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)
        from ..ui.bar_page import BarPage
        cm = None
        try:
            from ..ui.backend import ConfigManager
            cm = ConfigManager()
        except Exception as e:  # the page only needs cm for its data dir
            print(f"devshot: no ConfigManager ({e})", file=sys.stderr)
        win = Adw.ApplicationWindow(application=app, default_width=size[0], default_height=size[1])
        overlay = Adw.ToastOverlay()
        page = BarPage(cm, overlay)
        tv = Adw.ToolbarView(content=page)
        tv.add_top_bar(Adw.HeaderBar(title_widget=Adw.WindowTitle(title="Bar")))
        overlay.set_child(tv)
        win.set_content(overlay)
        win.present()
        state.update(win=win, page=page)
        if preset:
            page._restore(next(p for p in page.presets if p.id == preset).bar)  # no toast in shots
        if select:
            GLib.timeout_add(500, lambda: (page.dev_select(select), False)[1])
        GLib.timeout_add(900, do_scroll)
        GLib.timeout_add(1500, shoot)

    def do_scroll():
        page = state["page"]
        sw = page.get_child().get_last_child()
        adj = sw.get_vadjustment()
        adj.set_value((adj.get_upper() - adj.get_page_size()) * scroll)
        return False

    def shoot():
        win = state["win"]
        target = win.get_content()
        w, h = target.get_width(), target.get_height()
        # broadway without a browser never ticks the frame clock: lay out by hand
        target.allocate(w, h, -1, None)
        inner = Gtk.Snapshot()
        target.get_parent().snapshot_child(target, inner)
        node = inner.to_node()
        if node is None:
            state["tries"] = state.get("tries", 0) + 1
            if state["tries"] < 25:
                target.queue_draw()
                return True
            print("devshot: nothing rendered", file=sys.stderr)
            app.quit()
            return False
        snap = Gtk.Snapshot()
        bg = Gdk.RGBA()
        bg.parse("#222226" if dark else "#fafafb")
        snap.append_color(bg, Graphene.Rect().init(0, 0, w, h))
        snap.append_node(node)
        renderer = Gsk.CairoRenderer()
        renderer.realize_for_display(Gdk.Display.get_default())
        tex = renderer.render_texture(snap.to_node(), Graphene.Rect().init(0, 0, w, h))
        renderer.unrealize()
        path = os.path.abspath(out)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        tex.save_to_png(path)
        print(f"devshot: {path} ({w}x{h})", file=sys.stderr)
        app.quit()
        return False

    app.connect("activate", activate)
    return app.run([argv[0]])


if __name__ == "__main__":
    sys.exit(main(sys.argv))
