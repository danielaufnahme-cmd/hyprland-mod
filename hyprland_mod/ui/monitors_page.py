"""Displays: arrange monitors and pick resolution, scale and rotation (applied with auto-revert)."""

import math
import re
import time

import gi

gi.require_version("PangoCairo", "1.0")
from gi.repository import Adw, Gdk, GLib, Gtk, Pango, PangoCairo  # noqa: E402

REVERT_SECONDS = 15
SCALES = [1.0, 1.25, 1.5, 1.6, 1.75, 2.0]
ROTATIONS = [(0, "Normal"), (1, "90° Clockwise"), (2, "Upside Down"), (3, "90° Counter-clockwise")]
SNAP_PX = 14  # on-screen snapping distance while dragging
_MODE_RE = re.compile(r"^(\d+)x(\d+)(?:@([\d.]+))?")
ACCENT = (0x35 / 255, 0x84 / 255, 0xe4 / 255)


def _parse_mode(text):
    m = _MODE_RE.match(str(text or ""))
    if not m:
        return None
    return int(m.group(1)), int(m.group(2)), m.group(3)


def _fmt_rate(rate):
    try:
        return f"{float(rate):.2f}"
    except (TypeError, ValueError):
        return None


def _fmt_scale(s):
    return f"{s:g}"


def _rounded(cr, x, y, w, h, r):
    r = max(0.0, min(r, w / 2, h / 2))
    cr.new_sub_path()
    cr.arc(x + w - r, y + r, r, -math.pi / 2, 0)
    cr.arc(x + w - r, y + h - r, r, 0, math.pi / 2)
    cr.arc(x + r, y + h - r, r, math.pi / 2, math.pi)
    cr.arc(x + r, y + r, r, math.pi, 3 * math.pi / 2)
    cr.close_path()


class Display:
    """Working copy of one monitor's settings."""

    def __init__(self, info, index):
        self.index = index
        self.name = info["name"]
        self.label = (info.get("description") or "").strip() or self.name
        self.modes = []
        for text in info.get("availableModes") or []:
            p = _parse_mode(text)
            if p and p not in self.modes:
                self.modes.append(p)
        ov = info.get("override") or {}
        mode = _parse_mode(ov.get("mode")) if ov else None
        self.width = mode[0] if mode else int(info.get("width") or 1920)
        self.height = mode[1] if mode else int(info.get("height") or 1080)
        self.rate = (mode[2] if mode and mode[2] else None) or _fmt_rate(info.get("refreshRate"))
        pos = re.match(r"^(-?\d+)x(-?\d+)$", str(ov.get("position") or "")) if ov else None
        self.x = int(pos.group(1)) if pos else int(info.get("x") or 0)
        self.y = int(pos.group(2)) if pos else int(info.get("y") or 0)
        try:
            self.scale = float(ov["scale"]) if ov and ov.get("scale") not in (None, "auto") \
                else float(info.get("scale") or 1)
        except ValueError:
            self.scale = 1.0
        self.transform = int((ov.get("transform") if ov else info.get("transform")) or 0)
        self.enabled = bool(ov.get("enabled", True)) if ov else not info.get("disabled")
        if not any(m[:2] == (self.width, self.height) for m in self.modes):
            self.modes.insert(0, (self.width, self.height, self.rate))

    def state(self):
        return (self.width, self.height, self.rate, self.x, self.y, self.scale, self.transform, self.enabled)

    @property
    def size(self):
        """Logical size on the desktop (after scale and rotation)."""
        w, h = self.width / self.scale, self.height / self.scale
        if self.transform % 2:
            w, h = h, w
        return round(w), round(h)

    def spec(self):
        mode = f"{self.width}x{self.height}" + (f"@{self.rate}" if self.rate else "")
        return {"mode": mode, "position": f"{self.x}x{self.y}", "scale": _fmt_scale(self.scale),
                "transform": self.transform, "enabled": self.enabled}


def _overlap(a, b):
    """Logical rects ``(x, y, w, h)`` share some area."""
    return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]


