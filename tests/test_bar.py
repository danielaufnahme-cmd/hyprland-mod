"""Bar builder: model, Waybar config/CSS generation, sharing and process control.

Nothing here starts waybar or writes to the real ~/.config/waybar: every path is
a temp dir and processes are fakes. The GTK smoke test only runs offscreen
(GDK_BACKEND=broadway) and is skipped otherwise.
"""

import json
import os
import re
import shutil
import signal
import tempfile
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path
from unittest import mock

from hyprland_mod import bar
from hyprland_mod.bar import modules, process

REAL_WAYBAR = os.path.realpath(os.path.expanduser("~/.config/waybar"))


class BarTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="hyprland-mod-bar-test-"))
        self.waybar = self.tmp / "waybar"
        self.data = self.tmp / "data"
        env = mock.patch.dict(os.environ, {
            "HYPRLAND_MOD_DRY_RUN": "1",
            "HYPRLAND_MOD_WAYBAR_DIR": str(self.waybar),
            "HYPRLAND_MOD_DATA": str(self.data),
            "XDG_CONFIG_HOME": str(self.tmp / "xdg"),
        })
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)

    def parse_config(self, text):
        return json.loads(bar.strip_jsonc(text))

    def assert_css_sane(self, css):
        self.assertEqual(css.count("{"), css.count("}"))
        # each declaration is "name: value;" with no stray braces/semicolons in values
        for block in re.findall(r"\{([^{}]*)\}", css):
            for decl in filter(None, (d.strip() for d in block.split(";"))):
                self.assertRegex(decl, r"^[a-z-]+: [^;{}]+$", decl)
        self.assertNotIn("@import", css)
        self.assertNotIn("url(", css)


# ---------------------------------------------------------------- presets

class PresetsTest(BarTestCase):
    def test_ten_distinct_presets(self):
        presets = bar.load_presets()
        self.assertEqual(len(presets), 10)
        self.assertEqual(len({p.id for p in presets}), 10)
        self.assertEqual(len({p.name for p in presets}), 10)
        self.assertEqual(len({json.dumps(p.bar.to_dict(), sort_keys=True) for p in presets}), 10)
        for p in presets:
            self.assertTrue(p.name and p.description, p.id)

    def test_every_preset_generates_valid_files(self):
        for p in bar.load_presets():
            with self.subTest(p.id):
                cfg = self.parse_config(bar.generate_config(p.bar))
                self.assertIn(cfg["position"], ("top", "bottom"))
                self.assertEqual(cfg["height"], p.bar.height)
                for zone in bar.ZONES:
                    names = cfg[f"modules-{zone}"]
                    self.assertEqual(names, [modules.MODULES[e["type"]]["waybar"] for e in p.bar.modules[zone]])
                    for name in names:
                        self.assertIsInstance(cfg[name], dict, name)
                css = bar.generate_style(p.bar)
                self.assert_css_sane(css)
                self.assertIn(f'"{p.bar.font}"', css)
                ET.fromstring(bar.generate_menu(p.bar))

    def test_presets_cover_the_promised_looks(self):
        by_id = {p.id: p.bar for p in bar.load_presets()}
        self.assertEqual(by_id["dock-bottom"].position, "bottom")
        self.assertEqual(by_id["islands"].style, "islands")
        self.assertTrue(by_id["floating-pill"].floating)
        self.assertFalse(by_id["monochrome"].icons)
        self.assertEqual(len({b.style for b in by_id.values()}), 3)


# ---------------------------------------------------------------- generation

