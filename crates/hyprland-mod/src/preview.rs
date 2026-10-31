//! Animated mock desktop that mirrors the current look settings.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::f64::consts::PI;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo::{self, Context, ImageSurface};

use crate::backend::WithClasses;
use crate::backend::{self, Cm, OptionSpec};
use crate::colors::{self, Rgba};
use crate::rows::rounded_rect;

const KEYS: &[(&str, &str, &str)] = &[
    ("gaps_in", "general:gaps_in", "5"),
    ("gaps_out", "general:gaps_out", "20"),
    ("border", "general:border_size", "2"),
    (
        "active",
        "general:col.active_border",
        "rgba(33ccffee) rgba(00ff99ee) 45deg",
    ),
    ("inactive", "general:col.inactive_border", "rgba(595959aa)"),
    ("rounding", "decoration:rounding", "10"),
    ("shadow_on", "decoration:shadow:enabled", "true"),
    ("shadow_range", "decoration:shadow:range", "4"),
    ("shadow_color", "decoration:shadow:color", "rgba(1a1a1aee)"),
    ("dim_on", "decoration:dim_inactive", "false"),
    ("dim_strength", "decoration:dim_strength", "0.5"),
    ("a_op", "decoration:active_opacity", "1.0"),
    ("i_op", "decoration:inactive_opacity", "1.0"),
    ("blur", "decoration:blur:enabled", "true"),
];
const STOPS: usize = 4;
const VIRTUAL_H: f64 = 440.0;

fn num(text: &str, default: f64) -> f64 {
    backend::parse_number(text).unwrap_or(default)
}

