"""Bar builder: design a Waybar bar by clicking, with an animated live preview.

Nothing here writes outside the app's own Waybar folder (see hyprland_mod.bar).
The design autosaves to the data dir; "Use This Bar" writes the Waybar files and
(re)starts the waybar process this app owns.
"""

import math
import re
from pathlib import Path

import gi

gi.require_version("PangoCairo", "1.0")
gi.require_version("Graphene", "1.0")
from gi.repository import Adw, Gdk, Gio, GLib, GObject, Graphene, Gtk, Pango, PangoCairo  # noqa: E402

from .. import bar as barlib  # noqa: E402
from ..bar.modules import MODULES, sample_label, workspace_labels  # noqa: E402
from .presets_page import dismissed, file_filter, filter_list, toast  # noqa: E402

CSS_PATH = Path(__file__).resolve().parent.parent.parent / "styles" / "bar.css"
_css_loaded = False

# calm dusk wallpaper (slate blue -> sand), never purple
WALL = [(0.25, 0.38, 0.50), (0.47, 0.58, 0.64), (0.82, 0.70, 0.56)]
TRAY_COLORS = [(0.20, 0.52, 0.89), (0.18, 0.76, 0.49), (0.96, 0.62, 0.20)]
STYLE_LABELS = {"solid": "Solid", "islands": "Islands", "pills": "Capsules"}
WEIGHT_LABELS = [("normal", "Regular", Pango.Weight.NORMAL), ("medium", "Medium", Pango.Weight.MEDIUM),
                 ("bold", "Bold", Pango.Weight.BOLD)]
SAVE_DELAY_MS = 400
_GLYPH_GAP = re.compile("([\U000F0000-\U000FFFFF]) ")
POLL_S = 4


def load_css():
    global _css_loaded
    if _css_loaded or not CSS_PATH.exists():
        return
    _css_loaded = True
    provider = Gtk.CssProvider()
    provider.load_from_path(str(CSS_PATH))
    Gtk.StyleContext.add_provider_for_display(Gdk.Display.get_default(), provider,
                                              Gtk.STYLE_PROVIDER_PRIORITY_APPLICATION)


def _hex(text):
    return tuple(int(text[i:i + 2], 16) / 255 for i in (1, 3, 5))


def _to_hex(rgba):
    return "#" + "".join(f"{max(0, min(255, round(c * 255))):02x}" for c in (rgba.red, rgba.green, rgba.blue))


def _rgba(text):
    c = Gdk.RGBA()
    c.parse(text)
    return c


def _lerp(a, b, t):
    if isinstance(a, tuple):
        return tuple(x + (y - x) * t for x, y in zip(a, b))
    return a + (b - a) * t


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


_nerd = None


def has_nerd_font():
    """Icons in the preview need a Nerd Font; without one show text labels instead of tofu."""
    global _nerd
    if _nerd is None:
        fm = PangoCairo.FontMap.get_default()
        _nerd = any("nerd font" in f.get_name().lower() for f in fm.list_families())
    return _nerd


# ---------------------------------------------------------------- preview painting

def bar_params(bar, vw=1100.0):
    """Everything the painter interpolates between two designs."""
    return {
        "vw": float(vw),
        "pos": 0.0 if bar.position == "top" else 1.0,
        "height": float(bar.height),
        "margin": float(bar.margin if bar.floating else 0),
        "radius": float(bar.radius),
        "solid_radius": float(bar.radius if bar.floating else 0),
        "spacing": float(bar.spacing),
        "padding": float(bar.padding),
        "border": float(bar.border),
        "font_size": float(bar.font_size),
        "opacity": float(bar.opacity),
        "bg": _hex(bar.background),
        "fg": _hex(bar.foreground),
        "accent": _hex(bar.accent),
        "solid": 1.0 if bar.style == "solid" else 0.0,
        "islands": 1.0 if bar.style == "islands" else 0.0,
        "pills": 1.0 if bar.style == "pills" else 0.0,
        "edge": 0.0 if bar.floating else 6.0,
        "ivin": 0.0 if bar.floating else 4.0,
    }


def lerp_params(a, b, t):
    return {k: _lerp(v, b[k], t) for k, v in a.items()}


class _Measure:
    """Pango layout set to the bar's font; widths are in virtual (unscaled) pixels."""

    def __init__(self, layout, bar, size):
        self.layout = layout
        fd = Pango.FontDescription.from_string(bar.font)
        fd.set_absolute_size(max(1.0, size) * Pango.SCALE)
        fd.set_weight({k: w for k, _l, w in WEIGHT_LABELS}[bar.font_weight])
        layout.set_font_description(fd)

    def width(self, text):
        self.layout.set_text(text, -1)
        return self.layout.get_pixel_extents()[1].width

    def height(self):
        return self.layout.get_pixel_extents()[1].height


def _ws_labels(opts):
    n = opts["persistent"] or 4
    return workspace_labels(opts["style"])[:max(3, min(10, n))] if opts["style"] != "dots" else \
        ["●" if i == 1 else "○" for i in range(max(3, min(10, n)))]


def module_items(bar, meas, P):
    """Per-zone list of (type, width, item) for the target design at params P."""
    icons = bar.icons and has_nerd_font()
    zones = {}
    for zone in barlib.ZONES:
        out = []
        for entry in bar.modules[zone]:
            mtype = entry["type"]
            opts = bar.options(mtype)
            if mtype == "workspaces":
                labels = _ws_labels(opts)
                widths = [meas.width(lbl) + 16 for lbl in labels]
                item = {"kind": "ws", "labels": labels, "widths": widths}
                w = sum(widths) + 2 * len(widths) + 8
            elif mtype == "tray":
                n, size = 3, opts["icon_size"]
                item = {"kind": "tray", "n": n, "size": size, "gap": opts["spacing"]}
                w = n * size + (n - 1) * opts["spacing"] + 2 * P["padding"]
            else:
                # nerd glyphs often come from a fallback font and sit tight: give them air
                text = _GLYPH_GAP.sub("\\1  ", sample_label(mtype, opts, icons))
                role = "accent" if mtype in ("power",) else ("dim" if mtype == "window" else "fg")
                item = {"kind": "text", "text": text, "role": role}
                w = meas.width(text) + 2 * P["padding"]
            out.append((mtype, w, item))
        zones[zone] = out
    return zones


def natural_width(bar, meas, P):
    """Width the bar needs so the centered zone clears both sides (like Waybar)."""
    zones = module_items(bar, meas, P)
    zw = {}
    for zone in barlib.ZONES:
        lst = zones[zone]
        zw[zone] = sum(w for _t, w, _i in lst) + P["spacing"] * max(0, len(lst) - 1) \
            + (2 * P["padding"] + 16 if lst else 0)
    side = max(zw["left"], zw["right"])
    return max(zw["left"] + zw["right"], 2 * side + zw["center"]) + 2 * P["margin"] + 24


