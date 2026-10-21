"""Adw.Application, stylesheet loading and the offscreen screenshot dev mode."""

import os
import sys
from pathlib import Path

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
gi.require_version("Gsk", "4.0")
gi.require_version("Graphene", "1.0")
from gi.repository import Adw, Gdk, Gio, GLib, Graphene, Gsk, Gtk  # noqa: E402

APP_ID = "io.github.HyprlandMod"
PROJECT_ROOT = Path(__file__).resolve().parent.parent.parent


class DevOptions:
    def __init__(self):
        self.screenshot = None
        self.page = None
        self.dark = False
        self.search = None
        self.sets = []
        self.advanced = False
        self.size = None
        self.scale = 1.0


def parse_dev_args(argv):
    """Strip hidden dev flags from argv; returns (DevOptions, remaining argv)."""
    dev, rest, it = DevOptions(), [argv[0]], iter(argv[1:])
    for a in it:
        if a == "--screenshot":
            dev.screenshot = next(it)
        elif a == "--page":
            dev.page = next(it)
        elif a == "--dark":
            dev.dark = True
        elif a == "--search":
            dev.search = next(it)
        elif a == "--set":
            k, _, v = next(it).partition("=")
            dev.sets.append((k, v))
        elif a == "--advanced":
            dev.advanced = True
        elif a == "--size":
            w, _, h = next(it).partition("x")
            dev.size = (int(w), int(h))
        elif a == "--scale":
            dev.scale = float(next(it))
        else:
            rest.append(a)
    return dev, rest


class HyprlandModApp(Adw.Application):
    def __init__(self, dev):
        flags = Gio.ApplicationFlags.DEFAULT_FLAGS
        if dev.screenshot:
            flags |= Gio.ApplicationFlags.NON_UNIQUE
        super().__init__(application_id=APP_ID, flags=flags)
        self.dev = dev
        self.window = None
        about = Gio.SimpleAction.new("about", None)
        about.connect("activate", self._about)
        self.add_action(about)
        quit_ = Gio.SimpleAction.new("quit", None)
        quit_.connect("activate", lambda *_: self.quit())
        self.add_action(quit_)
        self.set_accels_for_action("app.quit", ["<Control>q"])

    def do_startup(self):
        Adw.Application.do_startup(self)
        sm = Adw.StyleManager.get_default()
        if self.dev.screenshot:
            # broadway without a connected browser never ticks the frame clock, so
            # transitions would freeze mid-way: render the settled state instead
            Gtk.Settings.get_default().set_property("gtk-enable-animations", False)
            sm.set_color_scheme(Adw.ColorScheme.FORCE_DARK if self.dev.dark else Adw.ColorScheme.FORCE_LIGHT)
        else:
            sm.set_color_scheme(Adw.ColorScheme.DEFAULT)
        css = PROJECT_ROOT / "styles" / "style.css"
        if css.exists():
            provider = Gtk.CssProvider()
            provider.load_from_path(str(css))
            Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider,
                                                      Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)

    def do_activate(self):
        if self.window:
            self.window.present()
            return
        from .window import MainWindow
        cm = schema = error = None
        try:
            from .backend import ConfigManager, load_schema
            schema = load_schema()
            cm = ConfigManager()
        except Exception as e:  # show a friendly page instead of a traceback window
            import traceback
            traceback.print_exc()
            error = e
        self.window = MainWindow(self, cm, schema, error)
        if self.dev.size:
            self.window.set_default_size(*self.dev.size)
        self.window.present()
        if self.dev.screenshot:
            if self.window.ready:
                self.window.dev_prepare(self.dev.page, self.dev.search, self.dev.sets, self.dev.advanced)
            GLib.timeout_add(1400, self._start_screenshot)

    def _about(self, *_):
        about = Adw.AboutDialog(application_name="Hyprland Mod", application_icon="preferences-desktop-appearance-symbolic",
                                developer_name="Hyprland Mod contributors", version="0.1.0",
                                comments="Friendly settings for Hyprland",
                                license_type=Gtk.License.GPL_3_0)
        about.present(self.window)

    def _start_screenshot(self):
        GLib.timeout_add(200, self._screenshot)
        return GLib.SOURCE_REMOVE

    def _screenshot(self):
        win = self.window
        if win.ready:
            win.apply_bar.settle()
        target = win.get_content()
        w, h = target.get_width(), target.get_height()
        if not win.get_mapped() or w <= 0:
            print("screenshot: window not mapped", file=sys.stderr)
            self.quit()
            return GLib.SOURCE_REMOVE
        inner = Gtk.Snapshot()
        target.get_parent().snapshot_child(target, inner)
        content_node = inner.to_node()
        if content_node is None:
            self._shot_tries = getattr(self, "_shot_tries", 0) + 1
            if self._shot_tries < 25:
                target.queue_draw()
                return GLib.SOURCE_CONTINUE
            print("screenshot: nothing rendered", file=sys.stderr)
            self.quit()
            return GLib.SOURCE_REMOVE
        snap = Gtk.Snapshot()
        snap.scale(self.dev.scale, self.dev.scale)
        # the window background lives on the toplevel, which we don't snapshot
        bg = Gdk.RGBA()
        bg.parse("#222226" if Adw.StyleManager.get_default().get_dark() else "#fafafb")
        snap.append_color(bg, Graphene.Rect().init(0, 0, w, h))
        snap.append_node(content_node)
        node = snap.to_node()
        renderer = Gsk.CairoRenderer()
        renderer.realize_for_display(Gdk.Display.get_default())
        rect = Graphene.Rect().init(0, 0, w * self.dev.scale, h * self.dev.scale)
        tex = renderer.render_texture(node, rect)
        renderer.unrealize()
        out = os.path.abspath(self.dev.screenshot)
        os.makedirs(os.path.dirname(out), exist_ok=True)
        tex.save_to_png(out)
        print(f"screenshot: {out} ({w}x{h})", file=sys.stderr)
        self.quit()
        return GLib.SOURCE_REMOVE


def main(argv=None):
    argv = list(sys.argv if argv is None else argv)
    dev, rest = parse_dev_args(argv)
    if dev.screenshot and os.environ.get("WAYLAND_DISPLAY"):
        print("--screenshot refuses to run on a real Wayland session; use GDK_BACKEND=broadway", file=sys.stderr)
        return 2
    app = HyprlandModApp(dev)
    return app.run(rest)
