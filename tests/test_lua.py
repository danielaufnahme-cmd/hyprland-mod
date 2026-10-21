import json
import os
import shutil
import subprocess
import unittest

from _helpers import FIXTURES, FakeRunner, TempConfigTest
from hyprland_mod.core import lua
from hyprland_mod.core.managed import ManagedState

LUA = shutil.which("lua") or shutil.which("lua5.4") or shutil.which("luajit")
LUAC = shutil.which("luac")
STUBS = str(FIXTURES / "hl_stubs.lua")
TYPES = lua.load_lua_types(STUBS)


def run_fake_hl(path) -> list[str]:
    """Execute a Lua config against the fake `hl` table; returns its printed lines."""
    proc = subprocess.run([LUA, str(FIXTURES / "fake_hl.lua"), str(path)],
                          capture_output=True, text=True, timeout=10)
    if proc.returncode != 0:
        raise AssertionError(f"lua failed: {proc.stderr}")
    return proc.stdout.splitlines()


class LuaConfigTest(TempConfigTest):
    """Temp copy of the Lua fixtures: hyprland.lua + colors.lua (no hyprland.conf)."""

    def setUp(self):
        super().setUp()
        (self.hypr / "hyprland.conf").unlink()
        (self.hypr / "extra.conf").unlink()
        shutil.copy(FIXTURES / "sample_hyprland.lua", self.hypr / "hyprland.lua")
        shutil.copy(FIXTURES / "colors.lua", self.hypr / "colors.lua")
        self.config = self.hypr / "hyprland.lua"
        self.managed = self.hypr / "hyprland-mod.lua"
        self.original_text = self.config.read_text()
        os.environ["HYPRLAND_MOD_CONFIG"] = str(self.config)
        os.environ["HYPRLAND_MOD_LUA_STUBS"] = STUBS

    def require_lines(self):
        return [ln for ln in self.config.read_text().splitlines() if "hyprland-mod" in ln
                and not ln.startswith("--")]


class LiteralTest(unittest.TestCase):
    def lit(self, key, value):
        return lua.lua_literal(key, value, TYPES.get(lua.dotted(key)))

    def test_scalars(self):
        self.assertEqual(self.lit("decoration:blur:enabled", "yes"), "true")
        self.assertEqual(self.lit("decoration:blur:enabled", "0"), "false")
        self.assertEqual(self.lit("misc:unknown_flag", "off"), "false")
        self.assertEqual(self.lit("decoration:rounding", "8"), "8")
        self.assertEqual(self.lit("decoration:blur:vibrancy", ".5"), "0.5")
        self.assertEqual(self.lit("general:layout", "master"), '"master"')
        self.assertEqual(self.lit("input:kb_layout", "us,de"), '"us,de"')
        self.assertEqual(self.lit("input:kb_layout", "1"), '"1"')
        self.assertEqual(self.lit("misc:x", 'say "hi" \\ now'), '"say \\"hi\\" \\\\ now"')

    def test_colors_and_gradients(self):
        self.assertEqual(self.lit("decoration:shadow:color", "0xee1a1a1a"), '"rgba(1a1a1aee)"')
        self.assertEqual(self.lit("general:col.active_border",
                                  "rgba(33ccffee) rgba(00ff99ee) 45deg"),
                         '{ colors = { "rgba(33ccffee)", "rgba(00ff99ee)" }, angle = 45 }')
        self.assertEqual(self.lit("general:col.inactive_border", "rgb(595959)"), '"rgba(595959ff)"')
        self.assertEqual(self.lit("group:groupbar:text_color", "rgba(ffffffff)"), '"rgba(ffffffff)"')

    def test_vec2_and_gaps(self):
        self.assertEqual(self.lit("decoration:shadow:offset", "2 -3"), "{ 2, -3 }")
        self.assertEqual(self.lit("general:gaps_out", "5 10"),
                         "{ top = 5, right = 10, bottom = 5, left = 10 }")
        self.assertEqual(self.lit("general:gaps_out", "12"), "12")

    def test_without_stubs(self):
        self.assertEqual(lua.lua_literal("x:y", "true"), "true")
        self.assertEqual(lua.lua_literal("x:y", "1.25"), "1.25")
        self.assertEqual(lua.lua_literal("x:y", "hello"), '"hello"')
        self.assertEqual(lua.load_lua_types(str(FIXTURES / "nope.lua")), {})

    def test_keys(self):
        self.assertEqual(lua.lua_path("general:col.active_border"), ["general", "col", "active_border"])
        self.assertEqual(lua.lua_key("end"), '["end"]')
        self.assertEqual(lua.lua_key("my-plugin"), '["my-plugin"]')


