import json
import os
import unittest

from _helpers import FakeRunner, TempConfigTest
from hyprland_mod.core import ApplyResult, ConfigManager
from hyprland_mod.core.parser import parse_config


class DefaultsTest(TempConfigTest):
    def test_env_paths(self):
        cm = self.make()
        self.assertEqual(cm.config_path, str(self.config))
        self.assertEqual(cm.managed_path, str(self.managed))
        self.assertEqual(cm.data_dir, str(self.data))
        self.assertTrue(cm.dry_run)

    def test_default_paths_without_env(self):
        # HOME points at a temp dir, so the real ~/.config/hypr is never even looked at.
        from unittest import mock
        from hyprland_mod.core.manager import detect_config
        home = self.tmp / "home"
        (home / ".config" / "hypr").mkdir(parents=True)
        with mock.patch.dict(os.environ, {"HOME": str(home)}):
            for var in ("HYPRLAND_MOD_CONFIG", "HYPRLAND_MOD_DATA"):
                os.environ.pop(var, None)
            with mock.patch.object(ConfigManager, "reload"), \
                    mock.patch.object(ConfigManager, "_load_history", return_value=([], 0)):
                cm = ConfigManager()
                self.assertTrue(cm.config_path.endswith("/.config/hypr/hyprland.conf"))
                self.assertTrue(cm.managed_path.endswith("/.config/hypr/hyprland-mod.conf"))
                self.assertTrue(cm.data_dir.endswith("/.local/share/hyprland-mod"))
                self.assertEqual(cm.mode, "conf")
                (home / ".config" / "hypr" / "hyprland.lua").write_text("")
                cm = ConfigManager()
                self.assertTrue(cm.config_path.endswith("/.config/hypr/hyprland.lua"))
                self.assertTrue(cm.managed_path.endswith("/.config/hypr/hyprland-mod.lua"))
                self.assertEqual(cm.mode, "lua")
        self.assertEqual(detect_config(str(self.tmp / "x.lua"))[1], "lua")
        self.assertEqual(detect_config(str(self.config))[1], "conf")


class GetPrecedenceTest(TempConfigTest):
    def test_user_config_values(self):
        cm = self.make()
        self.assertEqual(cm.get("decoration:blur:size"), "3")
        self.assertEqual(cm.get("dwindle:pseudotile"), "true")  # via source include
        self.assertEqual(cm.get("general:gaps_in"), "5")        # via $variable
        self.assertIsNone(cm.get("decoration:dim_inactive"))
        self.assertFalse(cm.is_modified("decoration:blur:size"))

    def test_pending_beats_managed_beats_user(self):
        self.managed.write_text("decoration {\n  blur {\n    size = 6\n  }\n}\n")
        cm = self.make()
        self.assertEqual(cm.get("decoration:blur:size"), "6")
        self.assertTrue(cm.is_modified("decoration:blur:size"))
        cm.set("decoration:blur:size", "9")
        self.assertEqual(cm.get("decoration:blur:size"), "9")
        cm.discard()
        self.assertEqual(cm.get("decoration:blur:size"), "6")
        cm.reset("decoration:blur:size")
        self.assertEqual(cm.get("decoration:blur:size"), "3")
        self.assertFalse(cm.is_modified("decoration:blur:size"))

    def test_managed_not_double_read_through_source(self):
        cm = self.make()
        cm.set("general:border_size", "4")
        cm.apply()
        cm2 = self.make()
        self.assertEqual(cm2.user_value("general:border_size"), "2")
        self.assertEqual(cm2.get("general:border_size"), "4")


