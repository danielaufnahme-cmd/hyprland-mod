"""Keybinds, window rules, monitors, the actions catalog, and the restore/backup regressions."""

import json
import os
import shutil
import subprocess
import unittest
from unittest import mock

from _helpers import FIXTURES, FakeRunner, TempConfigTest
from test_lua import LUA, LUAC, LuaConfigTest, run_fake_hl
from hyprland_mod.core import actions, items, lua
from hyprland_mod.core.managed import ManagedState, read_managed

SAMPLE_ARGS = {"workspace": "3", "direction": "left", "app": "kitty", "command": 'notify-send "hi there"',
               "int": "2", "string": "magic"}


def sample_arg(action):
    if action["arg"] is None:
        return None
    if action["id"] == "resize_window":
        return "40 -20"
    return SAMPLE_ARGS[action["arg"]["type"]]


def luac_ok(test, path):
    if not LUAC:
        return
    proc = subprocess.run([LUAC, "-p", str(path)], capture_output=True, text=True, timeout=10)
    test.assertEqual(proc.returncode, 0, proc.stderr)


SAMPLE_BIND = {"mods": ["SUPER", "SHIFT"], "key": "T", "action": "launch_app", "arg": "kitty",
               "description": "Terminal"}
SAMPLE_RULE = {"name": "Float calculator", "match": {"class": "^(org.gnome.Calculator)$", "floating": False},
               "effects": {"float": True, "size": "800 600", "center": True, "opacity": "0.9 0.8",
                           "rounding": "4"}}


