import unittest

from hyprland_mod.core import format_value, parse_value
from hyprland_mod.core.values import color_to_rgba_tuple, format_float, parse_color

BOOL = {"type": "bool", "default": False}
INT = {"type": "int", "default": 2}
FLOAT = {"type": "float", "default": 0.5}
COLOR = {"type": "color", "default": "0xff444444"}
GRADIENT = {"type": "gradient", "default": "rgba(ffffffff)"}
STRING = {"type": "string", "default": "dwindle"}
VEC2 = {"type": "vec2", "default": "0 0"}
ENUM = {"type": "enum", "default": "dwindle",
        "choices": [{"value": "dwindle", "label": "Dwindle"}, {"value": "master", "label": "Master"}]}
INT_ENUM = {"type": "enum", "default": 0,
            "choices": [{"value": 0, "label": "Off"}, {"value": 1, "label": "On"}, {"value": 2, "label": "Loose"}]}


class BoolTest(unittest.TestCase):
    def test_parse_words(self):
        for text in ("true", "yes", "on", "1", " TRUE "):
            self.assertIs(parse_value(BOOL, text), True, text)
        for text in ("false", "no", "off", "0"):
            self.assertIs(parse_value(BOOL, text), False, text)

    def test_format_and_fallback(self):
        self.assertEqual(format_value(BOOL, True), "true")
        self.assertEqual(format_value(BOOL, False), "false")
        self.assertEqual(format_value(BOOL, "yes"), "true")
        self.assertIs(parse_value(BOOL, "maybe"), False)
        self.assertIs(parse_value(BOOL, None), False)
        self.assertIs(parse_value({"type": "bool", "default": "true"}, None), True)


class NumberTest(unittest.TestCase):
    def test_int(self):
        self.assertEqual(parse_value(INT, "7"), 7)
        self.assertEqual(parse_value(INT, "0x10"), 16)
        self.assertEqual(parse_value(INT, "3.0"), 3)
        self.assertEqual(parse_value(INT, "abc"), 2)
        self.assertEqual(format_value(INT, 4.6), "5")

    def test_float(self):
        self.assertEqual(format_value(FLOAT, 1.0), "1")
        self.assertEqual(format_value(FLOAT, 0.1696), "0.1696")
        self.assertEqual(format_value(FLOAT, 0.30000000000000004), "0.3")
        self.assertEqual(format_value({"type": "float", "digits": 2}, 0.456), "0.46")
        self.assertEqual(format_value(FLOAT, -0.0), "0")
        self.assertEqual(parse_value(FLOAT, "0.95"), 0.95)
        self.assertEqual(parse_value(FLOAT, "bad"), 0.5)
        self.assertEqual(format_float(10), "10")


class ColorTest(unittest.TestCase):
    def test_forms(self):
        self.assertEqual(parse_value(COLOR, "rgba(33ccffee)"), "rgba(33ccffee)")
        self.assertEqual(parse_value(COLOR, "rgba(33CCFFEE)"), "rgba(33ccffee)")
        self.assertEqual(parse_value(COLOR, "rgb(33ccff)"), "rgba(33ccffff)")
        self.assertEqual(parse_value(COLOR, "0xee33ccff"), "rgba(33ccffee)")
        self.assertEqual(parse_value(COLOR, "rgba(255, 0, 128, 0.5)"), "rgba(ff008080)")
        self.assertEqual(parse_value(COLOR, "rgb(255, 0, 128)"), "rgba(ff0080ff)")
        self.assertEqual(parse_value(COLOR, "#112233"), "rgba(112233ff)")

    def test_default_and_invalid(self):
        self.assertEqual(parse_value(COLOR, None), "rgba(444444ff)")
        self.assertEqual(parse_value(COLOR, "notacolor"), "rgba(444444ff)")
        self.assertIsNone(parse_color("rgba(12345)"))
        self.assertIsNone(parse_color("rgba(1, 2)"))
        with self.assertRaises(ValueError):
            format_value(COLOR, "nope")

    def test_empty_means_automatic(self):
        auto = {"type": "color", "default": ""}
        self.assertEqual(parse_value(auto, None), "")
        self.assertEqual(parse_value(auto, ""), "")
        self.assertEqual(format_value(auto, ""), "")
        self.assertEqual(format_value(COLOR, None), "")
        self.assertEqual(format_value(GRADIENT, ""), "")
        self.assertEqual(parse_value({"type": "gradient", "default": ""}, None), "")

    def test_format(self):
        self.assertEqual(format_value(COLOR, "0xff112233"), "rgba(112233ff)")
        self.assertEqual(format_value(COLOR, (255, 0, 0)), "rgba(ff0000ff)")
        self.assertEqual(format_value(COLOR, (0, 255, 0, 0.5)), "rgba(00ff0080)")
        self.assertEqual(color_to_rgba_tuple("rgba(01020304)"), (1, 2, 3, 4))


class OtherTypesTest(unittest.TestCase):
    def test_gradient_and_string(self):
        g = "rgba(33ccffee)   rgba(00ff99ee) 45deg"
        self.assertEqual(parse_value(GRADIENT, g), "rgba(33ccffee) rgba(00ff99ee) 45deg")
        self.assertEqual(format_value(GRADIENT, g), "rgba(33ccffee) rgba(00ff99ee) 45deg")
        self.assertEqual(parse_value(STRING, " master "), "master")
        self.assertEqual(parse_value(STRING, None), "dwindle")

    def test_vec2(self):
        self.assertEqual(parse_value(VEC2, "10 20"), [10, 20])
        self.assertEqual(parse_value(VEC2, "1.5, -2"), [1.5, -2])
        self.assertEqual(parse_value(VEC2, None), [0, 0])
        self.assertEqual(parse_value(VEC2, "1 2 3"), [0, 0])
        self.assertEqual(format_value(VEC2, (10, 20.5)), "10 20.5")
        self.assertEqual(format_value(VEC2, "3,4"), "3 4")

    def test_enum(self):
        self.assertEqual(parse_value(ENUM, "Master"), "master")
        self.assertEqual(parse_value(ENUM, None), "dwindle")
        self.assertEqual(parse_value(INT_ENUM, "2"), 2)
        self.assertEqual(format_value(INT_ENUM, 1), "1")
        self.assertEqual(parse_value(ENUM, "custom"), "custom")

    def test_missing_optional_fields(self):
        self.assertEqual(parse_value({"type": "enum"}, "3"), 3)
        self.assertIsNone(parse_value({}, None))
        self.assertEqual(format_value({}, "x"), "x")


class RoundTripTest(unittest.TestCase):
    CASES = [
        (BOOL, True), (BOOL, False), (INT, 0), (INT, -12), (FLOAT, 0.25), (FLOAT, 3.0),
        (COLOR, "rgba(1a2b3c4d)"), (GRADIENT, "rgba(ffffffff) rgba(000000ff) 90deg"),
        (STRING, "hello world"), (VEC2, [3, 4.5]), (ENUM, "master"), (INT_ENUM, 2),
    ]

    def test_round_trips(self):
        for option, value in self.CASES:
            with self.subTest(type=option["type"], value=value):
                text = format_value(option, value)
                self.assertIsInstance(text, str)
                self.assertEqual(parse_value(option, text), value)
                self.assertEqual(format_value(option, parse_value(option, text)), text)


if __name__ == "__main__":
    unittest.main()