class GenerateTest(BarTestCase):
    def every_module_bar(self, icons=True):
        b = bar.Bar(modules={"left": [], "center": [], "right": []}, icons=icons)
        for i, mtype in enumerate(modules.MODULES):
            b.add_module(mtype, bar.ZONES[i % 3])
        return b

    def test_all_modules_valid_json_with_and_without_icons(self):
        for icons in (True, False):
            with self.subTest(icons=icons):
                b = self.every_module_bar(icons)
                cfg = self.parse_config(bar.generate_config(b))
                self.assertEqual(len(cfg["modules-left"]) + len(cfg["modules-center"])
                                 + len(cfg["modules-right"]), len(modules.MODULES))
                for spec in modules.MODULES.values():
                    self.assertIn(spec["waybar"], cfg)
                for name in ("hyprland/workspaces", "hyprland/window", "clock", "battery", "network",
                             "pulseaudio", "wireplumber", "cpu", "memory", "temperature", "tray", "backlight",
                             "bluetooth", "custom/power", "idle_inhibitor", "mpris", "keyboard-state",
                             "hyprland/language"):
                    self.assertIn(name, cfg)
                self.assert_css_sane(bar.generate_style(b))
                for mtype in modules.MODULES:
                    self.assertTrue(bar.sample_label(mtype, b.options(mtype), icons))

    def test_options_reach_the_config(self):
        b = self.every_module_bar()
        b.set_option("clock", "format", "12h")
        b.set_option("clock", "seconds", True)
        b.set_option("battery", "warning", 40)
        b.set_option("battery", "critical", 10)
        b.set_option("workspaces", "style", "roman")
        b.set_option("workspaces", "persistent", 3)
        b.set_option("power", "suspend", False)
        cfg = self.parse_config(bar.generate_config(b))
        self.assertEqual(cfg["clock"]["format"], "{:%I:%M:%S %p}")
        self.assertEqual(cfg["clock"]["interval"], 1)
        self.assertEqual(cfg["battery"]["states"], {"warning": 40, "critical": 10})
        self.assertEqual(cfg["hyprland/workspaces"]["format-icons"]["4"], "IV")
        self.assertEqual(cfg["hyprland/workspaces"]["persistent-workspaces"], {"*": 3})
        actions = cfg["custom/power"]["menu-actions"]
        self.assertNotIn("suspend", actions)
        self.assertIn("shutdown", actions)
        self.assertTrue(cfg["custom/power"]["menu-file"].startswith(str(self.waybar)))
        menu = ET.fromstring(bar.generate_menu(b))
        ids = [o.get("id") for o in menu.iter("object") if o.get("class") == "GtkMenuItem"]
        self.assertEqual(set(ids), set(actions))

    def test_floating_margins_and_position(self):
        b = bar.Bar(floating=True, margin=12, position="bottom")
        cfg = self.parse_config(bar.generate_config(b))
        self.assertEqual((cfg["margin-top"], cfg["margin-bottom"], cfg["margin-left"]), (0, 12, 12))
        b.set("floating", False)
        cfg = self.parse_config(bar.generate_config(b))
        self.assertEqual((cfg["margin-bottom"], cfg["margin-left"], cfg["margin-right"]), (0, 0, 0))
        self.assertIn("border-radius: 0px", bar.generate_style(b))

    def test_styles_change_the_css(self):
        b = bar.Bar(opacity=0.5, background="#102030")
        solid = bar.generate_style(b)
        self.assertIn("rgba(16, 32, 48, 0.50)", solid)
        b.set("style", "islands")
        islands = bar.generate_style(b)
        self.assertRegex(islands, r"window#waybar \{[^}]*background-color: transparent")
        self.assertIn(".modules-left,\n.modules-center,\n.modules-right {", islands)
        b.remove_module("clock")
        self.assertNotIn(".modules-center,", bar.generate_style(b))
        b.set("style", "pills")
        pills = bar.generate_style(b)
        self.assertIn("#battery,", pills)

    def test_strip_jsonc_respects_strings(self):
        text = '// head\n{"a": "http://x//y", /* c */ "b": "/* not */"} // tail\n'
        self.assertEqual(json.loads(bar.strip_jsonc(text)), {"a": "http://x//y", "b": "/* not */"})

    def test_write_files_only_in_own_folder(self):
        b = bar.load_presets()[0].bar
        paths = bar.write_files(b)
        self.assertEqual(sorted(p.name for p in self.waybar.iterdir()),
                         ["config.jsonc", "power-menu.xml", "style.css"])
        for p in paths.values():
            self.assertTrue(str(p).startswith(str(self.waybar)))
            self.assertFalse(os.path.realpath(p).startswith(REAL_WAYBAR + os.sep))
        self.assertTrue(bar.files_match(b))
        b.set("height", b.height + 2)
        self.assertFalse(bar.files_match(b))

    def test_default_folder_is_a_subfolder(self):
        with mock.patch.dict(os.environ, {"HYPRLAND_MOD_WAYBAR_DIR": ""}):
            self.assertEqual(bar.waybar_dir(), self.tmp / "xdg" / "waybar" / "hyprland-mod")


