"""Validate the settings schema in data/schema/*.json."""
import glob
import json
import os
import re
import unittest

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA_DIR = os.path.join(ROOT, "data", "schema")
SOURCE = os.path.join(SCHEMA_DIR, "source", "hypr-options-0.56.json")
TYPES = {"bool", "int", "float", "enum", "color", "gradient", "string", "vec2"}
COLOR_RE = re.compile(r"^rgba?\([0-9a-fA-F]{6,8}\)$")
GRAD_RE = re.compile(r"^(rgba?\([0-9a-fA-F]{6,8}\)\s*)+(-?\d+deg)?$")


def load_pages():
    pages = []
    for path in sorted(glob.glob(os.path.join(SCHEMA_DIR, "*.json"))):
        with open(path, encoding="utf-8") as fh:
            pages.append((path, json.load(fh)))
    return pages


def all_options(pages):
    for path, page in pages:
        for group in page["groups"]:
            for opt in group["options"]:
                yield path, opt


class SchemaTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pages = load_pages()
        with open(SOURCE, encoding="utf-8") as fh:
            cls.source = {o["name"]: o for o in json.load(fh)}

    def test_files_exist(self):
        self.assertGreaterEqual(len(self.pages), 10)

    def test_page_format(self):
        ids, orders = set(), set()
        for path, p in self.pages:
            base = os.path.splitext(os.path.basename(path))[0]
            self.assertEqual(p["id"], base, path)
            for f in ("title", "icon", "description"):
                self.assertIsInstance(p[f], str, path)
                self.assertTrue(p[f].strip(), path)
            self.assertTrue(p["icon"].endswith("-symbolic"), path)
            self.assertIsInstance(p["order"], int, path)
            self.assertNotIn(p["id"], ids)
            self.assertNotIn(p["order"], orders)
            ids.add(p["id"])
            orders.add(p["order"])
            self.assertTrue(p["groups"], path)
            for g in p["groups"]:
                self.assertTrue(g["title"].strip(), path)
                self.assertTrue(g["options"], path)

    def test_option_format(self):
        for path, o in all_options(self.pages):
            k = o["key"]
            for f in ("label", "description"):
                self.assertTrue(o[f].strip(), k)
            self.assertNotRegex(o["label"], r"[\U0001F000-\U0001FFFF]", k)
            self.assertIn(o["type"], TYPES, k)
            self.assertIn("default", o, k)
            self.assertLessEqual(set(o), {
                "key", "label", "description", "type", "default", "min", "max",
                "step", "digits", "unit", "choices", "advanced"}, k)
            if "advanced" in o:
                self.assertIs(o["advanced"], True, k)

    def test_keys_unique_and_known(self):
        seen = set()
        for _, o in all_options(self.pages):
            self.assertNotIn(o["key"], seen, "duplicate " + o["key"])
            seen.add(o["key"])
            self.assertIn(o["key"], self.source, "unknown " + o["key"])

    def test_defaults_and_ranges(self):
        for _, o in all_options(self.pages):
            k, t, d = o["key"], o["type"], o["default"]
            if t == "bool":
                self.assertIsInstance(d, bool, k)
            elif t == "int":
                self.assertIsInstance(d, int, k)
                self.assertNotIsInstance(d, bool, k)
            elif t == "float":
                self.assertIsInstance(d, float, k)
            elif t == "enum":
                values = [c["value"] for c in o["choices"]]
                self.assertEqual(len(values), len(set(values)), k)
                self.assertIn(d, values, k)
                for c in o["choices"]:
                    self.assertTrue(c["label"].strip(), k)
            elif t == "color":
                self.assertTrue(d == "" or COLOR_RE.match(d), (k, d))
            elif t == "gradient":
                self.assertRegex(d, GRAD_RE, k)
            else:
                self.assertIsInstance(d, str, k)
            if t == "vec2":
                self.assertRegex(d, r"^-?[\d.]+ -?[\d.]+$", k)
            if t in ("int", "float"):
                lo, hi = o.get("min"), o.get("max")
                if lo is not None and hi is not None:
                    self.assertLessEqual(lo, hi, k)
                if lo is not None:
                    self.assertLessEqual(lo, d, k)
                if hi is not None:
                    self.assertLessEqual(d, hi, k)
                if "step" in o:
                    self.assertGreater(o["step"], 0, k)
            else:
                for f in ("min", "max", "step", "digits"):
                    self.assertNotIn(f, o, k)
            if t != "enum":
                self.assertNotIn("choices", o, k)

    def test_defaults_match_source(self):
        for _, o in all_options(self.pages):
            sd = self.source[o["key"]]["default"]
            if o["type"] == "bool":
                self.assertEqual(o["default"], sd, o["key"])
            elif o["type"] == "float":
                self.assertAlmostEqual(o["default"], float(sd), 6, o["key"])
            elif o["type"] == "int" and isinstance(sd, (int, float)):
                self.assertEqual(o["default"], int(sd), o["key"])

    def test_coverage(self):
        used = {o["key"] for _, o in all_options(self.pages)}
        n = len(used & set(self.source))
        total = len(self.source)
        print("\ncoverage: %d/%d (%.1f%%)" % (n, total, 100.0 * n / total))
        self.assertGreaterEqual(n / total, 0.90)


if __name__ == "__main__":
    unittest.main()
