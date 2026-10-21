"""Preference rows generated from schema options."""

import math

from gi.repository import Adw, Gdk, GLib, Gtk

from . import colors
from .backend import current_text, current_value, format_value


def _rgba(t):
    c = Gdk.RGBA()
    c.red, c.green, c.blue, c.alpha = t
    return c


def _tuple(rgba):
    return (rgba.red, rgba.green, rgba.blue, rgba.alpha)


class _RowLogic:
    """Controller for one option; libadwaita row classes are final, so it wraps
    ``self.row`` and forwards unknown attributes to it."""

    def __getattr__(self, name):
        if name == "row":
            raise AttributeError(name)
        return getattr(self.row, name)

    def _init_logic(self, cm, opt):
        self.cm = cm
        self.opt = opt
        self.key = opt["key"]
        self._updating = False
        self.set_title(opt.get("label") or self.key)
        desc = opt.get("description")
        if desc and hasattr(self, "set_subtitle"):
            self.set_subtitle(GLib.markup_escape_text(desc))
        self.set_tooltip_text(self.key)
        self.add_css_class("option-row")
        if opt.get("advanced"):
            self.add_css_class("advanced-row")

        btn = Gtk.Button(icon_name="edit-undo-symbolic", valign=Gtk.Align.CENTER,
                         tooltip_text="Reset to default")
        btn.add_css_class("flat")
        btn.add_css_class("circular")
        btn.add_css_class("reset-button")
        btn.connect("clicked", lambda *_: self.cm.reset(self.key))
        btn.set_tooltip_text("Reset — go back to the value from your own config")
        self._reset_rev = Gtk.Revealer(transition_type=Gtk.RevealerTransitionType.SLIDE_LEFT,
                                       transition_duration=200, child=btn, valign=Gtk.Align.CENTER)

    def _attach_reset(self):
        self.add_suffix(self._reset_rev)

    def commit(self, text):
        if self._updating:
            return
        if text == current_text(self.cm, self.opt):
            return
        try:
            self.cm.set(self.key, text)
        except Exception as e:
            self.refresh()
            root = self.get_root()
            if hasattr(root, "toast"):
                root.toast(f"Couldn't change “{self.opt.get('label', self.key)}”: {e}")

    def refresh(self):
        self._updating = True
        try:
            self.load()
        finally:
            self._updating = False
        modified = bool(self.cm.is_modified(self.key))
        self._reset_rev.set_reveal_child(modified)
        if modified:
            self.add_css_class("modified")
        else:
            self.remove_css_class("modified")

    def load(self):
        raise NotImplementedError


class SwitchOptionRow(_RowLogic):
    def __init__(self, cm, opt):
        self.row = Adw.SwitchRow()
        self._init_logic(cm, opt)
        self._attach_reset()
        self.connect("notify::active", lambda *_: self.commit(format_value(self.opt, self.get_active())))
        self.refresh()

    def load(self):
        self.set_active(bool(current_value(self.cm, self.opt)))


class SpinOptionRow(_RowLogic):
    def __init__(self, cm, opt):
        self.row = Adw.SpinRow()
        self._init_logic(cm, opt)
        is_float = opt["type"] == "float"
        lo = opt.get("min", -1_000_000 if not is_float else -1e6)
        hi = opt.get("max", 1_000_000 if not is_float else 1e6)
        step = opt.get("step", 0.05 if is_float else 1)
        digits = opt.get("digits", 2 if is_float else 0)
        self.set_adjustment(Gtk.Adjustment(lower=lo, upper=hi, step_increment=step,
                                           page_increment=step * 10))
        self.set_digits(digits)
        self.set_climb_rate(1.0)
        self._unit = opt.get("unit") or ""
        if self._unit:
            self.connect("output", self._on_output)
            self.connect("input", self._on_input)
        self._attach_reset()
        self.connect("notify::value", self._on_value)
        self.refresh()

    def _on_output(self, row):
        v = row.get_adjustment().get_value()
        row.set_text(f"{v:.{row.get_digits()}f} {self._unit}")
        return True

    def _on_input(self, row):
        txt = row.get_text().replace(self._unit, "").strip()
        try:
            return True, float(txt)
        except ValueError:
            return False, 0.0

    def _on_value(self, *_):
        v = self.get_value()
        val = int(round(v)) if self.opt["type"] == "int" else round(v, max(self.get_digits(), 2))
        self.commit(format_value(self.opt, val))

    def load(self):
        v = current_value(self.cm, self.opt)
        try:
            self.set_value(float(v))
        except (TypeError, ValueError):
            pass