# ---------------------------------------------------------------- safety

class InjectionTest(BarTestCase):
    BAD_FONTS = ['Inter"; } * { color: red; } x { a: "', "Inter; color: red", "Inter}", "a\\62 c",
                 "Inter\nwindow", "", " ", "x" * 80, "Inter/*", "url(x)", "<b>Inter</b>"]
    BAD_COLORS = ["red", "#fff", "#12345g", "#123456; color: red", "rgba(0,0,0,1)", "#1234567", "", None, 5]

    def test_bad_fonts_rejected(self):
        b = bar.Bar()
        for font in self.BAD_FONTS:
            with self.subTest(font=font), self.assertRaises(bar.BarError):
                b.set("font", font)
        b.set("font", "JetBrainsMono Nerd Font")
        b.set("font", "Noto Sans CJK-JP 2.0")

    def test_bad_colors_rejected(self):
        b = bar.Bar()
        for field in ("background", "foreground", "accent"):
            for value in self.BAD_COLORS:
                with self.subTest(field=field, value=value), self.assertRaises(bar.BarError):
                    b.set(field, value)
        b.set("accent", "#ABCDEF")
        self.assertEqual(b.accent, "#abcdef")

    def test_generator_revalidates_tampered_objects(self):
        b = bar.Bar()
        b.font = 'x"; } window { background: url(evil) } y { z: "'
        with self.assertRaises(bar.BarError):
            bar.generate_style(b)
        b = bar.Bar()
        b.accent = "#fff; }"
        with self.assertRaises(bar.BarError):
            bar.generate_style(b)
        b = bar.Bar()
        b.height = "30; }"
        with self.assertRaises(bar.BarError):
            bar.generate_config(b)

    def test_numbers_and_choices_validated(self):
        b = bar.Bar()
        for field, value in (("height", 5), ("height", 500), ("height", 3.5), ("height", True),
                             ("opacity", 1.5), ("opacity", "1"), ("position", "left"), ("style", "x"),
                             ("floating", "yes"), ("font_weight", "900"), ("nope", 1)):
            with self.subTest(field=field, value=value), self.assertRaises(bar.BarError):
                b.set(field, value)

    def test_module_options_validated(self):
        b = bar.Bar()
        cases = [("clock", "format", "%H; rm -rf"), ("clock", "nope", 1), ("battery", "warning", 99),
                 ("battery", "critical", 35), ("workspaces", "persistent", "5"), ("tray", "icon_size", 2.5)]
        b.add_module("workspaces", "left") if not b.find("workspaces") else None
        for mtype, key, value in cases:
            with self.subTest(key=key, value=value), self.assertRaises(bar.BarError):
                b.set_option(mtype, key, value)
        before = b.to_dict()
        with self.assertRaises(bar.BarError):
            b.set_option("battery", "critical", 40)
        self.assertEqual(b.to_dict(), before)

    def test_user_strings_never_reach_files_raw(self):
        doc = bar.document(bar.Bar(), 'Evil "name" </script>', "desc")
        self.assertEqual(doc["id"], "evil-name-script")
        css = bar.generate_style(bar.validate_document(doc).bar)
        self.assertNotIn("Evil", css)


# ---------------------------------------------------------------- editing

