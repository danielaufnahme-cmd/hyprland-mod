"""Animated mock desktop that mirrors the current look settings."""

import math

import cairo
from gi.repository import Adw, Gtk

from . import colors
from .backend import current_text

KEYS = {
    "gaps_in": "general:gaps_in",
    "gaps_out": "general:gaps_out",
    "border": "general:border_size",
    "active": "general:col.active_border",
    "inactive": "general:col.inactive_border",
    "rounding": "decoration:rounding",
    "shadow_on": "decoration:shadow:enabled",
    "shadow_range": "decoration:shadow:range",
    "shadow_color": "decoration:shadow:color",
    "dim_on": "decoration:dim_inactive",
    "dim_strength": "decoration:dim_strength",
    "a_op": "decoration:active_opacity",
    "i_op": "decoration:inactive_opacity",
    "blur": "decoration:blur:enabled",
}
FALLBACK = {
    "gaps_in": "5", "gaps_out": "20", "border": "2",
    "active": "rgba(33ccffee) rgba(00ff99ee) 45deg", "inactive": "rgba(595959aa)",
    "rounding": "10", "shadow_on": "true", "shadow_range": "4", "shadow_color": "rgba(1a1a1aee)",
    "dim_on": "false", "dim_strength": "0.5", "a_op": "1.0", "i_op": "1.0", "blur": "true",
}
STOPS = 4
VIRTUAL_H = 440


def _num(text, default=0.0):
    try:
        return float(str(text).replace(",", " ").split()[0])
    except (ValueError, IndexError):
        return default


def _truthy(text):
    return str(text).strip().lower() in ("1", "true", "yes", "on")


def _sample(cols, t):
    if len(cols) == 1:
        return cols[0]
    pos = t * (len(cols) - 1)
    i = min(int(pos), len(cols) - 2)
    return colors.mix(cols[i], cols[i + 1], pos - i)


def _resample(cols):
    return [_sample(cols, i / (STOPS - 1)) for i in range(STOPS)]


def _lerp_state(a, b, t):
    out = {}
    for k, va in a.items():
        vb = b[k]
        if isinstance(va, list):
            out[k] = [colors.mix(x, y, t) for x, y in zip(va, vb)]
        elif isinstance(va, tuple):
            out[k] = colors.mix(va, vb, t)
        else:
            out[k] = va + (vb - va) * t
    return out


def _rounded(cr, x, y, w, h, r):
    r = max(0.0, min(r, w / 2, h / 2))
    if r < 0.5:
        cr.rectangle(x, y, w, h)
        return
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