def _touch_positions(d, others, want_x, want_y, snap):
    """Positions where ``d`` sits flush against a neighbour without overlapping anyone."""
    w, h = d.size
    out = []
    for o in others:
        ow, oh = o.size
        sides = [(o.x - w, None), (o.x + ow, None), (None, o.y - h), (None, o.y + oh)]
        for fx, fy in sides:
            if fx is not None:
                y = min(max(want_y, o.y - h + 1), o.y + oh - 1)
                for edge in (o.y, o.y + oh - h, o.y + (oh - h) / 2):
                    if abs(y - edge) <= snap:
                        y = edge
                        break
                cand = (fx, round(y))
            else:
                x = min(max(want_x, o.x - w + 1), o.x + ow - 1)
                for edge in (o.x, o.x + ow - w, o.x + (ow - w) / 2):
                    if abs(x - edge) <= snap:
                        x = edge
                        break
                cand = (round(x), fy)
            rect = (cand[0], cand[1], w, h)
            if not any(_overlap(rect, (p.x, p.y, *p.size)) for p in others):
                out.append(cand)
    return out


class ArrangementCanvas(Gtk.DrawingArea):
    """Monitors as rounded tiles at their real proportions; drag to rearrange."""

    def __init__(self, page):
        super().__init__(content_height=280, hexpand=True)
        self.add_css_class("monitor-canvas")
        self.page = page
        self._drag = None  # (display, start_x, start_y)
        self._view = None  # frozen (scale, ox, oy) while dragging
        self.set_draw_func(self._draw)
        click = Gtk.GestureClick()
        click.connect("pressed", self._on_click)
        self.add_controller(click)
        drag = Gtk.GestureDrag()
        drag.connect("drag-begin", self._on_drag_begin)
        drag.connect("drag-update", self._on_drag_update)
        drag.connect("drag-end", self._on_drag_end)
        self.add_controller(drag)
        motion = Gtk.EventControllerMotion()
        motion.connect("motion", self._on_motion)
        self.add_controller(motion)

    # -- geometry --------------------------------------------------------

    def _transform(self):
        if self._view is not None:
            return self._view
        ds = self.page.displays
        w, h = self.get_width(), self.get_height()
        if not ds or w <= 0 or h <= 0:
            return 1.0, 0.0, 0.0
        x0 = min(d.x for d in ds)
        y0 = min(d.y for d in ds)
        x1 = max(d.x + d.size[0] for d in ds)
        y1 = max(d.y + d.size[1] for d in ds)
        pad = 36
        s = min((w - 2 * pad) / max(1, x1 - x0), (h - 2 * pad) / max(1, y1 - y0))
        ox = (w - (x1 - x0) * s) / 2 - x0 * s
        oy = (h - (y1 - y0) * s) / 2 - y0 * s
        return s, ox, oy

    def _rect(self, d):
        s, ox, oy = self._transform()
        w, h = d.size
        return ox + d.x * s, oy + d.y * s, w * s, h * s

    def _hit(self, x, y):
        for d in reversed(self.page.displays):
            rx, ry, rw, rh = self._rect(d)
            if rx <= x <= rx + rw and ry <= y <= ry + rh:
                return d
        return None

    # -- input -----------------------------------------------------------

    def _on_motion(self, _ctrl, x, y):
        if self._drag is None:
            self.set_cursor_from_name("grab" if self._hit(x, y) and len(self.page.displays) > 1 else None)

    def _on_click(self, _g, _n, x, y):
        d = self._hit(x, y)
        if d is not None:
            self.page.select(d)

    def _on_drag_begin(self, _g, x, y):
        d = self._hit(x, y)
        if d is None or len(self.page.displays) < 2:
            self._drag = None
            return
        self._view = self._transform()
        self._drag = (d, d.x, d.y)
        self.set_cursor_from_name("grabbing")
        self.page.select(d)

    def _on_drag_update(self, _g, dx, dy):
        if self._drag is None:
            return
        d, sx, sy = self._drag
        s = self._view[0]
        want_x, want_y = sx + dx / s, sy + dy / s
        snap = SNAP_PX / s
        others = [o for o in self.page.displays if o is not d]
        w, h = d.size
        # live: snap edges that come close, otherwise follow the pointer freely
        x, y = want_x, want_y
        for o in others:
            ow, oh = o.size
            for edge in (o.x - w, o.x + ow, o.x, o.x + ow - w):
                if abs(x - edge) <= snap:
                    x = edge
                    break
            for edge in (o.y - h, o.y + oh, o.y, o.y + oh - h):
                if abs(y - edge) <= snap:
                    y = edge
                    break
        d.x, d.y = round(x), round(y)
        self.queue_draw()

    def _on_drag_end(self, _g, dx, dy):
        if self._drag is None:
            return
        d, sx, sy = self._drag
        s = self._view[0]
        want_x, want_y = d.x, d.y
        others = [o for o in self.page.displays if o is not d]
        # settle: displays must touch a neighbour and never overlap
        cands = _touch_positions(d, others, want_x, want_y, SNAP_PX / s)
        if cands:
            d.x, d.y = min(cands, key=lambda c: (c[0] - want_x) ** 2 + (c[1] - want_y) ** 2)
        else:
            d.x, d.y = sx, sy
        self._drag = None
        self._view = None
        self.set_cursor_from_name("grab")
        self.queue_draw()
        self.page.changed()

    # -- drawing ---------------------------------------------------------

    def _draw(self, _area, cr, width, height):
        dark = Adw.StyleManager.get_default().get_dark()
        fg = (1, 1, 1) if dark else (0, 0, 0)
        layout = PangoCairo.create_layout(cr)
        family = self.get_pango_context().get_font_description().get_family() or "Sans"
        for d in self.page.displays:
            x, y, w, h = self._rect(d)
            x, y, w, h = x + 2, y + 2, w - 4, h - 4  # a hair of gap between neighbours
            selected = d is self.page.selected
            _rounded(cr, x, y, w, h, 10)
            if selected:
                cr.set_source_rgb(*ACCENT)
            else:
                cr.set_source_rgba(*fg, 0.10 if dark else 0.07)
            cr.fill_preserve()
            if not d.enabled:
                cr.set_dash([5, 4])
            cr.set_line_width(1.0)
            cr.set_source_rgba(*((1, 1, 1) if selected else fg), 0.25 if selected else 0.18)
            cr.stroke()
            cr.set_dash([])
            if self._drag is not None and d is self._drag[0]:
                _rounded(cr, x - 3, y - 3, w + 6, h + 6, 13)
                cr.set_source_rgba(*ACCENT, 0.35)
                cr.set_line_width(2)
                cr.stroke()

            text_rgb = (1, 1, 1) if selected else fg
            # number badge
            r = max(11, min(16, h * 0.12))
            cx, cy = x + 12 + r, y + 12 + r
            cr.arc(cx, cy, r, 0, 2 * math.pi)
            cr.set_source_rgba(*((1, 1, 1) if selected else fg), 0.22 if selected else 0.10)
            cr.fill()
            layout.set_font_description(Pango.FontDescription.from_string(f"{family} Bold {max(8, r * 0.75):.0f}px"))
            layout.set_text(str(d.index + 1), -1)
            tw, th = layout.get_pixel_size()
            cr.move_to(cx - tw / 2, cy - th / 2)
            cr.set_source_rgba(*text_rgb, 0.95)
            PangoCairo.show_layout(cr, layout)

            # name + resolution, centred
            if w > 90 and h > 50:
                layout.set_width(int((w - 16) * Pango.SCALE))
                layout.set_ellipsize(Pango.EllipsizeMode.END)
                layout.set_alignment(Pango.Alignment.CENTER)
                layout.set_font_description(Pango.FontDescription.from_string(f"{family} Bold 13px"))
                layout.set_text(d.name, -1)
                _tw, th1 = layout.get_pixel_size()
                cr.move_to(x + 8, y + h / 2 - th1)
                cr.set_source_rgba(*text_rgb, 0.95)
                PangoCairo.show_layout(cr, layout)
                layout.set_font_description(Pango.FontDescription.from_string(f"{family} 12px"))
                sub = f"{d.width} × {d.height}" if d.enabled else "Off"
                layout.set_text(sub, -1)
                cr.move_to(x + 8, y + h / 2 + 2)
                cr.set_source_rgba(*text_rgb, 0.7)
                PangoCairo.show_layout(cr, layout)
                layout.set_width(-1)