class EditTest(BarTestCase):
    def fresh(self):
        return bar.validate_bar({"modules": {"left": ["workspaces", "window"], "center": ["clock"],
                                             "right": ["pulseaudio", "battery", "tray"]}})

    def layout(self, b):
        return {z: [m["type"] for m in b.modules[z]] for z in bar.ZONES}

    def test_add_remove(self):
        b = self.fresh()
        b.add_module("cpu", "right", 0)
        self.assertEqual(self.layout(b)["right"], ["cpu", "pulseaudio", "battery", "tray"])
        b.add_module("mpris", "center")
        self.assertEqual(self.layout(b)["center"], ["clock", "mpris"])
        with self.assertRaises(bar.BarError):
            b.add_module("cpu", "left")
        with self.assertRaises(bar.BarError):
            b.add_module("nope", "left")
        with self.assertRaises(bar.BarError):
            b.add_module("memory", "middle")
        removed = b.remove_module("battery")
        self.assertEqual(removed["type"], "battery")
        self.assertIsNone(b.find("battery"))
        with self.assertRaises(bar.BarError):
            b.remove_module("battery")

    def test_reorder_and_move(self):
        b = self.fresh()
        self.assertTrue(b.shift_module("tray", -1))
        self.assertEqual(self.layout(b)["right"], ["pulseaudio", "tray", "battery"])
        self.assertFalse(b.shift_module("pulseaudio", -1))
        self.assertFalse(b.shift_module("battery", 1))
        b.move_module("clock", "left", 1)
        self.assertEqual(self.layout(b), {"left": ["workspaces", "clock", "window"], "center": [],
                                          "right": ["pulseaudio", "tray", "battery"]})
        b.move_module("workspaces", "left", 99)
        self.assertEqual(self.layout(b)["left"], ["clock", "window", "workspaces"])
        b.move_module("battery", "center", 0)
        self.assertEqual(self.layout(b)["center"], ["battery"])
        cfg = self.parse_config(bar.generate_config(b))
        self.assertEqual(cfg["modules-center"], ["battery"])
        self.assertEqual(cfg["modules-left"], ["clock", "hyprland/window", "hyprland/workspaces"])

    def test_options_survive_moves(self):
        b = self.fresh()
        b.set_option("clock", "format", "full")
        b.move_module("clock", "right", 0)
        self.assertEqual(b.options("clock")["format"], "full")

    def test_copy_is_independent(self):
        b = self.fresh()
        c = b.copy()
        c.add_module("cpu", "left")
        c.set_option("clock", "seconds", True)
        self.assertIsNone(b.find("cpu"))
        self.assertFalse(b.options("clock")["seconds"])
        self.assertNotEqual(b, c)

    def test_duplicate_and_damaged_layouts_rejected(self):
        for data in ({"modules": {"left": ["clock"], "right": ["clock"]}},
                     {"modules": {"top": ["clock"]}},
                     {"modules": {"left": [{"type": "clock", "exec": "rm"}]}},
                     {"modules": {"left": "clock"}},
                     {"modules": {"left": [{"type": "custom/evil"}]}},
                     {"height": 30, "exec-once": "x"},
                     "not a dict"):
            with self.subTest(data=data), self.assertRaises(bar.BarError):
                bar.validate_bar(data)


# ---------------------------------------------------------------- persistence & sharing

class PersistTest(BarTestCase):
    def test_save_load_round_trip(self):
        self.assertIsNone(bar.load_current())
        for p in bar.load_presets():
            with self.subTest(p.id):
                bar.save_current(p.bar)
                self.assertEqual(bar.load_current(), p.bar)
        self.assertTrue(str(bar.data_dir()).startswith(str(self.data)))

    def test_damaged_current_is_ignored(self):
        path = bar.save_current(bar.Bar())
        path.write_text("{ not json")
        self.assertIsNone(bar.load_current())
        path.write_text(json.dumps({"height": 9999}))
        self.assertIsNone(bar.load_current())

    def test_export_import(self):
        b = bar.load_presets()[2].bar.copy()
        b.set_option("clock", "format", "iso")
        out = self.tmp / f"mine{bar.BAR_EXT}"
        bar.export_bar(b, out, "My Islands", "shared with a friend")
        raw = json.loads(out.read_text())
        self.assertEqual((raw["format"], raw["version"], raw["id"]), (bar.BAR_FORMAT, 1, "my-islands"))
        preset = bar.import_bar(out)
        self.assertEqual((preset.name, preset.description), ("My Islands", "shared with a friend"))
        self.assertEqual(preset.bar, b)

    def test_import_rejects_bad_files(self):
        good = bar.document(bar.Bar(), "Ok")
        cases = {
            "wrong-format": {**good, "format": "hyprland-mod-preset"},
            "newer": {**good, "version": 99},
            "no-name": {**good, "name": "  "},
            "control-chars": {**good, "name": "a\x1bb"},
            "bad-bar": {**good, "bar": {**good["bar"], "accent": "javascript:"}},
            "bad-id": {**good, "id": "../../etc"},
        }
        for name, data in cases.items():
            path = self.tmp / f"{name}{bar.BAR_EXT}"
            path.write_text(json.dumps(data))
            with self.subTest(name), self.assertRaises(bar.BarError):
                bar.import_bar(path)
        (self.tmp / "junk.hyprmod-bar").write_text("nope")
        with self.assertRaises(bar.BarError):
            bar.import_bar(self.tmp / "junk.hyprmod-bar")
        big = self.tmp / "big.hyprmod-bar"
        big.write_text(" " * (300 * 1024))
        with self.assertRaises(bar.BarError):
            bar.import_bar(big)
        with self.assertRaises(bar.BarError):
            bar.import_bar(self.tmp / "missing.hyprmod-bar")