class ComboOptionRow(_RowLogic):
    def __init__(self, cm, opt):
        self.row = Adw.ComboRow()
        self._init_logic(cm, opt)
        self._values = [c["value"] if isinstance(c, dict) else c for c in opt.get("choices", [])]
        labels = [str(c.get("label", c["value"])) if isinstance(c, dict) else str(c) for c in opt.get("choices", [])]
        self._model = Gtk.StringList.new(labels)
        self.set_model(self._model)
        self._attach_reset()
        self.connect("notify::selected", self._on_selected)
        self.refresh()

    def _on_selected(self, *_):
        i = self.get_selected()
        if 0 <= i < len(self._values):
            self.commit(format_value(self.opt, self._values[i]))

    def _index(self, val):
        for i, v in enumerate(self._values):
            if v == val or str(v).lower() == str(val).lower():
                return i
        return -1

    def load(self):
        val = current_value(self.cm, self.opt)
        i = self._index(val)
        if i < 0:
            self._values.append(val)
            self._model.append(f"Custom ({val})")
            i = len(self._values) - 1
        self.set_selected(i)


class ColorOptionRow(_RowLogic):
    def __init__(self, cm, opt):
        self.row = Adw.ActionRow()
        self._init_logic(cm, opt)
        self._btn = Gtk.ColorDialogButton(dialog=Gtk.ColorDialog(with_alpha=True, title=opt.get("label", "")),
                                          valign=Gtk.Align.CENTER)
        self._btn.add_css_class("color-swatch")
        self.add_suffix(self._btn)
        self.set_activatable_widget(self._btn)
        self._attach_reset()
        self._btn.connect("notify::rgba", lambda *_: self.commit(colors.format_color(_tuple(self._btn.get_rgba()))))
        self.refresh()

    def load(self):
        self._btn.set_rgba(_rgba(colors.parse_color(current_text(self.cm, self.opt))))