fn truthy(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn sample(cols: &[Rgba], t: f64) -> Rgba {
    if cols.len() == 1 {
        return cols[0];
    }
    let pos = t * (cols.len() - 1) as f64;
    let i = (pos as usize).min(cols.len() - 2);
    colors::mix(cols[i], cols[i + 1], pos - i as f64)
}

fn resample(cols: &[Rgba]) -> [Rgba; STOPS] {
    std::array::from_fn(|i| sample(cols, i as f64 / (STOPS - 1) as f64))
}

#[derive(Clone, PartialEq, Debug)]
struct State {
    gaps_in: f64,
    gaps_out: f64,
    border: f64,
    rounding: f64,
    shadow: f64,
    shadow_alpha: f64,
    shadow_color: Rgba,
    dim: f64,
    a_op: f64,
    i_op: f64,
    blur: f64,
    a_cols: [Rgba; STOPS],
    a_deg: f64,
    i_cols: [Rgba; STOPS],
    i_deg: f64,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

impl State {
    fn lerp(&self, b: &State, t: f64) -> State {
        let cols = |x: &[Rgba; STOPS], y: &[Rgba; STOPS]| {
            std::array::from_fn(|i| colors::mix(x[i], y[i], t))
        };
        State {
            gaps_in: lerp(self.gaps_in, b.gaps_in, t),
            gaps_out: lerp(self.gaps_out, b.gaps_out, t),
            border: lerp(self.border, b.border, t),
            rounding: lerp(self.rounding, b.rounding, t),
            shadow: lerp(self.shadow, b.shadow, t),
            shadow_alpha: lerp(self.shadow_alpha, b.shadow_alpha, t),
            shadow_color: colors::mix(self.shadow_color, b.shadow_color, t),
            dim: lerp(self.dim, b.dim, t),
            a_op: lerp(self.a_op, b.a_op, t),
            i_op: lerp(self.i_op, b.i_op, t),
            blur: lerp(self.blur, b.blur, t),
            a_cols: cols(&self.a_cols, &b.a_cols),
            a_deg: lerp(self.a_deg, b.a_deg, t),
            i_cols: cols(&self.i_cols, &b.i_cols),
            i_deg: lerp(self.i_deg, b.i_deg, t),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Editor,
    Terminal,
    Files,
}

struct Wallpaper {
    size: (i32, i32),
    wall: ImageSurface,
    blur: ImageSurface,
}

struct Inner {
    cm: Cm,
    opts: RefCell<HashMap<String, OptionSpec>>,
    shown: RefCell<State>,
    from: RefCell<State>,
    target: RefCell<State>,
    wall: RefCell<Option<Wallpaper>>,
    anim: OnceCell<adw::TimedAnimation>,
}

#[derive(Clone)]
pub struct LivePreview {
    pub area: gtk::DrawingArea,
    inner: Rc<Inner>,
}

impl LivePreview {
    pub fn new(cm: &Cm, opts: HashMap<String, OptionSpec>) -> Self {
        let area = gtk::DrawingArea::builder()
            .hexpand(true)
            .content_height(320)
            .build()
            .with_classes(&["live-preview"]);
        let inner = Rc::new(Inner {
            cm: cm.clone(),
            opts: RefCell::new(opts),
            shown: RefCell::new(State::lerp_dummy()),
            from: RefCell::new(State::lerp_dummy()),
            target: RefCell::new(State::lerp_dummy()),
            wall: RefCell::new(None),
            anim: OnceCell::new(),
        });
        let st = inner.read();
        *inner.shown.borrow_mut() = st.clone();
        *inner.from.borrow_mut() = st.clone();
        *inner.target.borrow_mut() = st;

        let weak = Rc::downgrade(&inner);
        let a = area.clone();
        let target = adw::CallbackAnimationTarget::new(move |t| {
            if let Some(inner) = weak.upgrade() {
                let s = inner.from.borrow().lerp(&inner.target.borrow(), t);
                *inner.shown.borrow_mut() = s;
                a.queue_draw();
            }
        });
        let anim = adw::TimedAnimation::new(&area, 0.0, 1.0, 260, target);
        anim.set_easing(adw::Easing::EaseOutCubic);
        let _ = inner.anim.set(anim);

        let weak = Rc::downgrade(&inner);
        area.set_draw_func(move |_, cr, w, h| {
            if let Some(inner) = weak.upgrade() {
                inner.draw(cr, w, h);
            }
        });
        Self { area, inner }
    }

    pub fn handles(&self, key: Option<&str>) -> bool {
        key.is_none_or(|k| KEYS.iter().any(|(_, kk, _)| *kk == k))
    }

    pub fn update(&self) {
        let inner = &self.inner;
        let target = inner.read();
        if target == *inner.target.borrow() {
            return;
        }
        let shown = inner.shown.borrow().clone();
        *inner.from.borrow_mut() = shown;
        *inner.target.borrow_mut() = target.clone();
        let anim = inner.anim.get().expect("animation");
        if self.area.is_mapped() {
            anim.reset();
            anim.play();
        } else {
            *inner.shown.borrow_mut() = target;
            self.area.queue_draw();
        }
    }
}

impl State {
    fn lerp_dummy() -> State {
        State {
            gaps_in: 0.0,
            gaps_out: 0.0,
            border: 0.0,
            rounding: 0.0,
            shadow: 0.0,
            shadow_alpha: 0.0,
            shadow_color: [0.0; 4],
            dim: 0.0,
            a_op: 1.0,
            i_op: 1.0,
            blur: 0.0,
            a_cols: [[0.0; 4]; STOPS],
            a_deg: 0.0,
            i_cols: [[0.0; 4]; STOPS],
            i_deg: 0.0,
        }
    }
}

impl Inner {
    fn text(&self, name: &str) -> String {
        let (_, key, fallback) = KEYS
            .iter()
            .find(|(n, _, _)| *n == name)
            .expect("known preview key");
        if let Some(opt) = self.opts.borrow().get(*key) {
            return backend::current_text(&self.cm, opt);
        }
        self.cm
            .borrow()
            .get(key)
            .unwrap_or_else(|| fallback.to_string())
    }

    fn read(&self) -> State {
        let (a_cols, a_deg) = colors::parse_gradient(&self.text("active"));
        let (i_cols, i_deg) = colors::parse_gradient(&self.text("inactive"));
        let shadow_on = truthy(&self.text("shadow_on"));
        State {
            gaps_in: num(&self.text("gaps_in"), 5.0),
            gaps_out: num(&self.text("gaps_out"), 20.0),
            border: num(&self.text("border"), 2.0),
            rounding: num(&self.text("rounding"), 10.0),
            shadow: if shadow_on {
                num(&self.text("shadow_range"), 4.0)
            } else {
                0.0
            },
            shadow_alpha: if shadow_on { 1.0 } else { 0.0 },
            shadow_color: colors::parse_color_or(&self.text("shadow_color"), [0.1, 0.1, 0.1, 0.9]),
            dim: if truthy(&self.text("dim_on")) {
                num(&self.text("dim_strength"), 0.5)
            } else {
                0.0
            },
            a_op: num(&self.text("a_op"), 1.0),
            i_op: num(&self.text("i_op"), 1.0),
            blur: if truthy(&self.text("blur")) { 1.0 } else { 0.0 },
            a_cols: resample(&a_cols),
            a_deg: a_deg as f64,
            i_cols: resample(&i_cols),
            i_deg: i_deg as f64,
        }
    }

    // -- drawing -------------------------------------------------------

    fn ensure_wallpaper(&self, w: i32, h: i32) -> Result<(), cairo::Error> {
        if self
            .wall
            .borrow()
            .as_ref()
            .is_some_and(|wp| wp.size == (w, h))
        {
            return Ok(());
        }
        let (fw, fh) = (w as f64, h as f64);
        let wall = ImageSurface::create(cairo::Format::ARgb32, w, h)?;
        {
            let cr = Context::new(&wall)?;
            let bg = cairo::LinearGradient::new(0.0, 0.0, fw, fh);
            bg.add_color_stop_rgb(0.0, 0.07, 0.15, 0.24);
            bg.add_color_stop_rgb(0.55, 0.10, 0.27, 0.36);
            bg.add_color_stop_rgb(1.0, 0.13, 0.36, 0.40);
            cr.set_source(&bg)?;
            cr.paint()?;
            for (fx, fy, fr, col) in [
                (0.82, 0.12, 0.55, [0.27, 0.72, 0.70, 0.55]),
                (0.12, 0.95, 0.60, [0.22, 0.45, 0.85, 0.50]),
                (0.45, 0.45, 0.35, [0.55, 0.80, 0.90, 0.18]),
            ] {
                let rg = cairo::RadialGradient::new(
                    fx * fw,
                    fy * fh,
                    0.0,
                    fx * fw,
                    fy * fh,
                    fr * fw.max(fh),
                );
                rg.add_color_stop_rgba(0.0, col[0], col[1], col[2], col[3]);
                rg.add_color_stop_rgba(1.0, col[0], col[1], col[2], 0.0);
                cr.set_source(&rg)?;
                cr.paint()?;
            }
            // soft hills
            cr.move_to(0.0, fh * 0.78);
            cr.curve_to(fw * 0.3, fh * 0.62, fw * 0.55, fh * 0.92, fw, fh * 0.70);
            cr.line_to(fw, fh);
            cr.line_to(0.0, fh);
            cr.close_path();
            cr.set_source_rgba(0.04, 0.10, 0.16, 0.45);
            cr.fill()?;
        }
        let f = 14;
        let small = ImageSurface::create(cairo::Format::ARgb32, (w / f).max(1), (h / f).max(1))?;
        {
            let sc = Context::new(&small)?;
            sc.scale(1.0 / f as f64, 1.0 / f as f64);
            sc.set_source_surface(&wall, 0.0, 0.0)?;
            sc.source().set_filter(cairo::Filter::Good);
            sc.paint()?;
        }
        let blur = ImageSurface::create(cairo::Format::ARgb32, w, h)?;
        {
            let bc = Context::new(&blur)?;
            bc.scale(f as f64, f as f64);
            bc.set_source_surface(&small, 0.0, 0.0)?;
            bc.source().set_filter(cairo::Filter::Bilinear);
            bc.source().set_extend(cairo::Extend::Pad);
            bc.paint()?;
        }
        *self.wall.borrow_mut() = Some(Wallpaper {
            size: (w, h),
            wall,
            blur,
        });
        Ok(())
    }

    fn draw(&self, cr: &Context, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        // cairo errors only happen on OOM; nothing sensible to do but skip the frame
        let _ = self.try_draw(cr, w, h);
    }

    fn try_draw(&self, cr: &Context, wi: i32, hi: i32) -> Result<(), cairo::Error> {
        self.ensure_wallpaper(wi, hi)?;
        let wallpaper = self.wall.borrow();
        let wp = wallpaper.as_ref().expect("wallpaper");
        let st = self.shown.borrow().clone();
        let (w, h) = (wi as f64, hi as f64);
        let s = h / VIRTUAL_H;

        cr.set_source_surface(&wp.wall, 0.0, 0.0)?;
        cr.paint()?;

        // top bar
        let bar_h = 30.0 * s;
        let go = st.gaps_out.max(0.0) * s;
        let gi = st.gaps_in.max(0.0) * s;
        rounded_rect(
            cr,
            10.0 * s,
            7.0 * s,
            w - 20.0 * s,
            bar_h - 8.0 * s,
            8.0 * s,
        );
        cr.set_source_rgba(0.02, 0.05, 0.09, 0.40);
        cr.fill()?;
        for i in 0..4 {
            let r = if i == 0 { 4.2 } else { 3.2 } * s;
            cr.arc(
                26.0 * s + i as f64 * 16.0 * s,
                7.0 * s + (bar_h - 8.0 * s) / 2.0,
                r,
                0.0,
                2.0 * PI,
            );
            cr.set_source_rgba(1.0, 1.0, 1.0, if i == 0 { 0.9 } else { 0.35 });
            cr.fill()?;
        }
        rounded_rect(
            cr,
            w / 2.0 - 34.0 * s,
            12.0 * s,
            68.0 * s,
            bar_h - 18.0 * s,
            5.0 * s,
        );
        cr.set_source_rgba(1.0, 1.0, 1.0, 0.55);
        cr.fill()?;
        for i in 0..3 {
            rounded_rect(
                cr,
                w - 30.0 * s - i as f64 * 20.0 * s,
                13.0 * s,
                12.0 * s,
                bar_h - 20.0 * s,
                3.0 * s,
            );
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.35);
            cr.fill()?;
        }

        let top = bar_h + 2.0 * s;
        let (ax, ay) = (go, top + go);
        let (aw, ah) = (w - 2.0 * go, h - top - 2.0 * go);
        if aw < 40.0 || ah < 40.0 {
            return Ok(());
        }
        let left_w = (aw - 2.0 * gi) * 0.55;
        let right_x = ax + left_w + 2.0 * gi;
        let right_w = aw - left_w - 2.0 * gi;
        let top_h = (ah - 2.0 * gi) * 0.5;
        let wins = [
            ((ax, ay, left_w, ah), true, Kind::Editor),
            ((right_x, ay, right_w, top_h), false, Kind::Terminal),
            (
                (
                    right_x,
                    ay + top_h + 2.0 * gi,
                    right_w,
                    ah - top_h - 2.0 * gi,
                ),
                false,
                Kind::Files,
            ),
        ];
        for (rect, active, kind) in wins.iter().rev() {
            self.draw_window(cr, wp, *rect, *active, *kind, &st, s)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_window(
        &self,
        cr: &Context,
        wp: &Wallpaper,
        rect: (f64, f64, f64, f64),
        active: bool,
        kind: Kind,
        st: &State,
        s: f64,
    ) -> Result<(), cairo::Error> {
        let (x, y, w, h) = rect;
        let r = st.rounding.max(0.0) * s;
        let mut bs = st.border.max(0.0) * s;
        if st.border > 0.0 && bs < 1.0 {
            bs = 1.0;
        }
        let op = (if active { st.a_op } else { st.i_op }).clamp(0.0, 1.0);

        // shadow
        let rng = st.shadow * s;
        if rng > 0.3 && st.shadow_alpha > 0.0 {
            let sc = st.shadow_color;
            let steps = (rng as i32).clamp(4, 16);
            for i in (1..=steps).rev() {
                let grow = rng * i as f64 / steps as f64;
                let a = sc[3]
                    * st.shadow_alpha
                    * (1.0 - i as f64 / (steps + 1) as f64).powi(2)
                    * 0.35
                    * (0.6 + 0.4 * op);
                rounded_rect(
                    cr,
                    x - grow,
                    y - grow + rng * 0.15,
                    w + 2.0 * grow,
                    h + 2.0 * grow,
                    r + grow,
                );
                cr.set_source_rgba(sc[0], sc[1], sc[2], a);
                cr.fill()?;
            }
        }

        // border
        if bs > 0.0 {
            let (cols, deg) = if active {
                (&st.a_cols, st.a_deg)
            } else {
                (&st.i_cols, st.i_deg)
            };
            rounded_rect(cr, x, y, w, h, r);
            cr.set_source(colors::gradient_pattern(cols, deg, x, y, w, h))?;
            cr.fill()?;
        }

        let (ix, iy, iw, ih) = (x + bs, y + bs, w - 2.0 * bs, h - 2.0 * bs);
        if iw <= 2.0 || ih <= 2.0 {
            return Ok(());
        }
        let ir = (r - bs).max(0.0);

        cr.save()?;
        rounded_rect(cr, ix, iy, iw, ih, ir);
        cr.clip();
        if bs > 0.0 {
            // punch the inner area so translucent windows don't show the border fill
            cr.set_source_surface(&wp.wall, 0.0, 0.0)?;
            cr.paint()?;
        }
        if op < 0.999 && st.blur > 0.0 {
            cr.set_source_surface(&wp.blur, 0.0, 0.0)?;
            cr.paint_with_alpha(st.blur)?;
        }
        cr.push_group();
        draw_content(cr, kind, ix, iy, iw, ih, s)?;
        cr.pop_group_to_source()?;
        cr.paint_with_alpha(op)?;
        if !active && st.dim > 0.0 {
            cr.set_source_rgba(0.0, 0.0, 0.0, st.dim.clamp(0.0, 1.0) * 0.75);
            cr.paint()?;
        }
        cr.restore()?;
        Ok(())
    }
}

fn pill(
    cr: &Context,
    px: f64,
    py: f64,
    pw: f64,
    ph: f64,
    rgba: Rgba,
    rad: Option<f64>,
) -> Result<(), cairo::Error> {
    rounded_rect(cr, px, py, pw.max(0.0), ph, rad.unwrap_or(ph / 2.0));
    cr.set_source_rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
    cr.fill()
}

fn draw_content(
    cr: &Context,
    kind: Kind,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    s: f64,
) -> Result<(), cairo::Error> {
    match kind {
        Kind::Terminal => {
            cr.set_source_rgb(0.11, 0.12, 0.15);
            cr.paint()?;
            let lh = 15.0 * s;
            let pal = [
                [0.40, 0.80, 0.65, 1.0],
                [0.45, 0.65, 0.95, 1.0],
                [0.85, 0.85, 0.88, 0.75],
            ];
            let mut yy = y + 14.0 * s;
            let widths = [0.32, 0.55, 0.42, 0.18, 0.62, 0.28, 0.48, 0.36, 0.22, 0.52];
            for (i, fw) in widths.iter().enumerate() {
                if yy + lh > y + h - 6.0 * s {
                    break;
                }
                pill(cr, x + 14.0 * s, yy, 8.0 * s, 7.0 * s, pal[0], None)?;
                let c = if i % 3 == 0 { pal[1] } else { pal[2] };
                pill(cr, x + 26.0 * s, yy, (w - 40.0 * s) * fw, 7.0 * s, c, None)?;
                yy += lh;
            }
            pill(
                cr,
                x + 14.0 * s,
                yy,
                7.0 * s,
                9.0 * s,
                [0.9, 0.9, 0.92, 0.9],
                Some(1.0),
            )?;
        }
        Kind::Editor => {
            cr.set_source_rgb(0.98, 0.98, 0.985);
            cr.paint()?;
            let hb = 34.0 * s;
            cr.rectangle(x, y, w, hb);
            cr.set_source_rgb(0.93, 0.93, 0.945);
            cr.fill()?;
            pill(
                cr,
                x + 14.0 * s,
                y + 13.0 * s,
                60.0 * s,
                8.0 * s,
                [0.2, 0.2, 0.25, 0.35],
                None,
            )?;
            for i in 0..3 {
                cr.arc(
                    x + w - 18.0 * s - i as f64 * 18.0 * s,
                    y + hb / 2.0,
                    5.0 * s,
                    0.0,
                    2.0 * PI,
                );
                cr.set_source_rgba(0.2, 0.2, 0.25, 0.18);
                cr.fill()?;
            }
            let side = (150.0 * s).min(w * 0.28);
            cr.rectangle(x, y + hb, side, h - hb);
            cr.set_source_rgb(0.955, 0.955, 0.965);
            cr.fill()?;
            let mut yy = y + hb + 16.0 * s;
            for i in 0..7 {
                let c = if i == 1 {
                    [0.21, 0.52, 0.89, 0.85]
                } else {
                    [0.2, 0.2, 0.25, 0.22]
                };
                pill(
                    cr,
                    x + 14.0 * s,
                    yy,
                    side * (0.45 + 0.08 * (i % 3) as f64),
                    7.0 * s,
                    c,
                    None,
                )?;
                yy += 20.0 * s;
            }
            let syntax = [
                [0.75, 0.25, 0.45, 0.85],
                [0.21, 0.52, 0.89, 0.85],
                [0.15, 0.6, 0.5, 0.85],
                [0.2, 0.2, 0.25, 0.45],
            ];
            let code: [(usize, f64, usize, f64); 12] = [
                (0, 0.22, 0, 0.34),
                (1, 0.18, 3, 0.4),
                (1, 0.30, 2, 0.2),
                (2, 0.12, 3, 0.45),
                (0, 0.0, 0, 0.0),
                (0, 0.26, 3, 0.3),
                (1, 0.14, 3, 0.5),
                (2, 0.2, 1, 0.24),
                (1, 0.35, 3, 0.18),
                (0, 0.0, 0, 0.0),
                (0, 0.18, 2, 0.4),
                (1, 0.22, 3, 0.36),
            ];
            let indents = [0, 1, 1, 2, 0, 0, 1, 2, 2, 0, 0, 1];
            let cx = x + side + 22.0 * s;
            let mut yy = y + hb + 18.0 * s;
            let avail = w - side - 40.0 * s;
            for (indent, (c1, w1, c2, w2)) in indents.iter().zip(code) {
                if yy > y + h - 14.0 * s {
                    break;
                }
                if w1 > 0.0 {
                    let ox = cx + *indent as f64 * 18.0 * s;
                    pill(cr, ox, yy, avail * w1, 7.0 * s, syntax[c1], None)?;
                    pill(
                        cr,
                        ox + avail * w1 + 6.0 * s,
                        yy,
                        avail * w2,
                        7.0 * s,
                        syntax[c2],
                        None,
                    )?;
                }
                yy += 18.0 * s;
            }
        }
        Kind::Files => {
            cr.set_source_rgb(0.965, 0.965, 0.97);
            cr.paint()?;
            let hb = 30.0 * s;
            cr.rectangle(x, y, w, hb);
            cr.set_source_rgb(0.92, 0.92, 0.935);
            cr.fill()?;
            pill(
                cr,
                x + 12.0 * s,
                y + 11.0 * s,
                44.0 * s,
                8.0 * s,
                [0.2, 0.2, 0.25, 0.3],
                None,
            )?;
            let (tile, gap) = (38.0 * s, 16.0 * s);
            let cols = (((w - gap) / (tile + gap)).floor() as i64).max(1);
            let ox = x + (w - cols as f64 * (tile + gap) + gap) / 2.0;
            let mut yy = y + hb + 14.0 * s;
            let tints = [
                [0.21, 0.52, 0.89],
                [0.32, 0.65, 0.78],
                [0.95, 0.70, 0.30],
                [0.45, 0.72, 0.50],
            ];
            let mut k = 0;
            while yy + tile < y + h - 4.0 * s {
                for c in 0..cols {
                    let t = tints[k % tints.len()];
                    let tx = ox + c as f64 * (tile + gap);
                    rounded_rect(cr, tx, yy, tile, tile * 0.78, 5.0 * s);
                    cr.set_source_rgba(t[0], t[1], t[2], 0.85);
                    cr.fill()?;
                    pill(
                        cr,
                        tx + tile * 0.15,
                        yy + tile * 0.86,
                        tile * 0.7,
                        5.0 * s,
                        [0.2, 0.2, 0.25, 0.3],
                        None,
                    )?;
                    k += 1;
                }
                yy += tile + gap;
            }
        }
    }
    Ok(())
}