class ListConversionTest(unittest.TestCase):
    def test_curve(self):
        self.assertEqual(lua.curve_call("snappy, 0.05, 0.9, 0.1, 1.05"),
                         'hl.curve("snappy", { type = "bezier", points = { {0.05, 0.9}, {0.1, 1.05} } })')
        for bad in ("x, 1, 2", "x, a, b, c, d", ", 1, 2, 3, 4"):
            with self.assertRaises(ValueError):
                lua.curve_call(bad)

    def test_animation(self):
        self.assertEqual(lua.animation_call("windows, 1, 4.79, easeOutQuint, popin 87%"),
                         'hl.animation({ leaf = "windows", enabled = true, speed = 4.79, '
                         'bezier = "easeOutQuint", style = "popin 87%" })')
        self.assertEqual(lua.animation_call("fade, 0"), 'hl.animation({ leaf = "fade", enabled = false })')
        self.assertIn('spring = "easy"', lua.animation_call("windows, 1, 4, spring:easy"))
        for bad in ("windows", "windows, maybe, 1", "windows, 1, fast"):
            with self.assertRaises(ValueError):
                lua.animation_call(bad)
        with self.assertRaises(ValueError):
            lua.list_calls("monitor", ["DP-1, preferred, auto, 1"])

    def test_scan_user_lists(self):
        files = lua.required_files(str(FIXTURES / "sample_hyprland.lua"), exclude=[])
        self.assertEqual([os.path.basename(f) for f in files], ["sample_hyprland.lua", "colors.lua"])
        lists = lua.scan_lists(files)
        self.assertEqual(lists["bezier"], ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1",
                                           "quick, 0.15, 0, 0.1, 1"])
        self.assertEqual(lists["animation"], [
            "global, 1, 10, default", "windows, 1, 4.79, spring:easy",
            "windowsIn, 1, 4.1, easeOutQuint, popin 87%", "fade, 0"])

    def test_describe_value(self):
        self.assertEqual(lua.describe_value("a.b", True), "true")
        self.assertEqual(lua.describe_value("decoration.shadow.color", 0xee1a1a1a), "rgba(1a1a1aee)")
        self.assertEqual(lua.describe_value("general.col.active_border",
                                            {"colors": ["rgba(33ccffee)", "rgba(00ff99ee)"], "angle": 45}),
                         "rgba(33ccffee) rgba(00ff99ee) 45deg")
        self.assertEqual(lua.describe_value("x.offset", {"x": 1, "y": 2.5}), "1 2.5")
        self.assertEqual(lua.describe_value("x.f", 0.30000000000000004), "0.3")
        self.assertIsNone(lua.describe_value("x", None))