def compute_layout(bar, meas, P, vh):
    """Rects in virtual pixels: bar, zone islands and each module (x, w)."""
    vw = P["vw"]
    m, h = P["margin"], P["height"]
    bx, bw = m, vw - 2 * m
    by = _lerp(m, vh - m - h, P["pos"])
    inset = P["solid"] * P["padding"] + P["islands"] * (P["edge"] + P["padding"]) + P["pills"] * 6
    mods, zones_out = {}, {}
    items = module_items(bar, meas, P)
    for zone in barlib.ZONES:
        lst = items[zone]
        if not lst:
            continue
        zw = sum(w for _t, w, _i in lst) + P["spacing"] * (len(lst) - 1)
        if zone == "left":
            x = bx + inset
        elif zone == "right":
            x = bx + bw - inset - zw
        else:
            x = bx + (bw - zw) / 2
        zones_out[zone] = (x - P["padding"], zw + 2 * P["padding"])
        for mtype, w, item in lst:
            mods[mtype] = {**item, "x": x, "w": w, "zone": zone, "alpha": 1.0}
            x += w + P["spacing"]
    return {"bar": (bx, by, bw, h), "zones": zones_out, "mods": mods}


def paint_scene(cr, w, h, bar, P, meas_layout, prev=None, t=1.0, windows=True):
    """Draw wallpaper, a couple of windows and the bar. ``prev`` is the last drawn
    layout; modules glide from there (and fade in/out) as ``t`` goes 0 -> 1."""
    s = w / P["vw"]
    vh = h / s
    meas = _Measure(meas_layout, bar, P["font_size"])
    lay = compute_layout(bar, meas, P, vh)

    # merge with the previous frame so moved modules glide and removed ones fade
    mods = {}
    for k, it in lay["mods"].items():
        old = prev["mods"].get(k) if prev else None
        if old is not None and t < 1:
            mods[k] = {**it, "x": _lerp(old["x"], it["x"], t), "w": _lerp(old["w"], it["w"], t),
                       "alpha": _lerp(old["alpha"], 1.0, t)}
        else:
            mods[k] = {**it, "alpha": t if prev and old is None else 1.0}
    if prev and t < 1:
        for k, old in prev["mods"].items():
            if k not in mods and old["alpha"] * (1 - t) > 0.01:
                mods[k] = {**old, "alpha": old["alpha"] * (1 - t)}
    zones = {}
    for z, (x, zw) in lay["zones"].items():
        old = prev["zones"].get(z) if prev else None
        if old and t < 1:
            zones[z] = (_lerp(old[0], x, t), _lerp(old[1], zw, t), _lerp(old[2], 1.0, t))
        else:
            zones[z] = (x, zw, t if prev and not old and t < 1 else 1.0)
    if prev and t < 1:
        for z, old in prev["zones"].items():
            if z not in zones and old[2] * (1 - t) > 0.01:
                zones[z] = (old[0], old[1], old[2] * (1 - t))

    cr.save()
    # wallpaper (screen space)
    import cairo
    grad = cairo.LinearGradient(0, 0, w * 0.4, h)
    for i, c in enumerate(WALL):
        grad.add_color_stop_rgb(i / (len(WALL) - 1), *c)
    cr.set_source(grad)
    cr.paint()
    glow = cairo.RadialGradient(w * 0.78, h * 0.15, 0, w * 0.78, h * 0.15, w * 0.6)
    glow.add_color_stop_rgba(0, 1, 0.96, 0.9, 0.22)
    glow.add_color_stop_rgba(1, 1, 1, 1, 0)
    cr.set_source(glow)
    cr.paint()

    cr.scale(s, s)
    bx, by, bw, bh = lay["bar"]
    m = P["margin"]

    if windows:
        gap = 14
        top = (m + bh) * (1 - P["pos"]) + gap
        bottom = vh - (m + bh) * P["pos"] - gap
        x0, x1 = gap + 6, P["vw"] - gap - 6
        split = x0 + (x1 - x0) * 0.58
        for (wx, ww), alpha in (((x0, split - x0 - gap / 2), 0.80), ((split + gap / 2, x1 - split - gap / 2), 0.62)):
            if bottom - top < 20:
                break
            _rounded(cr, wx, top, ww, bottom - top, 12)
            cr.set_source_rgba(0.98, 0.98, 0.99, alpha)
            cr.fill()
            _rounded(cr, wx, top, ww, 26, 12)
            cr.rectangle(wx, top + 12, ww, 14)
            cr.set_source_rgba(0, 0, 0, 0.05)
            cr.fill()
            for i, frac in enumerate((0.42, 0.68, 0.55, 0.3)):
                yy = top + 42 + i * 18
                if yy + 8 > bottom - 10:
                    break
                _rounded(cr, wx + 18, yy, (ww - 36) * frac, 8, 4)
                cr.set_source_rgba(0.2, 0.22, 0.26, 0.16)
                cr.fill()

    bg, fg, ac = P["bg"], P["fg"], P["accent"]
    op = P["opacity"]
    border = P["border"]

    # solid bar
    if P["solid"] > 0.01:
        _rounded(cr, bx, by, bw, bh, P["solid_radius"])
        cr.set_source_rgba(*bg, op * P["solid"])
        cr.fill()
        if border > 0.05:
            cr.set_source_rgba(*ac, 0.85 * P["solid"])
            cr.set_line_width(border)
            if m >= 1:
                _rounded(cr, bx + border / 2, by + border / 2, bw - border, bh - border,
                         max(0, P["solid_radius"] - border / 2))
                cr.stroke()
            else:
                edge_y = by + bh - border / 2 if P["pos"] < 0.5 else by + border / 2
                cr.move_to(bx, edge_y)
                cr.line_to(bx + bw, edge_y)
                cr.stroke()
    # islands
    if P["islands"] > 0.01:
        iy, ih = by + P["ivin"], bh - 2 * P["ivin"]
        for z, (x, zw, a) in zones.items():
            _rounded(cr, x, iy, zw, ih, P["radius"])
            cr.set_source_rgba(*bg, op * P["islands"] * a)
            cr.fill_preserve() if border > 0.05 else cr.fill()
            if border > 0.05:
                cr.set_source_rgba(*ac, 0.85 * P["islands"] * a)
                cr.set_line_width(border)
                cr.stroke()
    # capsules
    if P["pills"] > 0.01:
        for it in mods.values():
            _rounded(cr, it["x"], by + 4, it["w"], bh - 8, P["radius"])
            cr.set_source_rgba(*bg, op * P["pills"] * it["alpha"])
            cr.fill_preserve() if border > 0.05 else cr.fill()
            if border > 0.05:
                cr.set_source_rgba(*ac, 0.85 * P["pills"] * it["alpha"])
                cr.set_line_width(border)
                cr.stroke()

    # module contents
    th = meas.height()
    cy = by + bh / 2
    for it in mods.values():
        a = it["alpha"]
        if it["kind"] == "ws":
            x = it["x"] + 4
            # scale buttons if the width is mid-animation
            natural = sum(it["widths"]) + 2 * len(it["widths"]) + 8
            k = it["w"] / natural if natural else 1
            br = max(3.0, min(P["radius"], bh / 2) - 4)
            for i, (lbl, bwid) in enumerate(zip(it["labels"], it["widths"])):
                bw_k = bwid * k
                x += 1
                if i == 1:
                    _rounded(cr, x, by + 4, bw_k, bh - 8, br)
                    cr.set_source_rgba(*ac, 0.18 * a)
                    cr.fill()
                    cr.set_source_rgba(*ac, a)
                else:
                    cr.set_source_rgba(*fg, (0.55 if i < 3 else 0.32) * a)
                meas.layout.set_text(lbl, -1)
                tw = meas.layout.get_pixel_extents()[1].width
                cr.move_to(x + (bw_k - tw) / 2, cy - th / 2)
                PangoCairo.update_layout(cr, meas.layout)
                PangoCairo.show_layout(cr, meas.layout)
                x += bw_k + 1
            continue
        if it["kind"] == "tray":
            k = it["w"] / (it["n"] * it["size"] + (it["n"] - 1) * it["gap"] + 2 * P["padding"])
            size = it["size"] * k
            x = it["x"] + P["padding"] * k
            for i, col in enumerate(TRAY_COLORS[:it["n"]]):
                _rounded(cr, x, cy - size / 2, size, size, size * 0.3)
                cr.set_source_rgba(*col, 0.9 * a)
                cr.fill()
                x += size + it["gap"] * k
            continue
        col = ac if it["role"] == "accent" else fg
        cr.set_source_rgba(*col, a * (0.85 if it["role"] == "dim" else 1.0))
        meas.layout.set_text(it["text"], -1)
        tw = meas.layout.get_pixel_extents()[1].width
        cr.move_to(it["x"] + (it["w"] - tw) / 2, cy - th / 2)
        PangoCairo.update_layout(cr, meas.layout)
        PangoCairo.show_layout(cr, meas.layout)
    cr.restore()

    # what was actually drawn becomes the start of the next animation
    return {"mods": mods, "zones": zones}