class StagingTest(TempConfigTest):
    def test_set_pending_discard(self):
        cm = self.make()
        events = []
        cm.subscribe(lambda e, k: events.append((e, k)))
        self.assertFalse(cm.is_dirty)
        cm.set("decoration:rounding", "12")
        self.assertTrue(cm.is_dirty)
        self.assertEqual(cm.pending(), {"decoration:rounding": "12"})
        self.assertTrue(cm.is_modified("decoration:rounding"))
        self.assertFalse(self.managed.exists())
        cm.discard()
        self.assertFalse(cm.is_dirty)
        self.assertEqual(cm.get("decoration:rounding"), "10")
        self.assertEqual(events, [("changed", "decoration:rounding"), ("discarded", None)])

    def test_invalid_input_rejected(self):
        cm = self.make()
        for key in ("", "bad key", "a::b", "device:mouse:sensitivity", "source", "x{"):
            with self.assertRaises(ValueError, msg=key):
                cm.set(key, "1")
        for value in ("1\n2", "a } b"):
            with self.assertRaises(ValueError):
                cm.set("general:gaps_in", value)
        self.assertFalse(cm.is_dirty)

    def test_unsubscribe_and_bad_listener(self):
        cm = self.make()
        seen = []
        unsub = cm.subscribe(lambda e, k: seen.append(e))
        cm.subscribe(lambda e, k: 1 / 0)
        cm.set("general:gaps_in", "2")
        unsub()
        cm.set("general:gaps_in", "3")
        self.assertEqual(seen, ["changed"])


class ApplyTest(TempConfigTest):
    def test_apply_writes_managed_and_source_line(self):
        cm = self.make()
        cm.set("decoration:blur:enabled", "false")
        cm.set("decoration:rounding", "8")
        cm.set("general:col.active_border", "rgba(ff0000ff) rgba(00ff00ff) 45deg")
        cm.set("misc:vfr", "true")
        result = cm.apply()
        self.assertIsInstance(result, ApplyResult)
        self.assertEqual((result.ok, result.errors, result.live_applied), (True, [], False))
        self.assertFalse(cm.is_dirty)

        text = self.managed.read_text()
        self.assertIn("decoration {\n    rounding = 8\n    blur {\n        enabled = false\n", text)
        parsed = parse_config(self.managed)
        self.assertEqual(parsed.get("decoration:blur:enabled"), "false")
        self.assertEqual(parsed.get("general:col.active_border"), "rgba(ff0000ff) rgba(00ff00ff) 45deg")
        self.assertEqual(parsed.get("misc:vfr"), "true")

        self.assertEqual(len(self.source_lines()), 1)
        main = self.config.read_text()
        self.assertTrue(main.startswith(self.original_text), "user lines must be untouched")
        self.assertTrue(main.rstrip().endswith(f"source = {self.managed}"))
        # Whole config (with the source line) now resolves the new value.
        self.assertEqual(parse_config(self.config).get("decoration:rounding"), "8")

    def test_apply_twice_keeps_one_source_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        after_first = self.config.read_text()
        cm.set("decoration:rounding", "6")
        cm.apply()
        cm2 = self.make()
        cm2.set("decoration:rounding", "4")
        cm2.apply()
        self.assertEqual(self.config.read_text(), after_first)
        self.assertEqual(len(self.source_lines()), 1)

    def test_source_line_moved_to_end_if_not_last(self):
        self.config.write_text(f"source = {self.managed}\n" + self.original_text)
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        lines = [ln for ln in self.config.read_text().splitlines() if ln.strip()]
        self.assertEqual(len(self.source_lines()), 1)
        self.assertEqual(lines[-1], f"source = {self.managed}")
        self.assertIn(self.original_text, self.config.read_text())

    def test_existing_relative_source_line_recognized(self):
        self.config.write_text(self.original_text + "\nsource = ./hyprland-mod.conf # mine\n")
        before = self.config.read_text()
        self.managed.write_text("")
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        self.assertEqual(self.config.read_text(), before)

    def test_symlinked_config_stays_symlink(self):
        real = self.tmp / "dotfiles" / "hyprland.conf"
        real.parent.mkdir()
        os.replace(self.config, real)
        os.symlink(real, self.config)
        cm = self.make()
        cm.set("decoration:rounding", "8")
        self.assertTrue(cm.apply().ok)
        self.assertTrue(os.path.islink(self.config))
        self.assertIn("hyprland-mod.conf", real.read_text())

    def test_no_changes_is_noop(self):
        cm = self.make()
        self.assertTrue(cm.apply().ok)
        self.assertFalse(self.managed.exists())
        self.assertEqual(self.config.read_text(), self.original_text)
        self.assertEqual(cm.list_backups(), [])

    def test_missing_config_fails_safely(self):
        self.config.unlink()
        cm = self.make()
        cm.set("decoration:rounding", "8")
        result = cm.apply()
        self.assertFalse(result.ok)
        self.assertFalse(self.managed.exists())
        self.assertFalse(self.config.exists())
        self.assertTrue(cm.is_dirty)

    def test_hash_value_round_trip(self):
        cm = self.make()
        cm.set("plugin:test:label", "a#b")
        cm.apply()
        self.assertIn("a##b", self.managed.read_text())
        self.assertEqual(self.make().get("plugin:test:label"), "a#b")

    def test_applied_event(self):
        cm = self.make()
        events = []
        cm.subscribe(lambda e, k: events.append(e))
        cm.set("decoration:rounding", "8")
        cm.apply()
        self.assertEqual(events, ["changed", "applied"])