class RequireLineTest(unittest.TestCase):
    CFG = "/cfg/hypr/hyprland.lua"
    MOD = "/cfg/hypr/hyprland-mod.lua"

    def test_added_once_and_idempotent(self):
        text = "hl.config({ general = { gaps_in = 1 } })"
        once = lua.with_require_line(text, self.CFG, self.MOD)
        self.assertTrue(once.startswith(text + "\n"))
        self.assertTrue(once.endswith('\npcall(require, "hyprland-mod")\n'))
        self.assertIsNone(lua.with_require_line(once, self.CFG, self.MOD))

    def test_existing_variants_recognized(self):
        for line in ('require("hyprland-mod")', "require 'hyprland-mod'",
                     'pcall(require, "hyprland-mod");'):
            text = f"x = 1\n{line}  -- mine\n-- trailing comment\n"
            self.assertIsNone(lua.with_require_line(text, self.CFG, self.MOD), line)

    def test_moved_to_end_and_deduplicated(self):
        text = 'pcall(require, "hyprland-mod")\nx = 1\nrequire("hyprland-mod")\ny = 2\n'
        out = lua.with_require_line(text, self.CFG, self.MOD)
        self.assertEqual(out.count("hyprland-mod\""), 1)
        self.assertIn("x = 1\ny = 2\n", out)

    def test_commented_line_does_not_count(self):
        text = '-- pcall(require, "hyprland-mod")\n--[[\nrequire("hyprland-mod")\n]]\nx = 1\n'
        out = lua.with_require_line(text, self.CFG, self.MOD)
        self.assertTrue(out.startswith(text))
        self.assertTrue(out.endswith('pcall(require, "hyprland-mod")\n'))

    def test_managed_outside_config_dir_uses_dofile(self):
        self.assertEqual(lua.require_line(self.CFG, "/elsewhere/mod.lua"), 'pcall(dofile, "/elsewhere/mod.lua")')


