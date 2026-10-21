import json
import tempfile
import unittest
from pathlib import Path

from hyprland_mod.core import load_schema
from hyprland_mod.core.schema import DEFAULT_SCHEMA_DIR


class LoadSchemaTest(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.dir = Path(self._tmp.name)
        self.addCleanup(self._tmp.cleanup)

    def write(self, name, data):
        (self.dir / name).write_text(data if isinstance(data, str) else json.dumps(data))

    def test_sorted_and_normalized(self):
        self.write("b.json", {"id": "decoration", "title": "Decoration", "icon": "x", "order": 20,
                              "description": "d", "groups": [{"title": "Blur", "options": [
                                  {"key": "decoration:blur:enabled", "label": "Blur", "type": "bool",
                                   "default": True}]}]})
        self.write("a.json", {"id": "general", "title": "General", "order": 10, "groups": [
            {"title": "Gaps", "options": [{"key": "general:gaps_in", "type": "int", "default": 5,
                                           "min": 0, "max": 50},
                                          {"label": "no key, dropped"}]}]})
        self.write("broken.json", "{not json")
        self.write("notes.txt", "ignored")
        pages = load_schema(self.dir)
        self.assertEqual([p["id"] for p in pages], ["general", "decoration"])
        general = pages[0]
        self.assertEqual(general["icon"], "preferences-system-symbolic")
        self.assertEqual(general["description"], "")
        opts = general["groups"][0]["options"]
        self.assertEqual(len(opts), 1)
        self.assertEqual(opts[0]["label"], "Gaps in")
        self.assertIs(opts[0]["advanced"], False)
        self.assertEqual(opts[0]["max"], 50)
        self.assertIs(pages[1]["groups"][0]["options"][0]["default"], True)

    def test_missing_dir_and_fields(self):
        self.assertEqual(load_schema(self.dir / "nope"), [])
        self.write("x.json", {})
        pages = load_schema(self.dir)
        self.assertEqual(pages[0]["id"], "x")
        self.assertEqual(pages[0]["groups"], [])
        self.assertEqual(pages[0]["order"], 1000)

    def test_choices_normalized(self):
        self.write("e.json", {"id": "e", "groups": [{"options": [
            {"key": "general:layout", "type": "enum", "choices": ["dwindle", {"value": "master", "label": "M"}]}]}]})
        opt = load_schema(self.dir)[0]["groups"][0]["options"][0]
        self.assertEqual(opt["choices"], [{"value": "dwindle", "label": "dwindle"},
                                          {"value": "master", "label": "M"}])

    def test_default_dir_location(self):
        self.assertEqual(DEFAULT_SCHEMA_DIR, Path(__file__).resolve().parents[1] / "data" / "schema")
        self.assertIsInstance(load_schema(), list)


if __name__ == "__main__":
    unittest.main()