class ActionsCatalogTest(unittest.TestCase):
    def test_catalog_shape(self):
        self.assertGreaterEqual(len(actions.ACTIONS), 35)
        ids = [a["id"] for a in actions.ACTIONS]
        self.assertEqual(len(ids), len(set(ids)))
        for a in actions.ACTIONS:
            self.assertEqual({"id", "label", "category", "arg", "conf", "lua"} - a.keys(), set(), a["id"])
            if a["arg"] is not None:
                self.assertIn(a["arg"]["type"], {"workspace", "direction", "string", "app", "command", "int"})

    def test_conf_forms(self):
        self.assertEqual(actions.conf_dispatch("close_window"), ("killactive", ""))
        self.assertEqual(actions.conf_dispatch("focus_direction", "up"), ("movefocus", "u"))
        self.assertEqual(actions.conf_dispatch("workspace", "3"), ("workspace", "3"))
        self.assertEqual(actions.conf_dispatch("resize_window", "40, -20"), ("resizeactive", "40 -20"))
        self.assertEqual(actions.conf_dispatch("move_to_special", "magic"), ("movetoworkspace", "special:magic"))
        self.assertEqual(actions.conf_dispatch("toggle_special"), ("togglespecialworkspace", ""))
        self.assertEqual(actions.conf_dispatch("maximize"), ("fullscreen", "1"))

    def test_lua_forms(self):
        self.assertEqual(actions.lua_call("close_window"), "hl.dsp.window.close()")
        self.assertEqual(actions.lua_call("workspace", "3"), "hl.dsp.focus({ workspace = 3 })")
        self.assertEqual(actions.lua_call("workspace", "e+1"), 'hl.dsp.focus({ workspace = "e+1" })')
        self.assertEqual(actions.lua_call("focus_direction", "r"), 'hl.dsp.focus({ direction = "right" })')
        self.assertEqual(actions.lua_call("run_command", 'a "b"'), 'hl.dsp.exec_cmd("a \\"b\\"")')
        self.assertEqual(actions.lua_call("resize_window", "40 -20"),
                         "hl.dsp.window.resize({ x = 40, y = -20, relative = true })")
        self.assertEqual(actions.lua_call("toggle_special"), "hl.dsp.workspace.toggle_special()")

    def test_invalid_args(self):
        for action_id, arg in (("workspace", "three"), ("focus_direction", "north"), ("launch_app", ""),
                               ("resize_window", "40"), ("run_command", "a\nb")):
            with self.assertRaises(ValueError, msg=(action_id, arg)):
                actions.clean_arg(action_id, arg)
        with self.assertRaises(ValueError):
            actions.get("nope")

    def test_match_conf(self):
        self.assertEqual(actions.match_conf("killactive", ""), ("close_window", None))
        self.assertEqual(actions.match_conf("movefocus", "l"), ("focus_direction", "left"))
        self.assertEqual(actions.match_conf("exec", "kitty"), ("run_command", "kitty"))
        self.assertEqual(actions.match_conf("fullscreen", "1"), ("maximize", None))
        self.assertEqual(actions.match_conf("workspace", "e+1"), ("next_workspace", None))
        self.assertEqual(actions.match_conf("frobnicate", "x"), (None, None))

    @unittest.skipUnless(LUA, "lua not installed")
    def test_every_action_executes_against_fake_hl(self):
        """Each Lua form only uses dispatchers that exist in Hyprland 0.56's stubs."""
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "binds.lua")
            lines = [f'hl.bind("SUPER + F{i}", {actions.lua_call(a["id"], sample_arg(a))})'
                     for i, a in enumerate(actions.ACTIONS)]
            with open(path, "w") as fh:
                fh.write("\n".join(lines) + "\n")
            out = [ln for ln in run_fake_hl(path) if ln.startswith("bind ")]
        self.assertEqual(len(out), len(actions.ACTIONS))
        self.assertIn("bind SUPER + F0 -> exec_cmd(kitty)", out)

    @unittest.skipUnless(LUA, "lua not installed")
    def test_fake_hl_rejects_unknown_dispatchers(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            path = os.path.join(tmp, "bad.lua")
            with open(path, "w") as fh:
                fh.write('hl.bind("SUPER + X", hl.dsp.window.togglefloating())\n')
            with self.assertRaises(AssertionError):
                run_fake_hl(path)


class ItemValidationTest(unittest.TestCase):
    def test_bind_normalized(self):
        b = items.clean_bind({"mods": "shift super", "key": "t", "action": "workspace", "arg": " 3 "}, "lua")
        self.assertEqual((b["mods"], b["key"], b["arg"], b["flags"]), (["SUPER", "SHIFT"], "t", "3", ""))
        self.assertTrue(b["id"].startswith("bind-"))
        m = items.clean_bind({"mods": ["SUPER"], "key": "mouse:272", "action": "drag_move_window"}, "lua")
        self.assertEqual(m["flags"], "m")

    def test_bind_rejects(self):
        for bad in ({"mods": ["HYPER"], "key": "A", "action": "close_window"},
                    {"mods": [], "key": "A, B", "action": "close_window"},
                    {"mods": [], "key": "", "action": "close_window"},
                    {"mods": [], "key": "A", "action": None},
                    {"mods": [], "key": "A", "action": "close_window", "flags": "z"}):
            with self.assertRaises(ValueError, msg=bad):
                items.clean_bind(bad, "lua")
        with self.assertRaises(ValueError):
            items.clean_bind({**SAMPLE_BIND, "description": "a, b"}, "conf")

    def test_bind_renderings(self):
        b = items.clean_bind({**SAMPLE_BIND, "flags": "le"}, "conf")
        self.assertEqual(items.bind_conf(b), ("bindled", "SUPER SHIFT, T, Terminal, exec, kitty"))
        self.assertEqual(items.bind_lua(b), 'hl.bind("SUPER + SHIFT + T", hl.dsp.exec_cmd("kitty"), '
                                            '{ description = "Terminal", locked = true, repeating = true })')
        c = items.clean_bind({"mods": [], "key": "XF86AudioMute", "action": "close_window"}, "conf")
        self.assertEqual(items.bind_conf(c), ("bind", ", XF86AudioMute, killactive,"))

    def test_rule_renderings(self):
        r = items.clean_rule(SAMPLE_RULE, "conf", rule_id="rule-1")
        self.assertEqual(items.rule_conf(r), "float on, size 800 600, center on, opacity 0.9 0.8, rounding 4, "
                                             "match:class ^(org.gnome.Calculator)$, match:float false")
        self.assertEqual(items.rule_lua(r),
                         'hl.window_rule({ name = "hyprland-mod-rule-1", match = { class = '
                         '"^(org.gnome.Calculator)$", float = false }, float = true, size = "800 600", '
                         'center = true, opacity = "0.9 0.8", rounding = 4 })')
        for bad in ({"match": {}, "effects": {"float": True}}, {"match": {"class": "x"}, "effects": {}},
                    {"match": {"Class!": "x"}, "effects": {"float": True}},
                    {"match": {"class": "a\nb"}, "effects": {"float": True}}):
            with self.assertRaises(ValueError, msg=bad):
                items.clean_rule(bad, "lua")
        with self.assertRaises(ValueError):
            items.clean_rule({"match": {"class": "a,b"}, "effects": {"float": True}}, "conf")

    def test_monitor_renderings(self):
        m = items.clean_monitor({"mode": "2560x1440@239.97Hz", "position": "0x0", "scale": "1.25"})
        self.assertEqual(m, {"mode": "2560x1440@239.97", "position": "0x0", "scale": "1.25", "transform": 0,
                             "enabled": True})
        self.assertEqual(items.monitor_conf("DP-1", m), "DP-1, 2560x1440@239.97, 0x0, 1.25")
        self.assertEqual(items.monitor_lua("DP-1", {**m, "transform": 1}),
                         'hl.monitor({ output = "DP-1", mode = "2560x1440@239.97", position = "0x0", '
                         'scale = 1.25, transform = 1 })')
        self.assertEqual(items.monitor_conf("DP-1", {**m, "transform": 3}),
                         "DP-1, 2560x1440@239.97, 0x0, 1.25, transform, 3")
        off = items.clean_monitor({"enabled": False})
        self.assertEqual(items.monitor_conf("HDMI-A-1", off), "HDMI-A-1, disable")
        self.assertEqual(items.monitor_lua("HDMI-A-1", off), 'hl.monitor({ output = "HDMI-A-1", disabled = true })')
        for bad in ({"mode": "huge"}, {"position": "left"}, {"scale": "0"}, {"transform": 9}):
            with self.assertRaises(ValueError, msg=bad):
                items.clean_monitor(bad)
        with self.assertRaises(ValueError):
            items.clean_monitor_name("DP-1, 1080p")


# ---------------------------------------------------------------- regressions (Lua)

class LuaRestoreRegressionTest(LuaConfigTest):
    """Bug: restoring the original backup wrote a conf ``#`` header into hyprland-mod.lua."""

    def assert_valid_lua(self):
        text = self.managed.read_text()
        self.assertTrue(text.startswith("-- Hyprland Mod"), text[:80])
        self.assertNotIn("\n#", "\n" + text)
        luac_ok(self, self.managed)
        if LUA:
            run_fake_hl(self.managed)

    def test_apply_restore_apply_keeps_valid_lua(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        self.assertTrue(cm.apply().ok)
        original = cm.list_backups()[-1]
        self.assertTrue(cm.restore_backup(original["id"]).ok)
        self.assertEqual(self.config.read_text(), self.original_text)
        self.assert_valid_lua()
        self.assertEqual(lua.read_lua_state(str(self.managed)), ManagedState())

        cm.set("general:gaps_in", "3")
        cm.add_bind(SAMPLE_BIND)
        self.assertTrue(cm.apply().ok)
        self.assert_valid_lua()
        self.assertEqual(len(self.require_lines()), 1)
        luac_ok(self, self.config)
        if LUA:
            full = run_fake_hl(self.config)
            self.assertIn("cfg general.gaps_in=3", full)
            self.assertIn('bind SUPER + SHIFT + T -> exec_cmd(kitty) {description=Terminal}', full)

    def test_restore_in_fresh_manager_is_lua(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        original = cm.list_backups()[-1]["id"]
        self.make().restore_backup(original)  # e.g. after an app restart
        self.assert_valid_lua()

    def test_empty_state_writes_valid_lua(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.reset("decoration:rounding")
        self.assertTrue(cm.apply().ok)
        self.assert_valid_lua()
        self.assertEqual(lua.read_lua_state(str(self.managed)), ManagedState())

    def test_initial_backup_is_taken_before_require_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        original = cm.list_backups()[-1]
        self.assertIn("Original", original["label"])
        copy = next((self.data / "backups" / original["id"] / "files").iterdir()).read_text()
        self.assertEqual(copy, self.original_text)
        self.assertNotIn("hyprland-mod", copy)
        self.assertIn("hyprland-mod", self.config.read_text())

    def test_initial_backup_strips_preexisting_require_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        shutil.rmtree(self.data)  # app data lost; the require line is still in the config
        cm2 = self.make()
        cm2.set("decoration:rounding", "6")
        cm2.apply()
        original = cm2.list_backups()[-1]
        copy = next((self.data / "backups" / original["id"] / "files").iterdir()).read_text()
        self.assertEqual(copy, self.original_text)

    def test_empty_state_removes_require_line_and_apply_readds(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.undo()
        self.assertEqual(self.config.read_text(), self.original_text)
        self.assertEqual(self.require_lines(), [])
        cm.redo()
        self.assertEqual(len(self.require_lines()), 1)
        cm.reset("decoration:rounding")
        cm.apply()
        self.assertEqual(self.config.read_text(), self.original_text)


class ConfRestoreRegressionTest(TempConfigTest):
    def test_restore_writes_conf_header_in_conf_mode(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.restore_backup(cm.list_backups()[-1]["id"])
        self.assertTrue(self.managed.read_text().startswith("# Hyprland Mod"))
        self.assertEqual(self.config.read_text(), self.original_text)

    def test_empty_state_removes_source_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        self.assertEqual(len(self.source_lines()), 1)
        cm.reset("decoration:rounding")
        cm.apply()
        self.assertEqual(self.source_lines(), [])
        self.assertEqual(self.config.read_text(), self.original_text)
        cm.set("decoration:rounding", "9")
        cm.apply()
        self.assertEqual(len(self.source_lines()), 1)

    def test_initial_backup_before_source_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        original = cm.list_backups()[-1]
        copy = next((self.data / "backups" / original["id"] / "files").iterdir()).read_text()
        self.assertEqual(copy, self.original_text)


# ---------------------------------------------------------------- binds / rules / monitors (Lua)

class LuaItemsTest(LuaConfigTest):
    def test_binds_staged_applied_and_read_back(self):
        cm = self.make()
        bid = cm.add_bind(SAMPLE_BIND)
        self.assertTrue(cm.is_dirty)
        mod = [b for b in cm.list_binds() if b["source"] == "mod"]
        self.assertEqual(len(mod), 1)
        self.assertEqual(mod[0]["raw"], "exec kitty")
        self.assertFalse(self.managed.exists())
        self.assertTrue(cm.apply().ok)
        cm2 = self.make()
        self.assertEqual([b["id"] for b in cm2.list_binds() if b["source"] == "mod"], [bid])
        cm2.update_bind(bid, {"key": "Return", "action": "workspace", "arg": "2"})
        b = [b for b in cm2.list_binds() if b["id"] == bid][0]
        self.assertEqual((b["key"], b["action"], b["arg"], b["mods"]), ("Return", "workspace", "2", ["SUPER", "SHIFT"]))
        cm2.apply()
        self.assertIn('hl.bind("SUPER + SHIFT + Return", hl.dsp.focus({ workspace = 2 }), '
                      '{ description = "Terminal" })', self.managed.read_text())
        cm2.remove_bind(bid)
        with self.assertRaises(KeyError):
            cm2.remove_bind(bid)
        cm2.apply()
        self.assertEqual(self.require_lines(), [])  # nothing managed any more

    def test_user_binds_scanned_and_conflicts(self):
        cm = self.make()
        user = [b for b in cm.list_binds() if b["source"] == "user"]
        self.assertEqual([(b["mods"], b["key"]) for b in user], [(["SUPER"], "Q")])
        self.assertEqual(len(cm.find_conflicts(["SUPER"], "q")), 1)
        bid = cm.add_bind({"mods": ["SUPER"], "key": "Q", "action": "close_window"})
        conflicts = cm.find_conflicts("SUPER", "Q")
        self.assertEqual(sorted(c["source"] for c in conflicts), ["mod", "user"])
        self.assertEqual([c["source"] for c in cm.find_conflicts("SUPER", "Q", exclude_id=bid)], ["user"])
        self.assertEqual(cm.find_conflicts(["SUPER", "SHIFT"], "Q"), [])

    @unittest.skipUnless(LUA, "lua not installed")
    def test_generated_items_execute_against_fake_hl(self):
        cm = self.make()
        for i, a in enumerate(actions.ACTIONS):
            cm.add_bind({"mods": ["SUPER", "ALT"], "key": f"F{i + 1}", "action": a["id"], "arg": sample_arg(a)})
        cm.add_window_rule(SAMPLE_RULE)
        cm.add_window_rule({**SAMPLE_RULE, "enabled": False, "name": "off"})
        cm.set_monitor("DP-1", {"mode": "1920x1080@60", "position": "0x0", "scale": "1"})
        cm.set_monitor("HDMI-A-1", {"enabled": False})
        self.assertTrue(cm.apply().ok)
        luac_ok(self, self.managed)
        out = run_fake_hl(self.managed)
        self.assertEqual(len([ln for ln in out if ln.startswith("bind ")]), len(actions.ACTIONS))
        rules = [ln for ln in out if ln.startswith("rule ")]
        self.assertEqual(len(rules), 1)  # disabled rule is not emitted
        self.assertIn("match={class=^(org.gnome.Calculator)$,float=false}", rules[0])
        self.assertIn("monitor {mode=1920x1080@60,output=DP-1,position=0x0,scale=1,transform=0}", out)
        self.assertIn("monitor {disabled=true,output=HDMI-A-1}", out)
        full = run_fake_hl(self.config)
        self.assertEqual(len([ln for ln in full if ln.startswith("bind ")]), len(actions.ACTIONS) + 1)
        self.assertEqual(len(self.make().list_window_rules()), 2)

    def test_window_rules_crud(self):
        cm = self.make()
        rid = cm.add_window_rule(SAMPLE_RULE)
        cm.update_window_rule(rid, {"effects": {"workspace": "3"}, "enabled": False})
        rule = cm.list_window_rules()[0]
        self.assertEqual((rule["effects"], rule["enabled"], rule["name"]),
                         ({"workspace": "3"}, False, "Float calculator"))
        cm.apply()
        self.assertNotIn("hl.window_rule", self.managed.read_text())  # disabled
        cm.remove_window_rule(rid)
        self.assertEqual(cm.list_window_rules(), [])
        with self.assertRaises(KeyError):
            cm.update_window_rule("nope", {})

    def test_export_import_items(self):
        cm = self.make()
        cm.add_bind(SAMPLE_BIND)
        cm.add_window_rule(SAMPLE_RULE)
        cm.set_monitor("DP-1", {"scale": "1.5"})
        state = cm.export_state()
        self.assertEqual(json.loads(json.dumps(state)), state)
        self.assertEqual({len(state["binds"]), len(state["window_rules"]), len(state["monitors"])}, {1})
        cm.apply()
        cm.import_state({"version": 1, "settings": {"decoration:rounding": "3"}, "lists": {}})
        self.assertEqual(cm.export_state()["binds"], state["binds"])  # absent = kept
        cm.import_state({"version": 1, "settings": {}, "lists": {}, "binds": [], "window_rules": [],
                         "monitors": {}})
        cm.apply()
        self.assertEqual(self.make().export_state(), {"version": 1, "settings": {}, "lists": {}})
        cm.import_state(state)
        cm.apply()
        self.assertEqual(self.make().export_state(), state)
        with self.assertRaises(ValueError):
            cm.import_state({"binds": [{"mods": [], "key": "A", "action": "nope"}]})

    def test_undo_covers_items(self):
        cm = self.make()
        cm.add_bind(SAMPLE_BIND)
        cm.apply()
        cm.undo()
        self.assertEqual([b for b in cm.list_binds() if b["source"] == "mod"], [])
        cm.redo()
        self.assertEqual(len([b for b in cm.list_binds() if b["source"] == "mod"]), 1)


class LuaItemsLiveTest(LuaConfigTest):
    def runner(self, extra=None):
        replies = {("-j", "binds"): (FIXTURES / "binds.json").read_text(),
                   ("-j", "monitors", "all"): (FIXTURES / "monitors.json").read_text(),
                   ("-j", "animations"): (FIXTURES / "animations.json").read_text(),
                   ("-j", "descriptions"): json.dumps([{"name": "decoration:rounding", "current": 10}])}
        replies.update(extra or {})
        return FakeRunner(reply=lambda args: (0, replies.get(tuple(args), "ok"), ""))

    def test_live_binds_shape(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            binds = cm.list_binds()
            cm.list_binds()
        self.assertEqual(runner.calls.count(["-j", "binds"]), 1)  # cached
        self.assertEqual([(b["mods"], b["key"]) for b in binds],
                         [(["SUPER"], "Q"), (["SUPER", "SHIFT"], "C"), (["SUPER"], "left"), ([], "Escape")])
        q, c, left, esc = binds
        self.assertEqual((q["action"], q["raw"], q["source"]), (None, "", "user"))
        self.assertEqual((c["action"], c["description"], c["flags"]), ("close_window", "Close it", "le"))
        self.assertEqual((left["action"], left["arg"]), ("focus_direction", "left"))
        self.assertEqual(esc["submap"], "resize")
        with self.live_env():
            self.assertEqual(cm.find_conflicts([], "Escape"), [])  # submap binds don't clash

    def test_add_bind_live_eval_and_removal_reload(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            bid = cm.add_bind(SAMPLE_BIND)
            rid = cm.add_window_rule({"match": {"class": "kitty"}, "effects": {"float": True}})
            self.assertTrue(cm.apply().ok)
            evals = [c for c in runner.calls if c[0] == "eval"]
            self.assertEqual(len(evals), 1)
            self.assertIn('hl.bind("SUPER + SHIFT + T", hl.dsp.exec_cmd("kitty"), { description = "Terminal" })',
                          evals[0][1])
            self.assertIn(f'hl.window_rule({{ name = "hyprland-mod-{rid}", match = {{ class = "kitty" }}, '
                          'float = true })', evals[0][1])
            runner.calls.clear()
            # our applied bind is in hyprctl's list too: not shown twice
            live = FakeRunner(reply=lambda a: (0, json.dumps(json.loads((FIXTURES / "binds.json").read_text())
                                                             + [{"modmask": 65, "key": "T", "dispatcher": "__lua",
                                                                 "arg": "9", "submap": ""}]), ""))
            cm.runner = live
            self.assertEqual([b["source"] for b in cm.find_conflicts(["SUPER", "SHIFT"], "t")], ["mod"])
            cm.runner = runner
            cm.update_bind(bid, {"key": "Y"})
            cm.apply()
            self.assertEqual(runner.calls, [["reload"]])

    @unittest.skipUnless(LUAC, "luac not installed")
    def test_live_eval_compiles(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            for i, a in enumerate(actions.ACTIONS):
                cm.add_bind({"mods": ["SUPER"], "key": f"F{i + 1}", "action": a["id"], "arg": sample_arg(a)})
            cm.add_window_rule(SAMPLE_RULE)
            cm.set_monitor("DP-1", {"scale": "1.25"})
            cm.apply()
        code = [c for c in runner.calls if c[0] == "eval"][0][1]
        path = self.tmp / "eval.lua"
        path.write_text(code)
        luac_ok(self, path)

    def test_monitors_live_and_revert(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            mons = cm.list_monitors()
            self.assertEqual([m["name"] for m in mons], ["DP-1", "HDMI-A-1"])
            self.assertEqual(mons[0]["availableModes"][0], "2560x1440@239.97Hz")
            self.assertIsNone(mons[0]["override"])
            cm.set("decoration:rounding", "4")  # unrelated pending change survives
            cm.set_monitor("DP-1", {"scale": "1.5"})
            self.assertEqual(cm.list_monitors()[0]["override"],
                             {"mode": "2560x1440@239.97", "position": "0x0", "scale": "1.5", "transform": 0,
                              "enabled": True})
            runner.calls.clear()
            token = cm.apply_monitors_with_revert(timeout_s=15)
            self.assertTrue(cm.last_apply_result.ok)
            self.assertEqual(runner.calls[-1], ["eval", 'hl.monitor({ output = "DP-1", mode = "2560x1440@239.97", '
                                                        'position = "0x0", scale = 1.5, transform = 0 })'])
            self.assertEqual(cm.pending(), {"decoration:rounding": "4"})
            self.assertIsNotNone(cm.monitors_deadline(token))
            runner.calls.clear()
            result = cm.revert_monitors(token)
            self.assertTrue(result.ok)
            self.assertEqual(runner.calls, [["reload"]])  # override dropped: user's own monitor lines
            self.assertEqual(lua.read_lua_state(str(self.managed)).monitors, {})
            self.assertFalse(cm.revert_monitors(token).ok)
            self.assertEqual(cm.pending(), {"decoration:rounding": "4"})

            cm.set_monitor("DP-1", {"scale": "2"})
            token = cm.apply_monitors_with_revert()
            self.assertTrue(cm.confirm_monitors(token))
            self.assertFalse(cm.confirm_monitors(token))
            self.assertEqual(self.make(runner).list_monitors()[0]["override"]["scale"], "2")

    def test_snapshot_and_animations_live(self):
        runner = self.runner()
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "3")
            look = cm.snapshot_current_look(["decoration:rounding", "misc:vfr"])
            self.assertEqual(look, {"decoration:rounding": "10", "misc:vfr": None})
            anims = cm.current_animations()
        self.assertEqual(anims["bezier"], ["easeOutQuint, 0.23, 1, 0.32, 1"])
        self.assertEqual(anims["animation"], ["global, 1, 10, default", "windowsIn, 1, 4.1, easeOutQuint, popin 87%",
                                              "fade, 0"])


class LuaMonitorDryRunTest(LuaConfigTest):
    def test_fixture_and_managed_only(self):
        cm = self.make()
        self.assertEqual(cm.list_monitors(), [])
        cm.set_monitor("eDP-1", {"mode": "preferred"})
        self.assertEqual([(m["name"], m["override"]["position"]) for m in cm.list_monitors()], [("eDP-1", "auto")])
        with mock.patch.dict(os.environ, {"HYPRLAND_MOD_MONITORS_JSON": str(FIXTURES / "monitors.json")}):
            cm2 = self.make()
            self.assertEqual([m["name"] for m in cm2.list_monitors()], ["DP-1", "HDMI-A-1"])
            cm2.set_monitor("HDMI-A-1", {"position": "auto-right"})
            self.assertEqual(cm2.list_monitors()[1]["override"]["scale"], "1.25")
            token = cm2.apply_monitors_with_revert()
            self.assertTrue(cm2.last_apply_result.ok)
            self.assertIn('hl.monitor({ output = "HDMI-A-1"', self.managed.read_text())
            cm2.reset_monitor("HDMI-A-1")
            self.assertIsNone(cm2.list_monitors()[1]["override"])
            cm2.discard()
            cm2.revert_monitors(token)
            self.assertNotIn("hl.monitor", self.managed.read_text())

    def test_snapshot_dry_run(self):
        cm = self.make()
        cm.set("decoration:rounding", "3")
        cm.apply()
        self.assertEqual(cm.snapshot_current_look(["decoration:rounding"]), {"decoration:rounding": None})
        anims = cm.current_animations()
        self.assertEqual(anims["bezier"][0], "easeOutQuint, 0.23, 1, 0.32, 1")


class LuaAutostartTest(LuaConfigTest):
    CMD = 'qs -c "my bar" -d'

    def test_staged_applied_and_read_back(self):
        cm = self.make()
        self.assertIsNone(cm.get_autostart("bar"))
        cm.set_autostart("bar", self.CMD)
        self.assertTrue(cm.is_dirty)
        self.assertEqual(cm.get_autostart("bar"), self.CMD)
        self.assertTrue(cm.apply().ok)
        self.assertIn('hl.on("hyprland.start", function() hl.exec_cmd("qs -c \\"my bar\\" -d") end)',
                      self.managed.read_text())
        self.assertEqual(self.make().get_autostart("bar"), self.CMD)
        cm.set_autostart("bar", None)
        cm.apply()
        self.assertIsNone(self.make().get_autostart("bar"))
        self.assertEqual(self.require_lines(), [])

    def test_validation(self):
        cm = self.make()
        for bad_id, cmd in (("bar", "a\nb"), ("bar", ""), ("bad id", "x"), ("", "x")):
            with self.assertRaises(ValueError, msg=(bad_id, cmd)):
                cm.set_autostart(bad_id, cmd)
        self.assertFalse(cm.is_dirty)

    @unittest.skipUnless(LUA, "lua not installed")
    def test_generated_lua_executes(self):
        cm = self.make()
        cm.set_autostart("bar", self.CMD)
        cm.set_autostart("other", "nm-applet")
        cm.apply()
        luac_ok(self, self.managed)
        out = run_fake_hl(self.config)
        self.assertIn("on hyprland.start", out)
        self.assertIn('exec qs -c "my bar" -d', out)
        self.assertIn("exec nm-applet", out)

    def test_never_run_on_apply(self):
        runner = FakeRunner()
        with self.live_env():
            cm = self.make(runner)
            cm.set_autostart("bar", self.CMD)
            self.assertTrue(cm.apply().ok)
            cm.set_autostart("bar", "other-bar")
            cm.apply()
        self.assertFalse(any(c[0] in ("eval", "reload", "dispatch", "--batch") for c in runner.calls),
                         runner.calls)

    def test_export_import(self):
        cm = self.make()
        cm.set_autostart("bar", self.CMD)
        state = cm.export_state()
        self.assertEqual(state["autostart"], {"bar": self.CMD})
        cm.apply()
        cm.import_state({"version": 1, "settings": {}, "lists": {}})
        self.assertEqual(cm.get_autostart("bar"), self.CMD)  # absent = kept
        cm.import_state({"version": 1, "settings": {}, "lists": {}, "autostart": {}})
        self.assertIsNone(cm.get_autostart("bar"))
        cm.import_state(state)
        cm.apply()
        self.assertEqual(self.make().export_state(), state)
        with self.assertRaises(ValueError):
            cm.import_state({"autostart": {"bar": "a\nb"}})


# ---------------------------------------------------------------- conf backend

class ConfItemsTest(TempConfigTest):
    def test_user_binds_parsed(self):
        cm = self.make()
        user = [b for b in cm.list_binds() if b["source"] == "user"]
        self.assertEqual([(b["mods"], b["key"], b["action"]) for b in user],
                         [(["SUPER"], "Q", "run_command"), (["SUPER"], "C", "close_window"),
                          (["SUPER"], "mouse:272", "drag_move_window")])
        self.assertEqual(user[2]["flags"], "m")
        self.assertEqual(len(cm.find_conflicts(["SUPER"], "c")), 1)

    def test_render_and_read_back(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        bid = cm.add_bind({**SAMPLE_BIND, "arg": "kitty --title a#b"})
        cm.add_window_rule(SAMPLE_RULE)
        cm.set_monitor("DP-1", {"mode": "1920x1080@60", "position": "0x0", "scale": "1"})
        self.assertTrue(cm.apply().ok)
        text = self.managed.read_text()
        self.assertIn("bindd = SUPER SHIFT, T, Terminal, exec, kitty --title a##b\n", text)
        self.assertIn("windowrule = float on, size 800 600, center on, opacity 0.9 0.8, rounding 4, "
                      "match:class ^(org.gnome.Calculator)$, match:float false\n", text)
        self.assertIn("monitor = DP-1, 1920x1080@60, 0x0, 1\n", text)
        state = read_managed(str(self.managed))
        self.assertEqual(state.settings, {"decoration:rounding": "8"})
        self.assertEqual(state.lists, {})  # generated lines are not read back as lists
        self.assertEqual([b["id"] for b in state.binds], [bid])
        self.assertEqual(state.binds[0]["arg"], "kitty --title a#b")
        cm2 = self.make()
        self.assertEqual(cm2.export_state(), cm.export_state())
        self.assertFalse(cm2.is_dirty)
        self.assertTrue(cm2.can_undo)

    def test_live_keyword_batch(self):
        runner = FakeRunner((0, "ok", ""))
        with self.live_env():
            cm = self.make(runner)
            runner.calls.clear()
            cm.add_bind(SAMPLE_BIND)
            cm.add_window_rule({"match": {"class": "kitty"}, "effects": {"float": True}})
            cm.set_monitor("DP-1", {"mode": "preferred", "position": "auto", "scale": "auto"})
            cm.apply()
            self.assertEqual(runner.calls[-1], ["--batch", "keyword monitor DP-1, preferred, auto, auto ; "
                                                "keyword windowrule float on, match:class kitty ; "
                                                "keyword bindd SUPER SHIFT, T, Terminal, exec, kitty"])
            runner.calls.clear()
            cm.reset_monitor("DP-1")
            cm.apply()
            self.assertEqual(runner.calls[-1], ["reload"])

    def test_autostart_exec_once(self):
        runner = FakeRunner()
        with self.live_env():
            cm = self.make(runner)
            runner.calls.clear()
            cm.set_autostart("bar", "qs -c bar #1")
            cm.apply()
        self.assertEqual(runner.calls, [])  # next login only
        self.assertIn("exec-once = qs -c bar ##1\n", self.managed.read_text())
        state = read_managed(str(self.managed))
        self.assertEqual((state.autostart, state.lists), ({"bar": "qs -c bar #1"}, {}))
        self.assertEqual(self.make().get_autostart("bar"), "qs -c bar #1")

    def test_semicolon_command_reloads(self):
        runner = FakeRunner((0, "ok", ""))
        with self.live_env():
            cm = self.make(runner)
            cm.add_bind({"mods": ["SUPER"], "key": "B", "action": "run_command", "arg": "a; b"})
            cm.apply()
        self.assertEqual(runner.calls[-1], ["reload"])


if __name__ == "__main__":
    unittest.main()


class PendingCountTest(TempConfigTest):
    """The floating "N unsaved changes" bar counts every staged item separately."""

    def test_counts_each_item_change(self):
        from hyprland_mod.ui.backend import pending_count
        cm = self.make()
        self.assertEqual(pending_count(cm), 0)
        bid = cm.add_bind(SAMPLE_BIND)
        cm.add_bind({**SAMPLE_BIND, "key": "Y"})
        cm.add_window_rule(SAMPLE_RULE)
        cm.set_autostart("bar", "waybar")
        self.assertEqual(pending_count(cm), 4)
        self.assertTrue(cm.apply().ok)
        self.assertEqual(pending_count(cm), 0)
        cm.update_bind(bid, {"key": "U"})
        cm.remove_window_rule(cm.list_window_rules()[0]["id"])
        cm.set_autostart("bar", None)
        self.assertEqual(pending_count(cm), 3)