class BarPreview(Gtk.DrawingArea):
    """Mock desktop with the bar; glides between designs instead of jumping."""

    MIN_VW, MAX_VW = 1000.0, 2100.0

    def __init__(self, bar):
        super().__init__(hexpand=True, content_height=196)
        self.add_css_class("bar-preview")
        self._bar = bar.copy()
        self._target = self._shown = self._from = bar_params(bar, self._fit_width(bar))
        self._t = 1.0
        self._prev = None
        self._last = None
        self._anim = Adw.TimedAnimation.new(self, 0.0, 1.0, 240, Adw.CallbackAnimationTarget.new(self._tick))
        self._anim.set_easing(Adw.Easing.EASE_OUT_CUBIC)
        self.set_draw_func(self._draw)

    def _fit_width(self, bar):
        layout = Pango.Layout.new(self.get_pango_context())
        P = bar_params(bar)
        need = natural_width(bar, _Measure(layout, bar, bar.font_size), P)
        return max(self.MIN_VW, min(self.MAX_VW, need + 60))

    def set_bar(self, bar, animate=True):
        target = bar_params(bar, self._fit_width(bar))
        self._bar = bar.copy()
        self._from = self._shown
        self._target = target
        self._prev = self._last
        if animate and self.get_mapped():
            self._t = 0.0
            self._anim.reset()
            self._anim.play()
        else:
            self._t = 1.0
            self._shown = target
            self.queue_draw()

    def _tick(self, t):
        self._t = t
        self._shown = lerp_params(self._from, self._target, t)
        self.queue_draw()

    def _draw(self, _area, cr, w, h):
        if w <= 0 or h <= 0:
            return
        layout = PangoCairo.create_layout(cr)
        self._last = paint_scene(cr, w, h, self._bar, self._shown, layout, self._prev, self._t)


class BarThumbnail(Gtk.DrawingArea):
    def __init__(self, bar):
        super().__init__(content_height=54, hexpand=True, can_target=False)
        self.add_css_class("bar-thumb")
        self.set_overflow(Gtk.Overflow.HIDDEN)
        self.bar = bar
        self.set_draw_func(self._draw)

    def _draw(self, _a, cr, w, h):
        if w <= 0 or h <= 0:
            return
        layout = PangoCairo.create_layout(cr)
        need = natural_width(self.bar, _Measure(layout, self.bar, self.bar.font_size), bar_params(self.bar))
        P = bar_params(self.bar, max(560.0, min(1500.0, need)))
        paint_scene(cr, w, h, self.bar, P, layout, windows=True)


# ---------------------------------------------------------------- module chips

def _summary(mtype, opts):
    spec = MODULES[mtype]
    for o in spec["options"]:
        if o["kind"] == "choice":
            for c in o["choices"]:
                if c["value"] == opts.get(o["key"]):
                    return c["label"]
    if mtype == "battery":
        return f"Warns below {opts['warning']}%"
    if mtype in ("pulseaudio", "wireplumber"):
        return f"Scroll changes by {opts['step']}%"
    if mtype in ("window", "mpris"):
        return f"Up to {opts['max_length']} characters"
    if mtype == "power":
        return f"{len(barlib.modules.power_actions(opts))} actions"
    if mtype == "keyboard_state":
        return " & ".join(n for k, n in (("capslock", "Caps"), ("numlock", "Num")) if opts[k]) or "Nothing shown"
    if mtype == "tray":
        return f"{opts['icon_size']} px icons"
    if mtype in ("cpu", "memory"):
        return f"Every {opts['interval']} s"
    if mtype == "temperature":
        return f"Hot above {opts['critical']}°C"
    return spec["description"]


class ModuleRow(Gtk.ListBoxRow):
    def __init__(self, page, mtype, zone):
        super().__init__()
        self.page, self.mtype, self.zone = page, mtype, zone
        spec = MODULES[mtype]
        self.add_css_class("module-row")
        self.update_property([Gtk.AccessibleProperty.LABEL], [spec["label"]])

        box = Gtk.Box(spacing=10, margin_start=6, margin_end=4, margin_top=6, margin_bottom=6)
        handle = Gtk.Image(icon_name="list-drag-handle-symbolic", tooltip_text="Drag to move")
        handle.add_css_class("module-handle")
        box.append(handle)
        icon = Gtk.Image(icon_name=_icon(spec["icon"]))
        icon.add_css_class("module-icon")
        box.append(icon)
        text = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, valign=Gtk.Align.CENTER, hexpand=True)
        title = Gtk.Label(label=spec["label"], xalign=0, ellipsize=Pango.EllipsizeMode.END)
        title.add_css_class("module-title")
        self.subtitle = Gtk.Label(xalign=0, ellipsize=Pango.EllipsizeMode.END)
        self.subtitle.add_css_class("module-subtitle")
        text.append(title)
        text.append(self.subtitle)
        box.append(text)

        remove = Gtk.Button(icon_name="window-close-symbolic", valign=Gtk.Align.CENTER,
                            tooltip_text=f"Remove {spec['label']}")
        remove.add_css_class("flat")
        remove.add_css_class("circular")
        remove.add_css_class("module-remove")
        remove.connect("clicked", lambda *_: page.remove(mtype))
        box.append(remove)
        box.append(self._menu())
        self.set_child(box)
        self.refresh()

        src = Gtk.DragSource(actions=Gdk.DragAction.MOVE)
        src.connect("prepare", lambda *_: Gdk.ContentProvider.new_for_value(self.mtype))
        src.connect("drag-begin", self._drag_begin)
        src.connect("drag-end", lambda *_: self.remove_css_class("dragging"))
        self.add_controller(src)

    def _drag_begin(self, src, _drag):
        src.set_icon(Gtk.WidgetPaintable.new(self), 20, 20)
        self.add_css_class("dragging")

    def _menu(self):
        menu = Gio.Menu()
        order = Gio.Menu()
        order.append("Move Up", "mod.up")
        order.append("Move Down", "mod.down")
        menu.append_section(None, order)
        zones = Gio.Menu()
        for z in barlib.ZONES:
            if z != self.zone:
                zones.append(f"Move to {barlib.ZONE_LABELS[z]}", f"mod.to-{z}")
        menu.append_section(None, zones)
        rm = Gio.Menu()
        rm.append("Remove", "mod.remove")
        menu.append_section(None, rm)
        group = Gio.SimpleActionGroup()
        acts = {"up": lambda: self.page.shift(self.mtype, -1), "down": lambda: self.page.shift(self.mtype, 1),
                "remove": lambda: self.page.remove(self.mtype)}
        for z in barlib.ZONES:
            acts[f"to-{z}"] = (lambda zz=z: self.page.move(self.mtype, zz, None))
        for name, cb in acts.items():
            act = Gio.SimpleAction.new(name, None)
            act.connect("activate", lambda *_a, f=cb: f())
            group.add_action(act)
        self.insert_action_group("mod", group)
        btn = Gtk.MenuButton(icon_name="view-more-symbolic", menu_model=menu, valign=Gtk.Align.CENTER,
                             tooltip_text="Move or remove")
        btn.add_css_class("flat")
        btn.add_css_class("circular")
        btn.add_css_class("module-menu")
        return btn

    def refresh(self):
        self.subtitle.set_label(_summary(self.mtype, self.page.bar.options(self.mtype)))


