import unittest

from _helpers import FIXTURES
from hyprland_mod.core.parser import parse_config, parse_text, resolve_vars, strip_comment


class StripCommentTest(unittest.TestCase):
    def test_comments_and_escapes(self):
        self.assertEqual(strip_comment("a = b # c"), "a = b")
        self.assertEqual(strip_comment("# only"), "")
        self.assertEqual(strip_comment("a = ##b # c"), "a = #b")
        self.assertEqual(strip_comment("   key   =   v   "), "key   =   v")


class FixtureTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cfg = parse_config(FIXTURES / "sample_hyprland.conf")

    def test_nested_sections(self):
        self.assertEqual(self.cfg.get("decoration:blur:enabled"), "true")
        self.assertEqual(self.cfg.get("decoration:blur:vibrancy"), "0.1696")
        self.assertEqual(self.cfg.get("decoration:shadow:range"), "4")
        self.assertEqual(self.cfg.get("input:touchpad:natural_scroll"), "false")

    def test_variables_resolved(self):
        self.assertEqual(self.cfg.get("general:gaps_in"), "5")
        self.assertEqual(self.cfg.get("general:col.active_border"),
                         "rgba(33ccffee) rgba(00ff99ee) 45deg")
        self.assertEqual(self.cfg.get_all("bind")[0], "SUPER, Q, exec, kitty")
        self.assertEqual(self.cfg.variables["mainMod"], "SUPER")

    def test_source_include_followed(self):
        self.assertEqual(self.cfg.get("dwindle:pseudotile"), "true")
        self.assertEqual(self.cfg.get("cursor:no_hardware_cursors"), "true")
        self.assertEqual(len(self.cfg.files), 2)
        # extra.conf sets gaps_out=12 but main config sets 10 later: later wins
        self.assertEqual(self.cfg.get("general:gaps_out"), "10")

    def test_inline_category_and_comments(self):
        self.assertEqual(self.cfg.get("misc:force_default_wallpaper"), "0")
        self.assertEqual(self.cfg.get("misc:disable_hyprland_logo"), "true")
        self.assertEqual(self.cfg.get("plugin:test:label"), "#notacomment")

    def test_device_block_skipped(self):
        self.assertEqual(self.cfg.get("input:sensitivity"), "0")
        self.assertIsNone(self.cfg.get("name"))
        self.assertFalse(any(e.key.startswith("device") for e in self.cfg.entries))

    def test_repeated_keywords(self):
        self.assertEqual(self.cfg.get_all("bezier"),
                         ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"])
        self.assertEqual(len(self.cfg.get_all("animation")), 2)
        self.assertEqual(len(self.cfg.get_all("monitor")), 2)
        self.assertEqual(self.cfg.get_all("exec-once"), ["waybar", "kitty --class scratch"])

    def test_unknown_lines_kept(self):
        self.assertEqual(self.cfg.get("some_future_keyword"), "kept-as-is")
        self.assertEqual(self.cfg.warnings, [])


class EdgeCaseTest(unittest.TestCase):
    def test_one_line_and_close_on_value_line(self):
        cfg = parse_text("general { border_size = 3 }\nmisc {\n  vfr = true }\nx = 1\n")
        self.assertEqual(cfg.get("general:border_size"), "3")
        self.assertEqual(cfg.get("misc:vfr"), "true")
        self.assertEqual(cfg.get("x"), "1")

    def test_unbalanced_braces_warn_not_crash(self):
        cfg = parse_text("}\ngeneral {\n gaps_in = 1\n")
        self.assertEqual(cfg.get("general:gaps_in"), "1")
        self.assertTrue(cfg.warnings)

    def test_garbage_line_warns(self):
        cfg = parse_text("this is not valid\nok = 1\n")
        self.assertEqual(cfg.get("ok"), "1")
        self.assertEqual(len(cfg.warnings), 1)

    def test_nested_device_blocks_skipped(self):
        cfg = parse_text("device {\n name = a\n inner {\n x = 1\n }\n}\ny = 2\n")
        self.assertEqual([e.key for e in cfg.entries], ["y"])

    def test_longest_variable_wins(self):
        self.assertEqual(resolve_vars("$ab-$a", {"a": "1", "ab": "2"}), "2-1")
        self.assertEqual(resolve_vars("$missing", {"a": "1"}), "$missing")

    def test_recursive_source_is_safe(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as d:
            Path(d, "a.conf").write_text("source = ./b.conf\nx = 1\n")
            Path(d, "b.conf").write_text("source = ./a.conf\ny = 2\n")
            cfg = parse_config(Path(d, "a.conf"))
            self.assertEqual((cfg.get("x"), cfg.get("y")), ("1", "2"))
            self.assertTrue(any("recursive" in w for w in cfg.warnings))

    def test_missing_source_warns(self):
        import tempfile
        from pathlib import Path
        with tempfile.TemporaryDirectory() as d:
            Path(d, "a.conf").write_text("source = ./nope.conf\nx = 1\n")
            cfg = parse_config(Path(d, "a.conf"))
            self.assertEqual(cfg.get("x"), "1")
            self.assertEqual(len(cfg.warnings), 1)

    def test_animations_block_aliases(self):
        cfg = parse_text("animations {\n bezier = b, 0, 0, 1, 1\n}\nbezier = c, 1, 1, 1, 1\n")
        self.assertEqual(cfg.get_all("bezier"), ["b, 0, 0, 1, 1", "c, 1, 1, 1, 1"])


if __name__ == "__main__":
    unittest.main()