class LivePreview(Gtk.DrawingArea):
    def __init__(self, cm):
        super().__init__(hexpand=True, content_height=320)
        self.add_css_class("live-preview")
        self.cm = cm
        self._opts = {}
        self._wall = None
        self._wall_blur = None
        self._wall_size = (0, 0)
        self._shown = self._read()
        self._from = self._shown
        self._target = self._shown
        self._anim = Adw.TimedAnimation.new(self, 0.0, 1.0, 260, Adw.CallbackAnimationTarget.new(self._tick))
        self._anim.set_easing(Adw.Easing.EASE_OUT_CUBIC)
        self.set_draw_func(self._draw)

    def set_options(self, opts_by_key):
        self._opts = opts_by_key
        self._shown = self._target = self._read()
        self.queue_draw()

    def _text(self, name):
        key = KEYS[name]
        opt = self._opts.get(key)
        if opt is not None:
            return current_text(self.cm, opt)
        raw = None
        try:
            raw = self.cm.get(key)
        except Exception:
            pass
        return raw if raw is not None else FALLBACK[name]

    def _read(self):
        a_cols, a_deg = colors.parse_gradient(self._text("active"))
        i_cols, i_deg = colors.parse_gradient(self._text("inactive"))
        return {
            "gaps_in": _num(self._text("gaps_in"), 5),
            "gaps_out": _num(self._text("gaps_out"), 20),
            "border": _num(self._text("border"), 2),
            "rounding": _num(self._text("rounding"), 10),
            "shadow": _num(self._text("shadow_range"), 4) if _truthy(self._text("shadow_on")) else 0.0,
            "shadow_alpha": 1.0 if _truthy(self._text("shadow_on")) else 0.0,
            "shadow_color": colors.parse_color(self._text("shadow_color"), (0.1, 0.1, 0.1, 0.9)),
            "dim": _num(self._text("dim_strength"), 0.5) if _truthy(self._text("dim_on")) else 0.0,
            "a_op": _num(self._text("a_op"), 1),
            "i_op": _num(self._text("i_op"), 1),
            "blur": 1.0 if _truthy(self._text("blur")) else 0.0,
            "a_cols": _resample(a_cols),
            "a_deg": float(a_deg),
            "i_cols": _resample(i_cols),
            "i_deg": float(i_deg),
        }

    def handles(self, key):
        return key is None or key in KEYS.values()

    def update(self, animate=True):
        target = self._read()
        if target == self._target:
            return
        self._from = self._shown
        self._target = target
        if animate and self.get_mapped():
            self._anim.reset()
            self._anim.play()
        else:
            self._shown = target
            self.queue_draw()

    def _tick(self, t):
        self._shown = _lerp_state(self._from, self._target, t)
        self.queue_draw()

    # -- drawing -------------------------------------------------------

    def _ensure_wallpaper(self, w, h):
        if self._wall_size == (w, h) and self._wall:
            return
        self._wall_size = (w, h)
        self._wall = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        cr = cairo.Context(self._wall)
        bg = cairo.LinearGradient(0, 0, w, h)
        bg.add_color_stop_rgb(0, 0.07, 0.15, 0.24)
        bg.add_color_stop_rgb(0.55, 0.10, 0.27, 0.36)
        bg.add_color_stop_rgb(1, 0.13, 0.36, 0.40)
        cr.set_source(bg)
        cr.paint()
        for (fx, fy, fr, col) in ((0.82, 0.12, 0.55, (0.27, 0.72, 0.70, 0.55)),
                                  (0.12, 0.95, 0.60, (0.22, 0.45, 0.85, 0.50)),
                                  (0.45, 0.45, 0.35, (0.55, 0.80, 0.90, 0.18))):
            rg = cairo.RadialGradient(fx * w, fy * h, 0, fx * w, fy * h, fr * max(w, h))
            rg.add_color_stop_rgba(0, *col)
            rg.add_color_stop_rgba(1, col[0], col[1], col[2], 0)
            cr.set_source(rg)
            cr.paint()
        # soft hills
        cr.move_to(0, h * 0.78)
        cr.curve_to(w * 0.3, h * 0.62, w * 0.55, h * 0.92, w, h * 0.70)
        cr.line_to(w, h)
        cr.line_to(0, h)
        cr.close_path()
        cr.set_source_rgba(0.04, 0.10, 0.16, 0.45)
        cr.fill()

        f = 14
        small = cairo.ImageSurface(cairo.FORMAT_ARGB32, max(1, w // f), max(1, h // f))
        sc = cairo.Context(small)
        sc.scale(1 / f, 1 / f)
        sc.set_source_surface(self._wall)
        sc.get_source().set_filter(cairo.FILTER_GOOD)
        sc.paint()
        self._wall_blur = cairo.ImageSurface(cairo.FORMAT_ARGB32, w, h)
        bc = cairo.Context(self._wall_blur)
        bc.scale(f, f)
        bc.set_source_surface(small)
        bc.get_source().set_filter(cairo.FILTER_BILINEAR)
        bc.get_source().set_extend(cairo.EXTEND_PAD)
        bc.paint()

    def _draw(self, _area, cr, w, h):
        if w <= 0 or h <= 0:
            return
        st = self._shown
        self._ensure_wallpaper(w, h)
        s = h / VIRTUAL_H

        cr.set_source_surface(self._wall, 0, 0)
        cr.paint()

        # top bar
        bar_h = 30 * s
        go = max(0.0, st["gaps_out"]) * s
        gi = max(0.0, st["gaps_in"]) * s
        _rounded(cr, 10 * s, 7 * s, w - 20 * s, bar_h - 8 * s, 8 * s)
        cr.set_source_rgba(0.02, 0.05, 0.09, 0.40)
        cr.fill()
        for i in range(4):
            cr.arc(26 * s + i * 16 * s, 7 * s + (bar_h - 8 * s) / 2, (4.2 if i == 0 else 3.2) * s, 0, 2 * math.pi)
            cr.set_source_rgba(1, 1, 1, 0.9 if i == 0 else 0.35)
            cr.fill()
        _rounded(cr, w / 2 - 34 * s, 12 * s, 68 * s, bar_h - 18 * s, 5 * s)
        cr.set_source_rgba(1, 1, 1, 0.55)
        cr.fill()
        for i in range(3):
            _rounded(cr, w - 30 * s - i * 20 * s, 13 * s, 12 * s, bar_h - 20 * s, 3 * s)
            cr.set_source_rgba(1, 1, 1, 0.35)
            cr.fill()

        top = bar_h + 2 * s
        ax, ay = go, top + go
        aw, ah = w - 2 * go, h - top - 2 * go
        if aw < 40 or ah < 40:
            return
        left_w = (aw - 2 * gi) * 0.55
        right_x = ax + left_w + 2 * gi
        right_w = aw - left_w - 2 * gi
        top_h = (ah - 2 * gi) * 0.5
        wins = [
            ((ax, ay, left_w, ah), True, "editor"),
            ((right_x, ay, right_w, top_h), False, "terminal"),
            ((right_x, ay + top_h + 2 * gi, right_w, ah - top_h - 2 * gi), False, "files"),
        ]
        for rect, active, kind in reversed(wins):
            self._draw_window(cr, rect, active, kind, st, s)

    def _draw_window(self, cr, rect, active, kind, st, s):
        x, y, w, h = rect
        r = max(0.0, st["rounding"]) * s
        bs = max(0.0, st["border"]) * s
        if 0 < st["border"] and bs < 1:
            bs = 1.0
        op = max(0.0, min(1.0, st["a_op"] if active else st["i_op"]))

        # shadow
        rng = st["shadow"] * s
        if rng > 0.3 and st["shadow_alpha"] > 0:
            sc = st["shadow_color"]
            steps = max(4, min(16, int(rng)))
            for i in range(steps, 0, -1):
                grow = rng * i / steps
                a = sc[3] * st["shadow_alpha"] * (1 - i / (steps + 1)) ** 2 * 0.35 * (0.6 + 0.4 * op)
                _rounded(cr, x - grow, y - grow + rng * 0.15, w + 2 * grow, h + 2 * grow, r + grow)
                cr.set_source_rgba(sc[0], sc[1], sc[2], a)
                cr.fill()

        # border
        if bs > 0:
            cols, deg = (st["a_cols"], st["a_deg"]) if active else (st["i_cols"], st["i_deg"])
            from .rows import gradient_pattern
            _rounded(cr, x, y, w, h, r)
            cr.set_source(gradient_pattern(cols, deg, x, y, w, h))
            cr.fill()

        ix, iy, iw, ih = x + bs, y + bs, w - 2 * bs, h - 2 * bs
        if iw <= 2 or ih <= 2:
            return
        ir = max(0.0, r - bs)

        cr.save()
        _rounded(cr, ix, iy, iw, ih, ir)
        cr.clip()
        if bs > 0:
            # punch the inner area so translucent windows don't show the border fill
            cr.set_source_surface(self._wall, 0, 0)
            cr.paint()
        if op < 0.999 and st["blur"] > 0:
            cr.set_source_surface(self._wall_blur, 0, 0)
            cr.paint_with_alpha(st["blur"])
        cr.push_group()
        self._draw_content(cr, kind, ix, iy, iw, ih, s)
        cr.pop_group_to_source()
        cr.paint_with_alpha(op)
        if not active and st["dim"] > 0:
            cr.set_source_rgba(0, 0, 0, max(0.0, min(1.0, st["dim"])) * 0.75)
            cr.paint()
        cr.restore()

    def _draw_content(self, cr, kind, x, y, w, h, s):
        def bar(px, py, pw, ph, rgba, rad=None):
            _rounded(cr, px, py, max(0, pw), ph, ph / 2 if rad is None else rad)
            cr.set_source_rgba(*rgba)
            cr.fill()

        if kind == "terminal":
            cr.set_source_rgb(0.11, 0.12, 0.15)
            cr.paint()
            lh = 15 * s
            pal = [(0.40, 0.80, 0.65, 1), (0.45, 0.65, 0.95, 1), (0.85, 0.85, 0.88, 0.75)]
            yy = y + 14 * s
            widths = [0.32, 0.55, 0.42, 0.18, 0.62, 0.28, 0.48, 0.36, 0.22, 0.52]
            for i, fw in enumerate(widths):
                if yy + lh > y + h - 6 * s:
                    break
                bar(x + 14 * s, yy, 8 * s, 7 * s, pal[0])
                bar(x + 26 * s, yy, (w - 40 * s) * fw, 7 * s, pal[1] if i % 3 == 0 else pal[2])
                yy += lh
            bar(x + 14 * s, yy, 7 * s, 9 * s, (0.9, 0.9, 0.92, 0.9), 1)
        elif kind == "editor":
            cr.set_source_rgb(0.98, 0.98, 0.985)
            cr.paint()
            hb = 34 * s
            cr.rectangle(x, y, w, hb)
            cr.set_source_rgb(0.93, 0.93, 0.945)
            cr.fill()
            bar(x + 14 * s, y + 13 * s, 60 * s, 8 * s, (0.2, 0.2, 0.25, 0.35))
            for i in range(3):
                cr.arc(x + w - 18 * s - i * 18 * s, y + hb / 2, 5 * s, 0, 2 * math.pi)
                cr.set_source_rgba(0.2, 0.2, 0.25, 0.18)
                cr.fill()
            side = min(150 * s, w * 0.28)
            cr.rectangle(x, y + hb, side, h - hb)
            cr.set_source_rgb(0.955, 0.955, 0.965)
            cr.fill()
            yy = y + hb + 16 * s
            for i in range(7):
                bar(x + 14 * s, yy, side * (0.45 + 0.08 * (i % 3)), 7 * s,
                    (0.21, 0.52, 0.89, 0.85) if i == 1 else (0.2, 0.2, 0.25, 0.22))
                yy += 20 * s
            syntax = [(0.75, 0.25, 0.45, 0.85), (0.21, 0.52, 0.89, 0.85), (0.15, 0.6, 0.5, 0.85),
                      (0.2, 0.2, 0.25, 0.45)]
            code = [(0, 0.22, 0, 0.34), (1, 0.18, 3, 0.4), (1, 0.30, 2, 0.2), (2, 0.12, 3, 0.45),
                    (0, 0.0, 0, 0.0), (0, 0.26, 3, 0.3), (1, 0.14, 3, 0.5), (2, 0.2, 1, 0.24),
                    (1, 0.35, 3, 0.18), (0, 0.0, 0, 0.0), (0, 0.18, 2, 0.4), (1, 0.22, 3, 0.36)]
            cx = x + side + 22 * s
            yy = y + hb + 18 * s
            avail = w - side - 40 * s
            for indent, (c1, w1, c2, w2) in zip([0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 1], code):
                if yy > y + h - 14 * s:
                    break
                if w1:
                    ox = cx + indent * 18 * s
                    bar(ox, yy, avail * w1, 7 * s, syntax[c1])
                    bar(ox + avail * w1 + 6 * s, yy, avail * w2, 7 * s, syntax[c2])
                yy += 18 * s
        else:
            cr.set_source_rgb(0.965, 0.965, 0.97)
            cr.paint()
            hb = 30 * s
            cr.rectangle(x, y, w, hb)
            cr.set_source_rgb(0.92, 0.92, 0.935)
            cr.fill()
            bar(x + 12 * s, y + 11 * s, 44 * s, 8 * s, (0.2, 0.2, 0.25, 0.3))
            tile, gap = 38 * s, 16 * s
            cols = max(1, int((w - gap) // (tile + gap)))
            ox = x + (w - cols * (tile + gap) + gap) / 2
            yy = y + hb + 14 * s
            tints = [(0.21, 0.52, 0.89), (0.32, 0.65, 0.78), (0.95, 0.70, 0.30), (0.45, 0.72, 0.50)]
            k = 0
            while yy + tile < y + h - 4 * s:
                for c in range(cols):
                    t = tints[k % len(tints)]
                    _rounded(cr, ox + c * (tile + gap), yy, tile, tile * 0.78, 5 * s)
                    cr.set_source_rgba(t[0], t[1], t[2], 0.85)
                    cr.fill()
                    bar(ox + c * (tile + gap) + tile * 0.15, yy + tile * 0.86, tile * 0.7, 5 * s,
                        (0.2, 0.2, 0.25, 0.3))
                    k += 1
                yy += tile + gap
