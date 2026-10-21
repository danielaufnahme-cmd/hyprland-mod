"""Floating "unsaved changes" bar that springs up from the bottom."""

from gi.repository import Adw, Graphene, Gtk


class ApplyBar(Gtk.Widget):
    def __init__(self, on_discard, on_apply):
        super().__init__(halign=Gtk.Align.CENTER, valign=Gtk.Align.END, visible=False)
        self.set_layout_manager(Gtk.BinLayout())
        self._offset = 1.0
        self._shown = False

        card = Gtk.Box(spacing=6)
        card.add_css_class("apply-bar")
        icon = Gtk.Image(icon_name="document-edit-symbolic")
        icon.add_css_class("apply-bar-icon")
        card.append(icon)
        self.label = Gtk.Label(xalign=0)
        self.label.add_css_class("apply-bar-label")
        card.append(self.label)
        discard = Gtk.Button(label="Discard")
        discard.add_css_class("flat")
        discard.connect("clicked", lambda *_: on_discard())
        apply = Gtk.Button(label="Apply")
        apply.add_css_class("suggested-action")
        apply.add_css_class("pill-small")
        apply.set_tooltip_text("Apply changes (Ctrl+S)")
        apply.connect("clicked", lambda *_: on_apply())
        card.append(discard)
        card.append(apply)
        card.set_parent(self)
        self._card = card

        target = Adw.CallbackAnimationTarget.new(self._set_offset)
        self._in = Adw.SpringAnimation.new(self, 1.0, 0.0, Adw.SpringParams.new(0.72, 1.0, 380.0), target)
        self._in.set_epsilon(0.0005)
        self._out = Adw.TimedAnimation.new(self, 0.0, 1.0, 170, target)
        self._out.set_easing(Adw.Easing.EASE_IN_CUBIC)
        self._out.connect("done", lambda *_: self.set_visible(self._shown))

    def do_dispose(self):
        if self._card:
            self._card.unparent()
            self._card = None
        Gtk.Widget.do_dispose(self)

    def _set_offset(self, v):
        self._offset = v
        self.set_opacity(max(0.0, min(1.0, 1.0 - v * 1.2)))
        self.queue_draw()

    def do_snapshot(self, snapshot):
        h = self.get_height() + 24
        snapshot.save()
        snapshot.translate(Graphene.Point().init(0, self._offset * h))
        self.snapshot_child(self._card, snapshot)
        snapshot.restore()

    def set_count(self, n):
        self.label.set_label("1 unsaved change" if n == 1 else f"{n} unsaved changes")
        show = n > 0
        if show == self._shown:
            return
        self._shown = show
        if show:
            self._out.pause()
            self.set_visible(True)
            self._in.set_value_from(self._offset)
            self._in.play()
        else:
            self._in.pause()
            self._out.set_value_from(self._offset)
            self._out.play()

    def settle(self):
        self._in.skip() if self._shown else self._out.skip()