class GradientSwatch(Gtk.DrawingArea):
    def __init__(self):
        super().__init__(content_width=64, content_height=24, valign=Gtk.Align.CENTER)
        self.add_css_class("gradient-swatch")
        self.colors, self.angle = [(0.5, 0.5, 0.5, 1)], 0
        self.set_draw_func(self._draw)

    def set_gradient(self, cols, angle):
        self.colors, self.angle = cols, angle
        self.queue_draw()

    def _draw(self, _a, cr, w, h):
        r = 6
        cr.new_sub_path()
        cr.arc(w - r, r, r, -math.pi / 2, 0)
        cr.arc(w - r, h - r, r, 0, math.pi / 2)
        cr.arc(r, h - r, r, math.pi / 2, math.pi)
        cr.arc(r, r, r, math.pi, 3 * math.pi / 2)
        cr.close_path()
        cr.clip()
        sq = 6
        for y in range(0, h, sq):
            for x in range(0, w, sq):
                g = 0.85 if (x // sq + y // sq) % 2 else 0.65
                cr.set_source_rgb(g, g, g)
                cr.rectangle(x, y, sq, sq)
                cr.fill()
        pat = gradient_pattern(self.colors, self.angle, 0, 0, w, h)
        cr.set_source(pat)
        cr.paint()


def gradient_pattern(cols, angle, x, y, w, h):
    import cairo
    a = math.radians(angle)
    dx, dy = math.cos(a), -math.sin(a)
    cx, cy = x + w / 2, y + h / 2
    half = abs(dx) * w / 2 + abs(dy) * h / 2
    pat = cairo.LinearGradient(cx - dx * half, cy - dy * half, cx + dx * half, cy + dy * half)
    n = len(cols)
    for i, c in enumerate(cols):
        pat.add_color_stop_rgba(i / (n - 1) if n > 1 else 0, *c)
    if n == 1:
        pat.add_color_stop_rgba(1, *cols[0])
    return pat


class GradientOptionRow(_RowLogic):
    MAX_COLORS = 4

    def __init__(self, cm, opt):
        self.row = Adw.ActionRow()
        self._init_logic(cm, opt)
        box = Gtk.Box(spacing=6, valign=Gtk.Align.CENTER)
        self._swatch = GradientSwatch()
        box.append(self._swatch)
        self._buttons_box = Gtk.Box(spacing=4)
        self._buttons_box.add_css_class("linked-swatches")
        box.append(self._buttons_box)

        self._add_btn = Gtk.Button(icon_name="list-add-symbolic", tooltip_text="Add color")
        self._rm_btn = Gtk.Button(icon_name="list-remove-symbolic", tooltip_text="Remove last color")
        for b in (self._add_btn, self._rm_btn):
            b.add_css_class("flat")
            b.add_css_class("circular")
            b.add_css_class("small-icon-button")
        self._add_btn.connect("clicked", self._on_add)
        self._rm_btn.connect("clicked", self._on_remove)
        box.append(self._rm_btn)
        box.append(self._add_btn)

        self._angle = Gtk.SpinButton(adjustment=Gtk.Adjustment(lower=0, upper=360, step_increment=15,
                                                               page_increment=45),
                                     valign=Gtk.Align.CENTER, tooltip_text="Gradient angle", width_chars=3)
        self._angle.connect("output", self._angle_output)
        self._angle.connect("input", self._angle_input)
        self._angle.connect("notify::value", lambda *_: self._emit())
        self._angle_rev = Gtk.Revealer(transition_type=Gtk.RevealerTransitionType.SLIDE_LEFT,
                                       transition_duration=200, child=self._angle)
        box.append(self._angle_rev)
        self.add_suffix(box)
        self._attach_reset()
        self._cols, self._deg = [], 0
        self._color_buttons = []
        self.refresh()

    def _angle_output(self, sb):
        sb.set_text(f"{int(sb.get_value())}°")
        return True

    def _angle_input(self, sb):
        try:
            return True, float(sb.get_text().replace("°", "").strip())
        except ValueError:
            return False, 0.0

    def _rebuild_buttons(self):
        while (c := self._buttons_box.get_first_child()):
            self._buttons_box.remove(c)
        self._color_buttons = []
        for i, col in enumerate(self._cols):
            b = Gtk.ColorDialogButton(dialog=Gtk.ColorDialog(with_alpha=True), rgba=_rgba(col),
                                      valign=Gtk.Align.CENTER)
            b.add_css_class("color-swatch")
            b.connect("notify::rgba", self._on_color, i)
            self._buttons_box.append(b)
            self._color_buttons.append(b)

    def _on_color(self, btn, _p, i):
        if self._updating or i >= len(self._cols):
            return
        self._cols[i] = _tuple(btn.get_rgba())
        self._emit()

    def _on_add(self, *_):
        if len(self._cols) >= self.MAX_COLORS:
            return
        self._cols.append(self._cols[-1])
        if len(self._cols) == 2 and not self._deg:
            self._deg = 45
        self._emit(force=True)

    def _on_remove(self, *_):
        if len(self._cols) > 1:
            self._cols.pop()
            self._emit(force=True)

    def _emit(self, force=False):
        if self._updating:
            return
        if not force:
            self._deg = int(self._angle.get_value())
        self.commit(colors.format_gradient(self._cols, self._deg))
        self._swatch.set_gradient(self._cols, self._deg)

    def load(self):
        cols, deg = colors.parse_gradient(current_text(self.cm, self.opt))
        if len(cols) != len(self._cols):
            self._cols = cols
            self._rebuild_buttons()
        else:
            self._cols = cols
            for b, c in zip(self._color_buttons, cols):
                b.set_rgba(_rgba(c))
        self._deg = deg
        self._angle.set_value(deg)
        self._angle_rev.set_reveal_child(len(cols) > 1)
        self._rm_btn.set_visible(len(cols) > 1)
        self._add_btn.set_sensitive(len(cols) < self.MAX_COLORS)
        self._swatch.set_gradient(cols, deg)


class EntryOptionRow(_RowLogic):
    def __init__(self, cm, opt):
        self.row = Adw.EntryRow(show_apply_button=True)
        self._init_logic(cm, opt)
        if opt.get("description"):
            self.set_tooltip_text(f"{opt['description']}\n{self.key}")
        self._attach_reset()
        self.connect("apply", self._on_apply)
        self.connect("changed", lambda *_: self.remove_css_class("error"))
        self.refresh()

    def _valid(self, text):
        if self.opt["type"] != "vec2":
            return True
        parts = text.split()
        try:
            return len(parts) == 2 and all(float(p) is not None for p in parts)
        except ValueError:
            return False

    def _on_apply(self, *_):
        text = self.get_text().strip()
        if not self._valid(text):
            self.add_css_class("error")
            return
        self.commit(text)

    def load(self):
        self.set_text(current_text(self.cm, self.opt))


_TYPES = {
    "bool": SwitchOptionRow,
    "int": SpinOptionRow,
    "float": SpinOptionRow,
    "enum": ComboOptionRow,
    "color": ColorOptionRow,
    "gradient": GradientOptionRow,
    "string": EntryOptionRow,
    "vec2": EntryOptionRow,
}


def build_row(cm, opt):
    cls = _TYPES.get(opt.get("type"), EntryOptionRow)
    if cls is ComboOptionRow and not opt.get("choices"):
        cls = EntryOptionRow
    return cls(cm, opt)