# ---------------------------------------------------------------- process control

class FakeProc:
    """A fake /proc plus launcher/killer that never touch real processes."""

    def __init__(self, root):
        self.root = Path(root)
        self.launched, self.killed = [], []
        self.next_pid = 4242

    def add(self, pid, comm, cmdline):
        d = self.root / str(pid)
        d.mkdir(parents=True, exist_ok=True)
        (d / "comm").write_text(comm + "\n")
        (d / "cmdline").write_bytes(b"\0".join(a.encode() for a in cmdline) + b"\0")

    def launcher(self, cmd, log_path):
        pid = self.next_pid
        self.next_pid += 1
        self.launched.append(cmd)
        self.add(pid, "waybar", cmd)
        return pid

    def killer(self, pid, sig):
        self.killed.append((pid, sig))
        shutil.rmtree(self.root / str(pid), ignore_errors=True)


class ProcessTest(BarTestCase):
    def make(self, installed=True):
        self.fake = FakeProc(self.tmp / "proc")
        (self.tmp / "proc").mkdir()
        return bar.BarProcess(self.data / "bar", launcher=self.fake.launcher, killer=self.fake.killer,
                              which=lambda name: "/usr/bin/waybar" if installed else None,
                              proc_root=self.tmp / "proc")

    def test_start_stop(self):
        p = self.make()
        self.assertFalse(p.is_running())
        pid = p.start()
        self.assertEqual(pid, 4242)
        cfg = str(self.waybar / "config.jsonc")
        self.assertEqual(self.fake.launched, [["/usr/bin/waybar", "-c", cfg, "-s", str(self.waybar / "style.css")]])
        self.assertEqual(p.pid_path.read_text().strip(), "4242")
        self.assertTrue(p.is_running())
        self.assertTrue(p.stop())
        self.assertEqual(self.fake.killed, [(4242, signal.SIGTERM)])
        self.assertFalse(p.pid_path.exists())
        self.assertFalse(p.stop())

    def test_restart_replaces_own_bar_only(self):
        p = self.make()
        self.fake.add(100, "waybar", ["waybar"])  # the user's own waybar
        p.start()
        p.start()
        self.assertEqual(self.fake.killed, [(4242, signal.SIGTERM)])
        self.assertEqual(p.running_pid(), 4243)

    def test_never_kills_a_reused_pid(self):
        p = self.make()
        self.fake.add(555, "firefox", ["firefox"])
        p.state_dir.mkdir(parents=True)
        p.pid_path.write_text("555\n")
        self.assertIsNone(p.running_pid())
        self.assertFalse(p.stop())
        self.assertEqual(self.fake.killed, [])
        self.assertFalse(p.pid_path.exists())
        p.pid_path.write_text("garbage")
        self.assertFalse(p.is_running())

    def test_missing_waybar(self):
        p = self.make(installed=False)
        self.assertFalse(p.installed())
        with self.assertRaises(bar.WaybarMissing):
            p.start()
        self.assertEqual(self.fake.launched, [])

    def test_dry_run_never_launches_or_kills_for_real(self):
        with mock.patch("subprocess.Popen") as popen, mock.patch("os.kill") as kill:
            p = bar.BarProcess(self.data / "bar", which=lambda n: "/usr/bin/waybar", proc_root=self.tmp / "proc")
            self.assertIsNone(p.start())
            fake = FakeProc(self.tmp / "proc")
            fake.add(777, "waybar", ["waybar", "-c", str(self.waybar / "config.jsonc")])
            p.state_dir.mkdir(parents=True, exist_ok=True)
            p.pid_path.write_text("777")
            self.assertFalse(p.stop())
            popen.assert_not_called()
            kill.assert_not_called()

    def test_other_bars(self):
        p = self.make()
        f = self.fake
        f.add(10, "waybar", ["waybar"])
        f.add(11, "qs", ["qs", "-c", "caelestia", "-n", "-d"])
        f.add(12, "qs", ["qs", "-c", "jarvis", "-n", "-d"])  # a quickshell config that isn't a bar
        f.add(13, "eww", ["eww", "daemon"])
        f.add(14, "bash", ["bash"])
        f.add(15, "quickshell", ["quickshell"])
        p.start()
        self.assertEqual(sorted(p.other_bars()), ["Caelestia (Quickshell)", "Eww", "Quickshell", "Waybar"])
        self.assertEqual(process.other_bars(self.tmp / "proc", ignore={10, 11, 13, 15}), ["Waybar"])
        self.assertEqual(process.other_bars(self.tmp / "nowhere"), [])

    def test_autostart_command(self):
        self.assertEqual(bar.autostart_command(),
                         f"waybar -c {self.waybar}/config.jsonc -s {self.waybar}/style.css")
        spaced = self.tmp / "my bars"
        self.assertEqual(bar.autostart_command(spaced),
                         f"waybar -c '{spaced}/config.jsonc' -s '{spaced}/style.css'")

    def test_install_hints(self):
        text = " ".join(f"{d} {c}" for d, c in bar.INSTALL_HINTS)
        for word in ("Arch", "Fedora", "Debian", "openSUSE", "pacman", "dnf", "apt", "zypper"):
            self.assertIn(word, text)