class ResetTest(TempConfigTest):
    def test_reset_removes_key(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.set("general:gaps_in", "1")
        cm.apply()
        cm.reset("decoration:rounding")
        self.assertTrue(cm.is_dirty)
        self.assertEqual(cm.pending_resets(), ["decoration:rounding"])
        self.assertEqual(cm.get("decoration:rounding"), "10")
        cm.apply()
        parsed = parse_config(self.managed)
        self.assertIsNone(parsed.get("decoration:rounding"))
        self.assertEqual(parsed.get("general:gaps_in"), "1")
        self.assertFalse(cm.is_modified("decoration:rounding"))

    def test_empty_value_acts_as_reset(self):
        cm = self.make()
        cm.set("group:groupbar:text_color_inactive", "rgba(ffffffff)")
        cm.apply()
        cm.set("group:groupbar:text_color_inactive", "")
        self.assertEqual(cm.pending_resets(), ["group:groupbar:text_color_inactive"])
        self.assertEqual(cm.pending(), {})
        cm.apply()
        self.assertFalse(cm.is_modified("group:groupbar:text_color_inactive"))
        self.assertIsNone(cm.get("group:groupbar:text_color_inactive"))

    def test_reset_of_pending_only(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.reset("decoration:rounding")
        self.assertFalse(cm.is_dirty)

    def test_setting_managed_value_again_is_not_dirty(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.set("decoration:rounding", "8")
        self.assertFalse(cm.is_dirty)


class ListTest(TempConfigTest):
    def test_lists_fall_back_to_user_config(self):
        cm = self.make()
        self.assertEqual(cm.get_list("bezier"),
                         ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"])
        self.assertEqual(cm.get_list("animations:animation"), cm.get_list("animation"))
        self.assertFalse(cm.is_list_modified("bezier"))

    def test_set_list_wins_wholesale(self):
        cm = self.make()
        cm.set_list("bezier", ["snappy, 0.05, 0.9, 0.1, 1.05"])
        cm.set_list("animation", ["windows, 1, 3, snappy", "fade, 0"])
        self.assertEqual(cm.get_list("bezier"), ["snappy, 0.05, 0.9, 0.1, 1.05"])
        cm.apply()
        text = self.managed.read_text()
        self.assertLess(text.index("bezier ="), text.index("animation ="))
        cm2 = self.make()
        self.assertEqual(cm2.get_list("animation"), ["windows, 1, 3, snappy", "fade, 0"])
        self.assertTrue(cm2.is_list_modified("animation"))
        cm2.reset_list("animation")
        self.assertEqual(len(cm2.get_list("animation")), 2)
        self.assertEqual(cm2.get_list("animation")[0], "windows, 1, 4.79, easeOutQuint")
        cm2.apply()
        self.assertNotIn("animation =", self.managed.read_text())
        self.assertIn("bezier = snappy", self.managed.read_text())

    def test_empty_list_is_kept_as_override(self):
        cm = self.make()
        cm.set_list("bezier", [])
        self.assertEqual(cm.get_list("bezier"), [])


class BackupTest(TempConfigTest):
    def test_first_apply_backs_up_original(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        backups = cm.list_backups()
        self.assertEqual(len(backups), 1)
        b = backups[0]
        self.assertEqual(set(b), {"id", "created", "label", "files"})
        self.assertIn("Original", b["label"])
        self.assertEqual(b["files"], [str(self.config)])
        copy = self.data / "backups" / b["id"] / "files"
        self.assertEqual(next(copy.iterdir()).read_text(), self.original_text)
        cm.set("decoration:rounding", "6")
        cm.apply()
        self.assertEqual(len(cm.list_backups()), 1, "source line already present, no new backup")

    def test_create_and_restore(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        bid = cm.create_backup("my snapshot")
        self.assertEqual(cm.list_backups()[0]["id"], bid)
        self.assertEqual(cm.list_backups()[0]["label"], "my snapshot")
        cm.set("decoration:rounding", "2")
        cm.apply()
        events = []
        cm.subscribe(lambda e, k: events.append(e))
        result = cm.restore_backup(bid)
        self.assertTrue(result.ok)
        self.assertEqual(cm.get("decoration:rounding"), "8")
        self.assertEqual(events, ["restored"])
        labels = [b["label"] for b in cm.list_backups()]
        self.assertTrue(any(lb.startswith("Before restoring") for lb in labels))

    def test_restore_original_removes_source_line(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        original = cm.list_backups()[-1]["id"]
        cm.restore_backup(original)
        self.assertEqual(self.config.read_text(), self.original_text)

    def test_restore_unknown_or_malicious_id(self):
        cm = self.make()
        for bad in ("nope", "../../etc", ""):
            self.assertFalse(cm.restore_backup(bad).ok)

    def test_corrupt_manifest_ignored(self):
        cm = self.make()
        bid = cm.create_backup("x")
        (self.data / "backups" / bid / "manifest.json").write_text("{broken")
        self.assertEqual(cm.list_backups(), [])

    def test_many_backups_unique_ids(self):
        cm = self.make()
        ids = {cm.create_backup(f"b{i}") for i in range(5)}
        self.assertEqual(len(ids), 5)


class UndoRedoTest(TempConfigTest):
    def test_undo_redo_cycle(self):
        cm = self.make()
        self.assertFalse(cm.can_undo)
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.set("decoration:rounding", "4")
        cm.apply()
        self.assertTrue(cm.can_undo)
        self.assertFalse(cm.can_redo)

        events = []
        cm.subscribe(lambda e, k: events.append(e))
        self.assertTrue(cm.undo().ok)
        self.assertEqual(cm.get("decoration:rounding"), "8")
        self.assertEqual(parse_config(self.managed).get("decoration:rounding"), "8")
        self.assertTrue(cm.can_redo)
        cm.undo()
        self.assertEqual(cm.get("decoration:rounding"), "10")  # back to user's value
        self.assertFalse(cm.can_undo)
        self.assertFalse(cm.undo().ok)
        cm.redo()
        self.assertEqual(cm.get("decoration:rounding"), "8")
        cm.redo()
        self.assertEqual(cm.get("decoration:rounding"), "4")
        self.assertFalse(cm.can_redo)
        self.assertEqual(events, ["undo", "undo", "redo", "redo"])

    def test_new_apply_clears_redo(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.undo()
        cm.set("decoration:rounding", "1")
        cm.apply()
        self.assertFalse(cm.can_redo)

    def test_history_persists(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm2 = self.make()
        self.assertTrue(cm2.can_undo)
        cm2.undo()
        self.assertEqual(cm2.get("decoration:rounding"), "10")

    def test_external_edit_resets_history(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        self.managed.write_text("decoration {\n rounding = 3\n}\n")
        cm2 = self.make()
        self.assertFalse(cm2.can_undo)
        self.assertEqual(cm2.get("decoration:rounding"), "3")

    def test_undo_discards_pending(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.apply()
        cm.set("general:gaps_in", "0")
        cm.undo()
        self.assertFalse(cm.is_dirty)


class ExportImportTest(TempConfigTest):
    def test_round_trip(self):
        cm = self.make()
        cm.set("decoration:rounding", "8")
        cm.set("decoration:blur:enabled", "false")
        cm.set_list("bezier", ["b1, 0, 0, 1, 1"])
        cm.apply()
        cm.set("general:gaps_in", "2")  # pending is part of the export
        state = cm.export_state()
        self.assertEqual(json.loads(json.dumps(state)), state)
        self.assertEqual(state["settings"]["general:gaps_in"], "2")
        self.assertEqual(state["lists"], {"bezier": ["b1, 0, 0, 1, 1"]})

        cm.discard()
        cm.set("misc:vfr", "false")
        cm.apply()
        cm.import_state(state)
        self.assertTrue(cm.is_dirty)
        self.assertEqual(cm.get("general:gaps_in"), "2")
        self.assertFalse(cm.is_modified("misc:vfr"))
        cm.apply()
        self.assertEqual(self.make().export_state(), state)

    def test_import_rejects_bad_data(self):
        cm = self.make()
        with self.assertRaises(ValueError):
            cm.import_state({"settings": {"bad key": "1"}})
        with self.assertRaises(ValueError):
            cm.import_state("nope")  # type: ignore[arg-type]
        self.assertFalse(cm.is_dirty)

    def test_import_into_empty_replaces_lists(self):
        cm = self.make()
        cm.set_list("animation", ["fade, 0"])
        cm.apply()
        cm.import_state({"version": 1, "settings": {"decoration:rounding": "0"}, "lists": {}})
        cm.apply()
        self.assertEqual(cm.export_state(),
                         {"version": 1, "settings": {"decoration:rounding": "0"}, "lists": {}})


class LiveApplyTest(TempConfigTest):
    def test_dry_run_never_calls_runner(self):
        runner = FakeRunner()
        cm = self.make(runner)
        cm.set("decoration:rounding", "8")
        result = cm.apply()
        self.assertFalse(result.live_applied)
        self.assertEqual(runner.calls, [])

    def test_batch_string_when_live(self):
        runner = FakeRunner((0, "ok\nok\n", ""))
        with self.live_env():
            cm = self.make(runner)
            self.assertTrue(cm.hyprland_running)
            self.assertFalse(cm.dry_run)
            cm.set("decoration:rounding", "8")
            cm.set("decoration:blur:enabled", "false")
            result = cm.apply()
        self.assertEqual(runner.calls, [[
            "--batch",
            "keyword decoration:blur:enabled false ; keyword decoration:rounding 8",
        ]])
        self.assertTrue(result.ok)
        self.assertTrue(result.live_applied)

    def test_lists_live_bezier_first(self):
        runner = FakeRunner((0, "ok", ""))
        with self.live_env():
            cm = self.make(runner)
            cm.set_list("animation", ["windows, 1, 4.79, easeOutQuint", "fade, 1, 3.03, linear",
                                      "border, 1, 2, snap"])
            cm.set_list("bezier", ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1",
                                   "snap, 0, 1, 0, 1"])
            cm.apply()
        self.assertEqual(runner.calls, [["--batch",
                                         "keyword bezier snap, 0, 1, 0, 1 ; keyword animation border, 1, 2, snap"]])

    def test_errors_collected(self):
        runner = FakeRunner((0, "ok\nconfig option <decoration:nope> does not exist.\n", ""))
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "8")
            cm.set("decoration:nope", "1")
            result = cm.apply()
        self.assertFalse(result.ok)
        self.assertFalse(result.live_applied)
        self.assertEqual(result.errors, ["config option <decoration:nope> does not exist."])
        self.assertTrue(self.managed.exists(), "file is still written")

    def test_runner_exception_does_not_break_save(self):
        def boom(args):
            raise OSError("socket gone")
        with self.live_env():
            cm = self.make(boom)
            cm.set("decoration:rounding", "8")
            result = cm.apply()
        self.assertFalse(result.ok)
        self.assertIn("socket gone", result.errors[0])
        self.assertEqual(parse_config(self.managed).get("decoration:rounding"), "8")

    def test_reset_live_uses_user_value_or_reload(self):
        runner = FakeRunner()
        with self.live_env():
            cm = self.make(runner)
            cm.set("decoration:rounding", "8")
            cm.set("decoration:dim_inactive", "true")
            cm.apply()
            runner.calls.clear()
            cm.reset("decoration:rounding")
            cm.apply()
            self.assertEqual(runner.calls, [["--batch", "keyword decoration:rounding 10"]])
            runner.calls.clear()
            cm.reset("decoration:dim_inactive")  # no user value: needs a reload
            cm.apply()
            self.assertEqual(runner.calls, [["reload"]])
            runner.calls.clear()
            cm.undo()
            self.assertEqual(runner.calls, [["--batch", "keyword decoration:dim_inactive true"]])

    def test_restore_reloads_when_live(self):
        runner = FakeRunner()
        with self.live_env():
            cm = self.make(runner)
            bid = cm.create_backup("x")
            runner.calls.clear()
            cm.restore_backup(bid)
        self.assertEqual(runner.calls, [["reload"]])


if __name__ == "__main__":
    unittest.main()