class MonitorsPage(Adw.Bin):
    def __init__(self, cm, toast_overlay):
        super().__init__()
        self.cm = cm
        self.toast_overlay = toast_overlay
        self.displays = []
        self.selected = None
        self._initial = {}
        self._updating = False
        self._token = None
        self._tick_id = 0

        self.stack = Gtk.Stack(transition_type=Gtk.StackTransitionType.CROSSFADE, transition_duration=180)
        self.set_child(self.stack)
        self.empty = Adw.StatusPage(
            icon_name="video-display-symbolic", title="No Displays Found",
            description="Display settings need a running Hyprland session to see your monitors.")
        self.stack.add_named(self.empty, "empty")

        prefs = Adw.PreferencesPage(
            description="Drag the screens to match how they sit on your desk. "
                        "Changes are tried out first and undone automatically unless you keep them.")
        self.stack.add_named(prefs, "main")

        canvas_group = Adw.PreferencesGroup()
        frame = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        frame.add_css_class("monitor-canvas-card")
        frame.set_overflow(Gtk.Overflow.HIDDEN)
        self.canvas = ArrangementCanvas(self)
        frame.append(self.canvas)
        canvas_group.add(frame)

        # apply / reset strip, slides in once something changed
        actions = Gtk.Box(spacing=8, halign=Gtk.Align.END, margin_top=12)
        self.reset_btn = Gtk.Button(label="Reset")
        self.reset_btn.add_css_class("flat")
        self.reset_btn.connect("clicked", lambda *_: self.refresh())
        self.apply_btn = Gtk.Button(label="Apply")
        self.apply_btn.add_css_class("suggested-action")
        self.apply_btn.add_css_class("pill-small")
        self.apply_btn.connect("clicked", lambda *_: self.apply())
        self.hint = Gtk.Label(label="Not applied yet", hexpand=True, xalign=1)
        self.hint.add_css_class("dim-label")
        actions.append(self.hint)
        actions.append(self.reset_btn)
        actions.append(self.apply_btn)
        self.actions_rev = Gtk.Revealer(child=actions, transition_duration=200,
                                        transition_type=Gtk.RevealerTransitionType.SLIDE_DOWN)
        canvas_group.add(self.actions_rev)
        prefs.add(canvas_group)

        self.details = Adw.PreferencesGroup()
        self.enabled_row = Adw.SwitchRow(title="Use This Display")
        self.res_row = Adw.ComboRow(title="Resolution")
        self.rate_row = Adw.ComboRow(title="Refresh Rate")
        self.scale_row = Adw.ComboRow(title="Scale")
        self.rot_row = Adw.ComboRow(title="Orientation",
                                    model=Gtk.StringList.new([t for _v, t in ROTATIONS]))
        for r in (self.enabled_row, self.res_row, self.rate_row, self.scale_row, self.rot_row):
            self.details.add(r)
        self.enabled_row.connect("notify::active", self._on_enabled)
        self.res_row.connect("notify::selected", self._on_resolution)
        self.rate_row.connect("notify::selected", self._on_rate)
        self.scale_row.connect("notify::selected", self._on_scale)
        self.rot_row.connect("notify::selected", self._on_rotation)
        prefs.add(self.details)
        self.refresh()

    # -- model -----------------------------------------------------------

    def refresh(self):
        try:
            infos = [m for m in self.cm.list_monitors() if m.get("width")]
        except Exception as e:
            infos = []
            self.toast(f"Couldn't read displays: {e}")
        keep = self.selected.name if self.selected else None
        self.displays = [Display(m, i) for i, m in enumerate(infos)]
        self._initial = {d.name: d.state() for d in self.displays}
        self.selected = next((d for d in self.displays if d.name == keep), None) or \
            (self.displays[0] if self.displays else None)
        self.stack.set_visible_child_name("main" if self.displays else "empty")
        self._load_details()
        self.changed(redraw_only=True)

    def is_changed(self):
        return any(self._initial.get(d.name) != d.state() for d in self.displays)

    def select(self, d):
        if d is not self.selected:
            self.selected = d
            self._load_details()
        self.canvas.queue_draw()

    def changed(self, redraw_only=False):
        self.canvas.queue_draw()
        self.actions_rev.set_reveal_child(self.is_changed())

    # -- details ---------------------------------------------------------

    def _load_details(self):
        d = self.selected
        if d is None:
            return
        self._updating = True
        try:
            self.details.set_title(GLib.markup_escape_text(f"{d.index + 1}. {d.label}"))
            self.details.set_description(GLib.markup_escape_text(d.name) if d.label != d.name else None)
            enabled_count = sum(1 for x in self.displays if x.enabled)
            self.enabled_row.set_active(d.enabled)
            last = d.enabled and enabled_count <= 1
            self.enabled_row.set_sensitive(not last)
            self.enabled_row.set_subtitle("Your only active display can't be turned off" if last else "")
            for r in (self.res_row, self.rate_row, self.scale_row, self.rot_row):
                r.set_sensitive(d.enabled)

            self._resolutions = []
            for w, h, _r in d.modes:
                if (w, h) not in self._resolutions:
                    self._resolutions.append((w, h))
            self.res_row.set_model(Gtk.StringList.new([f"{w} × {h}" for w, h in self._resolutions]))
            self.res_row.set_selected(self._resolutions.index((d.width, d.height)))
            self._load_rates()

            self._scales = list(SCALES)
            if not any(abs(s - d.scale) < 1e-3 for s in self._scales):
                self._scales.append(d.scale)
                self._scales.sort()
            self.scale_row.set_model(Gtk.StringList.new([f"{round(s * 100)}%" for s in self._scales]))
            self.scale_row.set_selected(min(range(len(self._scales)), key=lambda i: abs(self._scales[i] - d.scale)))
            self._scale_subtitle()

            rot_ids = [v for v, _t in ROTATIONS]
            self.rot_row.set_selected(rot_ids.index(d.transform) if d.transform in rot_ids else 0)
        finally:
            self._updating = False

    def _load_rates(self):
        d = self.selected
        rates = [r for w, h, r in d.modes if (w, h) == (d.width, d.height) and r]
        self._rates = sorted(set(rates), key=lambda r: -float(r))
        if d.rate and d.rate not in self._rates:
            self._rates.insert(0, d.rate)
        labels = [f"{float(r):.2f} Hz".replace(".00 Hz", " Hz") for r in self._rates] or ["Automatic"]
        self.rate_row.set_model(Gtk.StringList.new(labels))
        self.rate_row.set_selected(self._rates.index(d.rate) if d.rate in self._rates else 0)

    def _scale_subtitle(self):
        d = self.selected
        w, h = d.size
        self.scale_row.set_subtitle(f"Looks like {w} × {h}")

    def _on_enabled(self, row, _p):
        if self._updating or self.selected is None:
            return
        self.selected.enabled = row.get_active()
        self._load_details()
        self.changed()

    def _on_resolution(self, row, _p):
        if self._updating or self.selected is None:
            return
        d = self.selected
        d.width, d.height = self._resolutions[row.get_selected()]
        rates = sorted({r for w, h, r in d.modes if (w, h) == (d.width, d.height) and r}, key=lambda r: -float(r))
        d.rate = rates[0] if rates else None
        self._updating = True
        try:
            self._load_rates()
            self._scale_subtitle()
        finally:
            self._updating = False
        self._settle(d)

    def _on_rate(self, row, _p):
        if self._updating or self.selected is None or not self._rates:
            return
        self.selected.rate = self._rates[row.get_selected()]
        self.changed()

    def _on_scale(self, row, _p):
        if self._updating or self.selected is None:
            return
        self.selected.scale = self._scales[row.get_selected()]
        self._scale_subtitle()
        self._settle(self.selected)

    def _on_rotation(self, row, _p):
        if self._updating or self.selected is None:
            return
        self.selected.transform = ROTATIONS[row.get_selected()][0]
        self._scale_subtitle()
        self._settle(self.selected)

    def _settle(self, d):
        """After a size change, keep displays flush and non-overlapping."""
        others = [o for o in self.displays if o is not d]
        rect = (d.x, d.y, *d.size)
        if others and any(_overlap(rect, (o.x, o.y, *o.size)) for o in others):
            cands = _touch_positions(d, others, d.x, d.y, 0)
            if cands:
                d.x, d.y = min(cands, key=lambda c: (c[0] - d.x) ** 2 + (c[1] - d.y) ** 2)
        self.changed()

    # -- apply with revert ---------------------------------------------------

    def toast(self, text, timeout=2):
        self.toast_overlay.add_toast(Adw.Toast(title=GLib.markup_escape_text(text), timeout=timeout))

    def apply(self):
        changed = [d for d in self.displays if self._initial.get(d.name) != d.state()]
        if not changed:
            return
        try:
            for d in changed:
                self.cm.set_monitor(d.name, d.spec())
            token = self.cm.apply_monitors_with_revert(REVERT_SECONDS)
        except (ValueError, KeyError) as e:
            self.toast(f"Couldn't change displays: {e}", 5)
            for d in changed:
                try:
                    self.cm.reset_monitor(d.name)
                except Exception:
                    pass
            return
        res = getattr(self.cm, "last_apply_result", None)
        if res is not None and not res.ok:
            self.cm.confirm_monitors(token)  # nothing was changed: nothing to revert
            self.toast("Couldn't apply: " + (res.errors[0] if res.errors else "unknown error"), 6)
            self.refresh()
            return
        self.ask_keep(token)

    def ask_keep(self, token):
        self._token = token
        dlg = Adw.AlertDialog(heading="Keep These Display Settings?")
        dlg.add_response("revert", "Revert Settings")
        dlg.add_response("keep", "Keep Changes")
        dlg.set_response_appearance("keep", Adw.ResponseAppearance.SUGGESTED)
        dlg.set_default_response("keep")
        dlg.set_close_response("revert")
        dlg.connect("response", self._on_keep_response)
        self._dialog = dlg
        self._tick()
        self._tick_id = GLib.timeout_add(200, self._tick)
        dlg.present(self.get_root())
        return dlg

    def _remaining(self):
        deadline = self.cm.monitors_deadline(self._token) if self._token else None
        if deadline is None:
            return 0
        return max(0, math.ceil(deadline - time.monotonic()))

    def _tick(self):
        if self._token is None:
            self._tick_id = 0
            return GLib.SOURCE_REMOVE
        left = self._remaining()
        body = f"Reverting to the previous settings in {left} second{'s' if left != 1 else ''}."
        if self._dialog.get_body() != body:
            self._dialog.set_body(body)
        if left <= 0:
            self._tick_id = 0
            self._finish(keep=False)
            self._dialog.force_close()
            return GLib.SOURCE_REMOVE
        return GLib.SOURCE_CONTINUE

    def _on_keep_response(self, _dlg, response):
        if self._token is not None:
            self._finish(keep=response == "keep")

    def _finish(self, keep):
        token, self._token = self._token, None
        if self._tick_id:
            GLib.source_remove(self._tick_id)
            self._tick_id = 0
        if token is None:
            return
        if keep:
            self.cm.confirm_monitors(token)
            self.toast("Display settings saved")
        else:
            res = self.cm.revert_monitors(token)
            if getattr(res, "ok", True):
                self.toast("Previous display settings restored")
            else:
                self.toast("Couldn't revert: " + (res.errors[0] if res.errors else "unknown error"), 6)
        self.refresh()

    # -- dev helpers -------------------------------------------------------

    def dev_open(self, name):
        """Screenshot helper: nudge the second display and show the keep/revert countdown."""
        if len(self.displays) > 1:
            d = self.displays[1]
            d.scale = 1.0
            self._settle(d)
        self.apply()