def _icon(name):
    theme = Gtk.IconTheme.get_for_display(Gdk.Display.get_default())
    return name if theme.has_icon(name) else "application-x-addon-symbolic"


class ZoneColumn(Gtk.Box):
    """One of Left / Center / Right: a list of module chips plus an Add button.
    Accepts drops anywhere in the column; a thin line shows where it will land."""

    def __init__(self, page, zone):
        super().__init__(orientation=Gtk.Orientation.VERTICAL, spacing=8, hexpand=True)
        self.page, self.zone = page, zone
        self.add_css_class("bar-zone")
        self.rows = []

        head = Gtk.Box(spacing=6, margin_start=4)
        title = Gtk.Label(label=barlib.ZONE_LABELS[zone], xalign=0)
        title.add_css_class("bar-zone-title")
        head.append(title)
        self.count = Gtk.Label()
        self.count.add_css_class("bar-zone-count")
        head.append(self.count)
        self.append(head)

        self.list = Gtk.ListBox(selection_mode=Gtk.SelectionMode.SINGLE)
        self.list.add_css_class("bar-zone-list")
        self.list.connect("row-selected", self._on_selected)
        self.append(self.list)
        self.empty = Gtk.Label(label="Drop modules here", vexpand=False)
        self.empty.add_css_class("bar-zone-empty")
        self.append(self.empty)

        self.add_btn = Gtk.MenuButton(child=Adw.ButtonContent(icon_name="list-add-symbolic", label="Add Module"),
                                      halign=Gtk.Align.START, tooltip_text=f"Add a module on the "
                                      f"{barlib.ZONE_LABELS[zone].lower()}")
        self.add_btn.add_css_class("flat")
        self.add_btn.add_css_class("bar-zone-add")
        pop = Gtk.Popover(has_arrow=False)
        pop.add_css_class("module-picker")
        pop.connect("show", self._fill_picker)
        self.add_btn.set_popover(pop)
        self.append(self.add_btn)

        tgt = Gtk.DropTarget.new(GObject.TYPE_STRING, Gdk.DragAction.MOVE)
        tgt.connect("motion", self._on_motion)
        tgt.connect("leave", lambda *_: self._mark(None, False))
        tgt.connect("drop", self._on_drop)
        self.add_controller(tgt)

    # -- rows ----------------------------------------------------------------

    def rebuild(self, fresh=None):
        self.list.remove_all()
        self.rows = []
        for entry in self.page.bar.modules[self.zone]:
            row = ModuleRow(self.page, entry["type"], self.zone)
            if entry["type"] == fresh:
                row.add_css_class("reveal-in")
            self.rows.append(row)
            self.list.append(row)
        n = len(self.rows)
        self.count.set_label(str(n) if n else "")
        self.list.set_visible(bool(n))
        self.empty.set_visible(not n)

    def select(self, mtype):
        for row in self.rows:
            if row.mtype == mtype:
                self.list.select_row(row)
                return
        self.list.unselect_all()

    def _on_selected(self, _lb, row):
        if row is not None:
            self.page.select(row.mtype)

    # -- picker --------------------------------------------------------------

    def _fill_picker(self, pop):
        lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE)
        lb.add_css_class("module-picker-list")
        used = set(self.page.bar.types())
        for mtype, spec in MODULES.items():
            row = Adw.ActionRow(title=spec["label"], subtitle=spec["description"], activatable=mtype not in used)
            row.add_prefix(Gtk.Image(icon_name=_icon(spec["icon"])))
            if mtype in used:
                row.set_sensitive(False)
                tag = Gtk.Label(label="On the bar", valign=Gtk.Align.CENTER)
                tag.add_css_class("module-used")
                row.add_suffix(tag)
            row.mtype = mtype
            lb.append(row)
        lb.connect("row-activated", lambda _lb, r: (pop.popdown(), self.page.add(r.mtype, self.zone)))
        sc = Gtk.ScrolledWindow(child=lb, hscrollbar_policy=Gtk.PolicyType.NEVER,
                                propagate_natural_height=True, max_content_height=420, min_content_width=300)
        pop.set_child(sc)

    # -- drag and drop -------------------------------------------------------

    def _index_at(self, y):
        """Insert position for a drop at column-y, plus the row/side to mark."""
        for i, row in enumerate(self.rows):
            ok, pt = row.compute_point(self, Graphene.Point().init(0, 0))
            if not ok:
                continue
            mid = pt.y + row.get_height() / 2
            if y < mid:
                return i, row, True
        return len(self.rows), (self.rows[-1] if self.rows else None), False

    def _mark(self, row, before):
        for r in self.rows:
            r.remove_css_class("drop-before")
            r.remove_css_class("drop-after")
        self.remove_css_class("drop-target")
        if row is not None:
            row.add_css_class("drop-before" if before else "drop-after")
        elif before is None:
            self.add_css_class("drop-target")

    def _on_motion(self, _tgt, _x, y):
        _i, row, before = self._index_at(y)
        self._mark(row, before if row is not None else None)
        return Gdk.DragAction.MOVE

    def _on_drop(self, _tgt, value, _x, y):
        self._mark(None, False)
        if not isinstance(value, str) or value not in MODULES:
            return False
        index, _row, _b = self._index_at(y)
        where = self.page.bar.find(value)
        if where and where[0] == self.zone and where[1] < index:
            index -= 1
        self.page.move(value, self.zone, index)
        return True


# ---------------------------------------------------------------- preset cards

class BarPresetCard(Gtk.FlowBoxChild):
    def __init__(self, preset):
        super().__init__()
        self.preset = preset
        self.set_tooltip_text(preset.description)
        self.update_property([Gtk.AccessibleProperty.LABEL], [f"{preset.name}. {preset.description}"])
        card = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, width_request=170)
        card.add_css_class("bar-preset-card")
        self.card = card
        card.append(BarThumbnail(preset.bar))
        name = Gtk.Label(label=preset.name, xalign=0, ellipsize=Pango.EllipsizeMode.END,
                         margin_start=12, margin_end=12, margin_top=8, margin_bottom=10)
        name.add_css_class("bar-preset-name")
        card.append(name)
        self.set_child(card)

    def set_current(self, on):
        (self.card.add_css_class if on else self.card.remove_css_class)("current")