class LuaManagerTest(LuaConfigTest):
    def test_mode_and_paths(self):
        cm = self.make()
        self.assertEqual(cm.mode, "lua")
        self.assertEqual(cm.config_path, str(self.config))
        self.assertEqual(cm.managed_path, str(self.managed))

    def test_conf_env_with_lua_sibling_switches_to_lua(self):
        os.environ["HYPRLAND_MOD_CONFIG"] = str(self.hypr / "hyprland.conf")
        cm = self.make()
        self.assertEqual((cm.mode, cm.config_path), ("lua", str(self.config)))

    def test_dry_run_values(self):
        cm = self.make()
        self.assertIsNone(cm.get("decoration:rounding"))  # no live Hyprland: default
        self.assertEqual(cm.get_list("bezier")[0], "easeOutQuint, 0.23, 1, 0.32, 1")
        cm.set("decoration:rounding", "8")
        self.assertEqual(cm.get("decoration:rounding"), "8")

    def apply_sample(self, cm):
        cm.set("decoration:rounding", "8")
        cm.set("decoration:blur:enabled", "false")
        cm.set("decoration:blur:vibrancy", "0.2")
        cm.set("decoration:shadow:color", "0xee1a1a1a")
        cm.set("general:col.active_border", "rgba(ff0000ff) rgba(00ff00ff) 90deg")
        cm.set("general:layout", "master")
        cm.set("misc:vfr", "true")
        cm.set("plugin:my-plugin:label", 'a "quoted" #value')
        cm.set_list("bezier", ["snappy, 0.05, 0.9, 0.1, 1.05"])
        cm.set_list("animation", ["windows, 1, 3, snappy, popin 80%", "fade, 0"])
        return cm.apply()

    def test_apply_writes_lua_and_require_line(self):
        cm = self.make()
        result = self.apply_sample(cm)
        self.assertTrue(result.ok, result.errors)
        text = self.managed.read_text()
        self.assertIn("hl.config({\n    decoration = {\n", text)
        self.assertIn('active_border = { colors = { "rgba(ff0000ff)", "rgba(00ff00ff)" }, angle = 90 },', text)
        self.assertIn('hl.curve("snappy"', text)
        self.assertLess(text.index("hl.curve"), text.index("hl.animation"))
        self.assertEqual(len(self.require_lines()), 1)
        self.assertTrue(self.config.read_text().startswith(self.original_text))
        self.assertTrue(self.config.read_text().endswith('pcall(require, "hyprland-mod")\n'))
        self.assertFalse((self.hypr / "hyprland-mod.conf").exists())

        cm2 = self.make()  # state reads back from the generated file
        self.assertEqual(cm2.get("general:layout"), "master")
        self.assertEqual(cm2.get_list("animation"), ["windows, 1, 3, snappy, popin 80%", "fade, 0"])
        self.assertFalse(cm2.can_redo)

    def test_apply_twice_one_require_line(self):
        cm = self.make()
        self.apply_sample(cm)
        first = self.config.read_text()
        cm.set("decoration:rounding", "2")
        cm.apply()
        self.make().apply()
        self.assertEqual(self.config.read_text(), first)
        self.assertEqual(len(self.require_lines()), 1)

    @unittest.skipUnless(LUAC, "luac not installed")
    def test_generated_lua_compiles(self):
        self.apply_sample(self.make())
        for path in (self.managed, self.config):
            proc = subprocess.run([LUAC, "-p", str(path)], capture_output=True, text=True, timeout=10)
            self.assertEqual(proc.returncode, 0, proc.stderr)

    @unittest.skipUnless(LUA, "lua not installed")
    def test_generated_lua_executes_against_fake_hl(self):
        self.apply_sample(self.make())
        out = run_fake_hl(self.managed)  # no pcall here: any error fails the test
        for expected in (
            "cfg decoration.rounding=8", "cfg decoration.blur.enabled=false",
            "cfg decoration.blur.vibrancy=0.2", "cfg decoration.shadow.color=rgba(1a1a1aee)",
            "cfg general.col.active_border.angle=90",
            "cfg general.col.active_border.colors.1=rgba(ff0000ff)",
            "cfg general.layout=master", "cfg misc.vfr=true",
            'cfg plugin.my-plugin.label=a "quoted" #value',
            "curve snappy:bezier:0.05,0.9,0.1,1.05",
            "anim windows|true|3|snappy|popin 80%", "anim fade|false|nil|nil|nil",
        ):
            self.assertIn(expected, out)
        # Through the user's config: managed values win because they load last.
        full = run_fake_hl(self.config)
        self.assertIn("cfg decoration.rounding=8", full)
        self.assertIn("cfg general.gaps_in=5", full)
        self.assertEqual(full[-2:], ["anim windows|true|3|snappy|popin 80%", "anim fade|false|nil|nil|nil"])

    def test_invalid_lua_lists_rejected(self):
        cm = self.make()
        with self.assertRaises(ValueError):
            cm.set_list("bezier", ["broken"])
        with self.assertRaises(ValueError):
            cm.set_list("monitor", ["DP-1, preferred, auto, 1"])
        with self.assertRaises(ValueError):
            cm.import_state({"settings": {}, "lists": {"animation": ["x"]}})
        self.assertFalse(cm.is_dirty)

    def test_undo_reset_backup_in_lua_mode(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.set("decoration:rounding", "4")
        cm.apply()
        cm.undo()
        self.assertEqual(lua.read_lua_state(str(self.managed)).settings, {"decoration:rounding": "8"})
        cm.reset("decoration:rounding")
        cm.apply()
        self.assertEqual(lua.read_lua_state(str(self.managed)), ManagedState())
        original = cm.list_backups()[-1]
        self.assertEqual(original["files"], [str(self.config)])
        cm.restore_backup(original["id"])
        self.assertEqual(self.config.read_text(), self.original_text)

    def test_export_import_round_trip(self):
        cm = self.make()
        self.apply_sample(cm)
        state = cm.export_state()
        cm.import_state({"version": 1, "settings": {}, "lists": {}})
        cm.apply()
        cm.import_state(state)
        cm.apply()
        self.assertEqual(self.make().export_state(), state)


class LuaLiveTest(LuaConfigTest):
    DESCRIPTIONS = json.dumps([
        {"value": "decoration:rounding", "data": {"default": 0, "current": 10}},
        {"value": "general.col.active_border",
         "data": {"current": {"colors": ["rgba(33ccffee)", "rgba(00ff99ee)"], "angle": 45}}},
        {"value": "decoration:blur:enabled", "data": {"current": True}},
        {"value": "decoration:shadow:color", "data": {"current": 0xee1a1a1a}},
    ])

    def runner(self):
        def reply(args):
            if args == ["-j", "descriptions"]:
                return 0, self.DESCRIPTIONS, ""
            return 0, "ok", ""
        return FakeRunner(reply=reply)

    def test_reads_current_values_once(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            self.assertEqual(cm.get("decoration:rounding"), "10")
            self.assertEqual(cm.get("general:col.active_border"), "rgba(33ccffee) rgba(00ff99ee) 45deg")
            self.assertEqual(cm.get("decoration:blur:enabled"), "true")
            self.assertEqual(cm.get("decoration:shadow:color"), "rgba(1a1a1aee)")
            self.assertIsNone(cm.get("misc:vfr"))
        self.assertEqual(runner.calls, [["-j", "descriptions"]])

    def test_real_descriptions_shape(self):
        """Shape of real `hyprctl -j descriptions`: top-level name/current, bare-hex colors."""
        from hyprland_mod.core import parse_value
        self.DESCRIPTIONS = json.dumps([
            {"name": "general:col.active_border", "description": "", "default": "ffffffff 0deg",
             "current": "33ccffee 00ff99ee 45deg"},
            {"name": "general:col.inactive_border", "current": "595959aa 0deg"},
            {"name": "group:groupbar:text_color", "current": "ffffffff 0deg"},
            {"name": "general:gaps_out", "current": "20 20 20 20"},
            {"name": "general:layout", "current": "dwindle"},
            {"name": "decoration:rounding", "current": 10, "min": 0, "max": 20},
        ])
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            self.assertEqual(cm.get("general:col.active_border"), "rgba(33ccffee) rgba(00ff99ee) 45deg")
            self.assertEqual(cm.get("general:col.inactive_border"), "rgba(595959aa) 0deg")
            self.assertEqual(cm.get("general:gaps_out"), "20 20 20 20")
            self.assertEqual(cm.get("general:layout"), "dwindle")
            self.assertEqual(cm.get("decoration:rounding"), "10")
            color = {"type": "color", "default": ""}
            self.assertEqual(parse_value(color, cm.get("group:groupbar:text_color")), "rgba(ffffffff)")
            self.assertEqual(parse_value(color, "ffffffff 0deg"), "rgba(ffffffff)")
            self.assertEqual(parse_value({"type": "gradient"}, "595959aa 0deg"), "rgba(595959aa) 0deg")
            self.assertEqual(parse_value({"type": "int", "default": 0}, cm.get("general:gaps_out")), 20)
        self.assertEqual(runner.calls, [["-j", "descriptions"]])

    def test_eval_string(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "8")
            cm.set("general:col.active_border", "rgba(ff0000ff) rgba(00ff00ff) 90deg")
            cm.set_list("bezier", cm.get_list("bezier") + ["snap, 0, 1, 0, 1"])
            result = cm.apply()
        self.assertTrue(result.ok, result.errors)
        self.assertTrue(result.live_applied)
        self.assertEqual(runner.calls, [["eval",
            'hl.config({ decoration = { rounding = 8 }, general = { col = { active_border = '
            '{ colors = { "rgba(ff0000ff)", "rgba(00ff00ff)" }, angle = 90 } } } }); '
            'hl.curve("snap", { type = "bezier", points = { {0, 1}, {0, 1} } })']])

    @unittest.skipUnless(LUAC, "luac not installed")
    def test_eval_string_compiles(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            cm.set("misc:vfr", "true")
            cm.set("plugin:x:label", "it's \"fine\"")
            cm.set_list("animation", cm.get_list("animation") + ["border, 1, 2, default"])
            cm.apply()
        code = runner.calls[-1][1]
        path = self.tmp / "eval.lua"
        path.write_text(code)
        proc = subprocess.run([LUAC, "-p", str(path)], capture_output=True, text=True, timeout=10)
        self.assertEqual(proc.returncode, 0, proc.stderr)

    def test_removal_reloads(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "8")
            cm.apply()
            runner.calls.clear()
            cm.reset("decoration:rounding")
            cm.apply()
        self.assertEqual(runner.calls, [["reload"]])

    def test_eval_errors_reported(self):
        runner = FakeRunner((1, "", "lua error: bad"))
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "8")
            result = cm.apply()
        self.assertFalse(result.ok)
        self.assertEqual(result.errors, ["lua error: bad"])
        self.assertTrue(self.managed.exists())


if __name__ == "__main__":
    unittest.main()