# ---------------------------------------------------------------- UI smoke (offscreen only)

@unittest.skipUnless(os.environ.get("GDK_BACKEND") == "broadway" and not os.environ.get("WAYLAND_DISPLAY"),
                     "GTK smoke test runs only offscreen (GDK_BACKEND=broadway)")
class PageSmokeTest(BarTestCase):
    def test_page_edits(self):
        import gi
        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw
        Adw.init()
        from hyprland_mod.ui.bar_page import BarPage
        overlay = Adw.ToastOverlay()
        page = BarPage(None, overlay)
        page.add("cpu", "left")
        self.assertEqual(page.selected, "cpu")
        page.move("cpu", "right", 0)
        page.shift("cpu", 1)
        page.set_option("cpu", "interval", 9)
        page._set("height", 40)
        page._set("font", 'bad"font')  # rejected with a toast, not applied
        self.assertNotEqual(page.bar.font, 'bad"font')
        page.remove("cpu")
        # drag and drop: drop the clock at the top of the right column, and back
        from gi.repository import Gtk
        self.assertTrue(page.columns["right"]._on_drop(None, "clock", 0, -1))
        self.assertEqual(page.bar.find("clock"), ("right", 0))
        self.assertFalse(page.columns["left"]._on_drop(None, "custom/evil", 0, 0))
        self.assertTrue(page.columns["center"]._on_drop(None, "clock", 0, -1))
        self.assertEqual(page.bar.find("clock"), ("center", 0))
        pop = Gtk.Popover()
        page.columns["left"]._fill_picker(pop)
        self.assertIsNotNone(pop.get_child())
        page.use_preset(page.presets[3])
        self.assertEqual(page.bar, page.presets[3].bar)
        page.use_bar()  # dry run: files only
        self.assertTrue((self.waybar / "config.jsonc").exists())
        page._save_now()
        self.assertEqual(bar.load_current(), page.bar)
        calls = []
        page.set_autostart_handler(lambda: False, lambda on, cmd: calls.append((on, cmd)))
        page.autostart_row.set_active(True)
        self.assertEqual(calls, [(True, bar.autostart_command())])
        out = self.tmp / f"x{bar.BAR_EXT}"
        bar.export_bar(page.bar, out, "X")
        page.use_preset(page.presets[0])
        self.assertIsNotNone(page.import_file(out))
        self.assertEqual(page.bar, page.presets[3].bar)


if __name__ == "__main__":
    unittest.main()