# ---------------------------------------------------------------- the page

class BarPage(Adw.Bin):
    def __init__(self, cm, toast_overlay, launcher=None):
        super().__init__()
        load_css()
        self.cm = cm
        self.toast_overlay = toast_overlay
        self.selected = None
        self._updating = False
        self._save_id = 0
        self._poll_id = 0
        self._others_seen = []
        self._autostart = None
        self.proc = barlib.BarProcess(barlib.data_dir(cm), launcher=launcher)
        try:
            self.presets = barlib.load_presets()
        except barlib.BarError as e:
            self.presets = []
            GLib.idle_add(lambda: toast(self.toast_overlay, f"Couldn't load bar presets: {e}") and False)
        saved = barlib.load_current(cm)
        self.bar = saved or (self.presets[0].bar.copy() if self.presets else barlib.Bar())

        root = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        self.banner = Adw.Banner(button_label="Why?")
        self.banner.connect("button-clicked", lambda *_: self._explain_overlap())
        root.append(self.banner)
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=22,
                        margin_top=24, margin_bottom=72, margin_start=18, margin_end=18)
        clamp = Adw.Clamp(maximum_size=1080, tightening_threshold=860, child=outer)
        scroller = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER, child=clamp, vexpand=True)
        root.append(scroller)
        self.set_child(root)

        outer.append(self._header())

        # live preview
        self.preview = BarPreview(self.bar)
        frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        frame.add_css_class("bar-preview-frame")
        frame.set_overflow(Gtk.Overflow.HIDDEN)
        pov = Gtk.Overlay(child=self.preview)
        badge = Gtk.Label(label="Live preview", halign=Gtk.Align.START,
                          valign=Gtk.Align.END, can_target=False)
        badge.add_css_class("preview-badge")
        self.badge = badge
        pov.add_overlay(badge)
        frame.append(pov)
        outer.append(frame)

        # presets
        outer.append(self._section("Start From a Preset", "Pick one, then make it yours below."))
        self.preset_grid = Gtk.FlowBox(selection_mode=Gtk.SelectionMode.NONE, homogeneous=True,
                                       min_children_per_line=2, max_children_per_line=5, column_spacing=14,
                                       row_spacing=14, activate_on_single_click=True, valign=Gtk.Align.START)
        self.preset_grid.add_css_class("bar-preset-grid")
        self.preset_cards = []
        for p in self.presets:
            card = BarPresetCard(p)
            self.preset_cards.append(card)
            self.preset_grid.append(card)
        self.preset_grid.connect("child-activated", lambda _fb, c: self.use_preset(c.preset))
        outer.append(self.preset_grid)

        # modules
        outer.append(self._section("Modules", "Drag modules to reorder them or move them between sides. "
                                              "Click one to change its options."))
        zones_box = Gtk.Box(spacing=12, homogeneous=True)
        zones_box.add_css_class("bar-zones")
        self.columns = {z: ZoneColumn(self, z) for z in barlib.ZONES}
        for z in barlib.ZONES:
            zones_box.append(self.columns[z])
        outer.append(zones_box)

        self.options_stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=150,
                                       vhomogeneous=False, interpolate_size=True)
        hint = Gtk.Label(label="Select a module above to change its options.", xalign=0)
        hint.add_css_class("dim-label")
        hint.add_css_class("bar-options-hint")
        self.options_stack.add_named(hint, "none")
        outer.append(self.options_stack)

        outer.append(self._layout_group())
        outer.append(self._appearance_group())
        outer.append(self._files_group())

        self._rebuild_zones()
        self._load_controls()
        self._sync_badge()
        self._sync_presets()
        self.connect("map", self._on_map)
        self.connect("unmap", self._on_unmap)

    # -- building blocks -----------------------------------------------------

    def _section(self, title, subtitle):
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=2, margin_top=6)
        t = Gtk.Label(label=title, xalign=0)
        t.add_css_class("bar-section-title")
        s = Gtk.Label(label=subtitle, xalign=0, wrap=True)
        s.add_css_class("dim-label")
        s.add_css_class("bar-section-sub")
        box.append(t)
        box.append(s)
        return box

    def _header(self):
        head = Gtk.Box(spacing=12)
        texts = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4, hexpand=True)
        title = Gtk.Label(label="Bar", xalign=0)
        title.add_css_class("title-1")
        sub = Gtk.Label(label="Design your Waybar by clicking — no config files. "
                              "Your own Waybar setup is never changed.", xalign=0, wrap=True)
        sub.add_css_class("dim-label")
        texts.append(title)
        texts.append(sub)
        self.status = Gtk.Label(xalign=0, halign=Gtk.Align.START, margin_top=4)
        self.status.add_css_class("bar-status")
        texts.append(self.status)
        head.append(texts)

        actions = Gtk.Box(spacing=8, valign=Gtk.Align.CENTER)
        self.stop_btn = Gtk.Button(label="Stop Bar", tooltip_text="Stop the bar this app started")
        self.stop_btn.add_css_class("pill")
        self.stop_btn.connect("clicked", lambda *_: self.stop_bar())
        self.use_btn = Gtk.Button(label="Use This Bar",
                                  tooltip_text="Save the Waybar files and start (or restart) the bar")
        self.use_btn.add_css_class("pill")
        self.use_btn.add_css_class("suggested-action")
        self.use_btn.connect("clicked", lambda *_: self.use_bar())
        actions.append(self.stop_btn)
        actions.append(self.use_btn)
        head.append(actions)
        return head

    @staticmethod
    def _spin_row(lo, hi):
        row = Adw.SpinRow()
        row.set_adjustment(Gtk.Adjustment(lower=lo, upper=hi, step_increment=1, page_increment=5))
        row.set_digits(0)
        row.set_climb_rate(1.0)
        return row

    def _spin(self, title, lo, hi, field, unit, subtitle=None):
        row = self._spin_row(lo, hi)
        row.set_title(title)
        if subtitle:
            row.set_subtitle(subtitle)
        row.connect("output", lambda r: (r.set_text(f"{int(r.get_value())} {unit}"), True)[1])
        row.connect("input", self._spin_input)
        row.connect("notify::value", lambda r, _p: self._set(field, int(r.get_value())))
        return row

    @staticmethod
    def _spin_input(row):
        txt = "".join(ch for ch in row.get_text() if ch.isdigit() or ch in ".-")
        try:
            return True, float(txt)
        except ValueError:
            return False, 0.0

    def _toggle_group(self, items, field):
        tg = Adw.ToggleGroup(valign=Gtk.Align.CENTER)
        tg.add_css_class("bar-toggle")
        for name, label in items:
            tg.add(Adw.Toggle(name=name, label=label))
        tg.connect("notify::active-name", lambda g, _p: g.get_active_name() and self._set(field, g.get_active_name()))
        return tg

    def _layout_group(self):
        g = Adw.PreferencesGroup(title="Layout")
        self.c = {}
        row = Adw.ActionRow(title="Position")
        self.c["position"] = self._toggle_group([("top", "Top"), ("bottom", "Bottom")], "position")
        row.add_suffix(self.c["position"])
        g.add(row)
        row = Adw.ActionRow(title="Style", subtitle="One solid bar, an island per side, or a capsule per module")
        self.c["style"] = self._toggle_group([(k, v) for k, v in STYLE_LABELS.items()], "style")
        row.add_suffix(self.c["style"])
        g.add(row)
        self.c["floating"] = Adw.SwitchRow(title="Floating", subtitle="Leave a gap between the bar and the screen edges")
        self.c["floating"].connect("notify::active", lambda r, _p: self._set("floating", r.get_active()))
        g.add(self.c["floating"])
        self.c["margin"] = self._spin("Gap From Edges", 0, 48, "margin", "px")
        g.add(self.c["margin"])
        self.c["height"] = self._spin("Height", 20, 72, "height", "px")
        g.add(self.c["height"])
        self.c["radius"] = self._spin("Corner Radius", 0, 36, "radius", "px")
        g.add(self.c["radius"])
        self.c["spacing"] = self._spin("Module Spacing", 0, 32, "spacing", "px", "Space between modules")
        g.add(self.c["spacing"])
        self.c["padding"] = self._spin("Module Padding", 0, 32, "padding", "px", "Space inside each module")
        g.add(self.c["padding"])
        self.c["border"] = self._spin("Outline", 0, 4, "border", "px", "A thin line in the accent color")
        g.add(self.c["border"])
        return g

    def _color_row(self, title, field, subtitle=None):
        row = Adw.ActionRow(title=title, subtitle=subtitle or "")
        btn = Gtk.ColorDialogButton(dialog=Gtk.ColorDialog(with_alpha=False, title=title), valign=Gtk.Align.CENTER)
        btn.add_css_class("color-swatch")
        btn.connect("notify::rgba", lambda b, _p: self._set(field, _to_hex(b.get_rgba())))
        row.add_suffix(btn)
        row.set_activatable_widget(btn)
        self.c[field] = btn
        return row

    def _appearance_group(self):
        g = Adw.PreferencesGroup(title="Appearance")
        row = Adw.ActionRow(title="Font")
        self.c["font"] = Gtk.FontDialogButton(dialog=Gtk.FontDialog(title="Bar Font"), level=Gtk.FontLevel.FAMILY,
                                              valign=Gtk.Align.CENTER, use_font=True)
        self.c["font"].connect("notify::font-desc", self._on_font)
        row.add_suffix(self.c["font"])
        row.set_activatable_widget(self.c["font"])
        g.add(row)
        self.c["font_size"] = self._spin("Font Size", 8, 28, "font_size", "px")
        g.add(self.c["font_size"])
        self.c["font_weight"] = Adw.ComboRow(title="Font Weight",
                                             model=Gtk.StringList.new([lbl for _k, lbl, _w in WEIGHT_LABELS]))
        self.c["font_weight"].connect("notify::selected", lambda r, _p: self._set(
            "font_weight", WEIGHT_LABELS[r.get_selected()][0]) if r.get_selected() < len(WEIGHT_LABELS) else None)
        g.add(self.c["font_weight"])
        g.add(self._color_row("Background", "background"))
        g.add(self._color_row("Text", "foreground"))
        g.add(self._color_row("Accent", "accent", "Active workspace, highlights and outline"))
        row = Adw.ActionRow(title="Background Opacity")
        self.c["opacity"] = Gtk.Scale.new_with_range(Gtk.Orientation.HORIZONTAL, 0, 100, 1)
        self.c["opacity"].set_size_request(200, -1)
        self.c["opacity"].set_valign(Gtk.Align.CENTER)
        self.c["opacity"].set_draw_value(True)
        self.c["opacity"].set_value_pos(Gtk.PositionType.LEFT)
        self.c["opacity"].set_format_value_func(lambda _s, v: f"{int(v)}%")
        self.c["opacity"].connect("value-changed", lambda s: self._set("opacity", round(s.get_value() / 100, 2)))
        row.add_suffix(self.c["opacity"])
        g.add(row)
        self.c["icons"] = Adw.SwitchRow(title="Icons", subtitle="Symbols next to values (needs a Nerd Font)"
                                        if not has_nerd_font() else "Symbols next to values")
        self.c["icons"].connect("notify::active", lambda r, _p: self._set("icons", r.get_active()))
        g.add(self.c["icons"])
        return g

    def _files_group(self):
        g = Adw.PreferencesGroup(title="Startup &amp; Sharing")
        box = Gtk.Box(spacing=6)
        imp = Gtk.Button(child=Adw.ButtonContent(icon_name="document-open-symbolic", label="Import…"),
                         tooltip_text="Open a bar someone shared with you (.hyprmod-bar)")
        imp.add_css_class("flat")
        imp.connect("clicked", lambda *_: self.import_dialog())
        exp = Gtk.Button(child=Adw.ButtonContent(icon_name="document-send-symbolic", label="Export…"),
                         tooltip_text="Save this bar as a file you can share")
        exp.add_css_class("flat")
        exp.connect("clicked", lambda *_: self.export_dialog())
        box.append(imp)
        box.append(exp)
        g.set_header_suffix(box)

        self.autostart_row = Adw.SwitchRow(title="Start Bar on Login",
                                           subtitle="Hyprland starts this bar every time you log in",
                                           visible=False)
        self.autostart_row.connect("notify::active", self._on_autostart)
        g.add(self.autostart_row)
        folder = str(barlib.waybar_dir())
        home = str(Path.home())
        shown = "~" + folder[len(home):] if folder.startswith(home + "/") else folder
        files = Adw.ActionRow(title="Bar Files", subtitle=GLib.markup_escape_text(shown))
        files.add_css_class("bar-files-row")
        files.set_subtitle_selectable(True)
        copy = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER,
                          tooltip_text="Copy the command that starts this bar")
        copy.add_css_class("flat")
        copy.connect("clicked", lambda *_: self._copy(barlib.autostart_command(), "Start command copied"))
        files.add_suffix(copy)
        g.add(files)
        return g

    # -- state <-> controls --------------------------------------------------

    def _load_controls(self):
        self._updating = True
        try:
            b, c = self.bar, self.c
            c["position"].set_active_name(b.position)
            c["style"].set_active_name(b.style)
            c["floating"].set_active(b.floating)
            for f in ("margin", "height", "radius", "spacing", "padding", "border", "font_size"):
                c[f].set_value(getattr(b, f))
            c["font"].set_font_desc(Pango.FontDescription.from_string(b.font))
            c["font_weight"].set_selected([k for k, _l, _w in WEIGHT_LABELS].index(b.font_weight))
            for f in ("background", "foreground", "accent"):
                c[f].set_rgba(_rgba(getattr(b, f)))
            c["opacity"].set_value(round(b.opacity * 100))
            c["icons"].set_active(b.icons)
        finally:
            self._updating = False
        self._sync_sensitivity()

    def _sync_sensitivity(self):
        b = self.bar
        self.c["margin"].set_visible(b.floating)
        self.c["radius"].set_sensitive(b.floating or b.style != "solid")
        self.c["radius"].set_subtitle("" if (b.floating or b.style != "solid")
                                      else "Turn on Floating to round a solid bar")

    def _set(self, field, value):
        if self._updating:
            return
        if getattr(self.bar, field) == value:
            return
        try:
            self.bar.set(field, value)
        except barlib.BarError as e:
            toast(self.toast_overlay, str(e), 4)
            self._load_controls()
            return
        self._sync_sensitivity()
        self.changed()

    def _on_font(self, btn, _p):
        desc = btn.get_font_desc()
        family = desc.get_family() if desc else None
        if family:
            self._set("font", family)

    def changed(self, structure=False, fresh=None):
        """The design changed: preview glides, chips update, autosave soon."""
        if structure:
            self._rebuild_zones(fresh)
        else:
            for col in self.columns.values():
                for row in col.rows:
                    row.refresh()
        self.preview.set_bar(self.bar)
        self._sync_badge()
        self._sync_presets()
        self._update_status()
        if self._save_id:
            GLib.source_remove(self._save_id)
        self._save_id = GLib.timeout_add(SAVE_DELAY_MS, self._save_now)

    def _sync_badge(self):
        self.badge.set_valign(Gtk.Align.START if self.bar.position == "bottom" else Gtk.Align.END)

    def _save_now(self):
        self._save_id = 0
        try:
            barlib.save_current(self.bar, self.cm)
        except OSError as e:
            toast(self.toast_overlay, f"Couldn't save the bar design: {e.strerror or e}", 5)
        return GLib.SOURCE_REMOVE

    def _sync_presets(self):
        for card in self.preset_cards:
            card.set_current(card.preset.bar == self.bar)

    def _rebuild_zones(self, fresh=None):
        for col in self.columns.values():
            col.rebuild(fresh)
        if self.selected and not self.bar.find(self.selected):
            self.selected = None
        self.select(self.selected)

    # -- module editing (called by chips and columns) ------------------------

    def select(self, mtype):
        if self._updating:
            return
        self._updating = True
        try:
            for col in self.columns.values():
                col.select(mtype)
        finally:
            self._updating = False
        changed = mtype != self.selected or self.options_stack.get_child_by_name("opts") is None
        self.selected = mtype
        if mtype is None:
            self.options_stack.set_visible_child_name("none")
        elif changed:
            self._show_options(mtype)

    def _show_options(self, mtype):
        old = self.options_stack.get_child_by_name("opts")
        spec = MODULES[mtype]
        g = Adw.PreferencesGroup(title=f"{spec['label']} Options", description=spec["description"])
        rm = Gtk.Button(child=Adw.ButtonContent(icon_name="user-trash-symbolic", label="Remove"),
                        tooltip_text=f"Remove {spec['label']} from the bar")
        rm.add_css_class("flat")
        rm.connect("clicked", lambda *_: self.remove(mtype))
        g.set_header_suffix(rm)
        opts = self.bar.options(mtype)
        if not spec["options"]:
            row = Adw.ActionRow(title="Nothing to set", subtitle="This module works as it is.")
            row.add_css_class("empty-row")
            g.add(row)
        for o in spec["options"]:
            g.add(self._option_row(mtype, o, opts[o["key"]]))
        if old is not None:
            self.options_stack.remove(old)
        self.options_stack.add_named(g, "opts")
        self.options_stack.set_visible_child(g)

    def _option_row(self, mtype, o, value):
        commit = (lambda v, k=o["key"]: self.set_option(mtype, k, v))
        if o["kind"] == "bool":
            row = Adw.SwitchRow(title=o["label"], active=value)
            row.connect("notify::active", lambda r, _p: commit(r.get_active()))
        elif o["kind"] == "int":
            row = self._spin_row(o["min"], o["max"])
            row.set_title(o["label"])
            row.set_value(value)
            unit = o.get("unit", "")
            row.connect("output", lambda r: (r.set_text(f"{int(r.get_value())}"
                                                        f"{'' if unit in ('%', '°C') else ' '}{unit}"), True)[1])
            row.connect("input", self._spin_input)
            row.connect("notify::value", lambda r, _p: commit(int(r.get_value())))
        else:
            values = [c["value"] for c in o["choices"]]
            row = Adw.ComboRow(title=o["label"], model=Gtk.StringList.new([c["label"] for c in o["choices"]]))
            row.set_selected(values.index(value) if value in values else 0)
            row.connect("notify::selected", lambda r, _p: commit(values[r.get_selected()])
                        if r.get_selected() < len(values) else None)
        if o.get("description"):
            row.set_subtitle(o["description"])
        return row

    def set_option(self, mtype, key, value):
        if self.bar.options(mtype).get(key) == value:
            return
        try:
            self.bar.set_option(mtype, key, value)
        except barlib.BarError as e:
            toast(self.toast_overlay, str(e), 4)
            self._show_options(mtype)
            return
        self.changed()

    def add(self, mtype, zone):
        try:
            self.bar.add_module(mtype, zone)
        except barlib.BarError as e:
            toast(self.toast_overlay, str(e), 3)
            return
        self.selected = mtype
        self.changed(structure=True, fresh=mtype)
        self._show_options(mtype)

    def remove(self, mtype):
        before = self.bar.copy()
        try:
            self.bar.remove_module(mtype)
        except barlib.BarError:
            return
        self.changed(structure=True)
        toast(self.toast_overlay, f"Removed {MODULES[mtype]['label']}", 3, "Undo", lambda: self._restore(before))

    def move(self, mtype, zone, index):
        where = self.bar.find(mtype)
        if where is None:
            return
        if index is None:
            index = len(self.bar.modules[zone])
        if where == (zone, index):
            return
        self.bar.move_module(mtype, zone, index)
        self.selected = mtype
        self.changed(structure=True)

    def shift(self, mtype, delta):
        if self.bar.shift_module(mtype, delta):
            self.selected = mtype
            self.changed(structure=True)

    def _restore(self, bar):
        self.bar = bar.copy()
        self._load_controls()
        self.changed(structure=True)

    def use_preset(self, preset):
        if preset.bar == self.bar:
            return
        before = self.bar.copy()
        self.bar = preset.bar.copy()
        self.selected = None
        self._load_controls()
        self.changed(structure=True)
        toast(self.toast_overlay, f"Loaded “{preset.name}” — tweak it, then Use This Bar", 4,
              "Undo", lambda: self._restore(before))

    # -- running the bar -----------------------------------------------------

    def _update_status(self):
        running = self.proc.is_running()
        self.stop_btn.set_visible(running)
        current = barlib.files_match(self.bar)
        if running and current:
            text, cls = "Running", "running"
        elif running:
            text, cls = "Running — press Update Bar to show your changes", "pending"
        else:
            text, cls = "Not running", "stopped"
        self.status.set_label(text)
        for c in ("running", "pending", "stopped"):
            self.status.remove_css_class(c)
        self.status.add_css_class(cls)
        self.use_btn.set_label("Update Bar" if running else "Use This Bar")
        self.use_btn.set_sensitive(not (running and current))

    def _check_others(self):
        others = self.proc.other_bars()
        if others:
            names = " and ".join(others) if len(others) < 3 else ", ".join(others)
            verb = "is" if len(others) == 1 else "are"
            self.banner.set_title(GLib.markup_escape_text(
                f"{names} {verb} also running — two bars at once will overlap"))
        self.banner.set_revealed(bool(others))
        self._others_seen = others
        return others

    def _poll(self):
        self._update_status()
        self._check_others()
        return GLib.SOURCE_CONTINUE

    def _on_map(self, *_):
        self.preview.set_bar(self.bar, animate=False)
        self._load_autostart()
        self._poll()
        if not self._poll_id:
            self._poll_id = GLib.timeout_add_seconds(POLL_S, self._poll)

    def _on_unmap(self, *_):
        if self._poll_id:
            GLib.source_remove(self._poll_id)
            self._poll_id = 0
        if self._save_id:
            GLib.source_remove(self._save_id)
            self._save_now()

    def use_bar(self):
        try:
            barlib.write_files(self.bar)
            barlib.save_current(self.bar, self.cm)
        except (OSError, barlib.BarError) as e:
            toast(self.toast_overlay, f"Couldn't write the bar files: {getattr(e, 'strerror', None) or e}", 5)
            return
        if barlib.is_dry_run():
            toast(self.toast_overlay, "Dry run — bar files saved, Waybar wasn't started", 4)
            self._update_status()
            return
        if not self.proc.installed():
            self._install_dialog()
            return
        others = self._check_others()
        if others and not self.proc.is_running():
            self._overlap_dialog(others)
            return
        self._start()

    def _start(self):
        try:
            self.proc.start()
        except barlib.WaybarMissing:
            self._install_dialog()
            return
        except OSError as e:
            toast(self.toast_overlay, f"Couldn't start Waybar: {e.strerror or e}", 5)
            return
        toast(self.toast_overlay, "Your bar is running", 3)
        GLib.timeout_add(600, lambda: (self._update_status(), False)[1])
        self._update_status()

    def stop_bar(self):
        stopped = self.proc.stop()
        toast(self.toast_overlay, "Bar stopped" if stopped else "The bar wasn't running", 2)
        self._update_status()

    def _overlap_dialog(self, others):
        dlg = Adw.AlertDialog(heading="Another Bar Is Running",
                              body=f"{' and '.join(others)} is already on screen. Two bars at once will overlap "
                                   "and both take space at the screen edge.\n\nStop the other bar first, or "
                                   "start this one anyway.")
        dlg.add_response("cancel", "Cancel")
        dlg.add_response("start", "Start Anyway")
        dlg.set_response_appearance("start", Adw.ResponseAppearance.SUGGESTED)
        dlg.set_default_response("cancel")
        dlg.set_close_response("cancel")
        dlg.connect("response", lambda _d, r: r == "start" and self._start())
        dlg.present(self.get_root())

    def _explain_overlap(self):
        dlg = Adw.AlertDialog(heading="Two Bars Overlap",
                              body="Each bar reserves its own strip at the screen edge, so running two shows "
                                   "both, stacked or on top of each other. Stop the other bar (or remove it from "
                                   "your autostart) before using this one. Hyprland Mod never stops a bar it "
                                   "didn't start.")
        dlg.add_response("ok", "OK")
        dlg.present(self.get_root())

    def _install_dialog(self):
        dlg = Adw.AlertDialog(heading="Waybar Isn't Installed",
                              body="Your bar is saved. Install Waybar with the command for your system, "
                                   "then press Use This Bar again.")
        lb = Gtk.ListBox(selection_mode=Gtk.SelectionMode.NONE, margin_top=6)
        lb.add_css_class("boxed-list")
        for distro, cmd in barlib.INSTALL_HINTS:
            row = Adw.ActionRow(title=GLib.markup_escape_text(distro), subtitle=GLib.markup_escape_text(cmd))
            row.add_css_class("install-row")
            btn = Gtk.Button(icon_name="edit-copy-symbolic", valign=Gtk.Align.CENTER, tooltip_text="Copy")
            btn.add_css_class("flat")
            btn.connect("clicked", lambda b, c=cmd: (self._copy(c), b.set_icon_name("object-select-symbolic")))
            row.add_suffix(btn)
            lb.append(row)
        dlg.set_extra_child(lb)
        dlg.add_response("ok", "OK")
        dlg.present(self.get_root())

    def _copy(self, text, message=None):
        self.get_clipboard().set(text)
        if message:
            toast(self.toast_overlay, message, 2)

    # -- login autostart (wired up by the window, see set_autostart_handler) --

    def set_autostart_handler(self, get_enabled, set_enabled):
        """``get_enabled() -> bool``; ``set_enabled(on: bool, command: str)`` may raise
        to refuse. ``command`` is ``hyprland_mod.bar.autostart_command()``."""
        self._autostart = (get_enabled, set_enabled)
        self.autostart_row.set_visible(True)
        self._load_autostart()

    def _load_autostart(self):
        if not self._autostart:
            return
        self._updating = True
        try:
            self.autostart_row.set_active(bool(self._autostart[0]()))
        except Exception:
            pass
        finally:
            self._updating = False

    def _on_autostart(self, row, _p):
        if self._updating or not self._autostart:
            return
        on = row.get_active()
        try:
            if on:
                barlib.write_files(self.bar)
            self._autostart[1](on, barlib.autostart_command())
        except Exception as e:
            toast(self.toast_overlay, f"Couldn't change login startup: {e}", 5)
            self._load_autostart()
            return
        toast(self.toast_overlay, "The bar will start when you log in" if on
              else "The bar won't start on login anymore", 3)

    # -- sharing -------------------------------------------------------------

    def export_dialog(self):
        dlg = Gtk.FileDialog(title="Export Bar", initial_name=f"my-bar{barlib.BAR_EXT}",
                             filters=filter_list(file_filter("Hyprland Mod bars", ["*" + barlib.BAR_EXT])))

        def done(d, res):
            try:
                f = d.save_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't export: {e.message}", 5)
                return
            path = f.get_path()
            name = Path(path).name
            if name.endswith(barlib.BAR_EXT):
                name = name[:-len(barlib.BAR_EXT)]
            try:
                barlib.export_bar(self.bar, path, name.replace("-", " ").strip().title() or "My Bar")
            except (OSError, barlib.BarError) as e:
                toast(self.toast_overlay, f"Couldn't export: {getattr(e, 'strerror', None) or e}", 5)
                return
            toast(self.toast_overlay, "Exported — share the file with anyone")
        dlg.save(self.get_root(), None, done)

    def import_dialog(self):
        dlg = Gtk.FileDialog(title="Import Bar", filters=filter_list(
            file_filter("Hyprland Mod bars", ["*" + barlib.BAR_EXT, "*.json"]), file_filter("All files", ["*"])))

        def done(d, res):
            try:
                f = d.open_finish(res)
            except GLib.Error as e:
                if not dismissed(e):
                    toast(self.toast_overlay, f"Couldn't open the file: {e.message}", 5)
                return
            self.import_file(f.get_path())
        dlg.open(self.get_root(), None, done)

    def import_file(self, path):
        try:
            preset = barlib.import_bar(path)
        except barlib.BarError as e:
            dlg = Adw.AlertDialog(heading="Couldn't Import Bar", body=str(e))
            dlg.add_response("ok", "OK")
            dlg.present(self.get_root())
            return None
        before = self.bar.copy()
        self.bar = preset.bar.copy()
        self.selected = None
        self._load_controls()
        self.changed(structure=True)
        toast(self.toast_overlay, f"Opened “{preset.name}”", 4, "Undo", lambda: self._restore(before))
        return preset

    # -- dev helpers (screenshots) -------------------------------------------

    def dev_select(self, mtype):
        self.select(mtype)
