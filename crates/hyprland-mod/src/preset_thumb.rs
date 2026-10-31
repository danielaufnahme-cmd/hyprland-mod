//! Small cairo mock desktop drawn from a preset's own values (no ConfigManager needed).

use std::cell::RefCell;
use std::f64::consts::PI;
use std::rc::Rc;

use adw::prelude::*;
use gtk::cairo::{self, Context, ImageSurface};

use crate::backend::{self, Json, Preset, WithClasses};
use crate::colors::{self, Rgba};
use crate::rows::rounded_rect;

const VIRTUAL_H: f64 = 210.0;

type Rgb = [f64; 3];

fn hex(text: &str) -> Option<Rgba> {
    gtk::gdk::RGBA::parse(text)
        .ok()
        .map(|c| colors::from_gdk(&c))
}

fn luma(c: &[f64]) -> f64 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn num(text: &str, default: f64) -> f64 {
    text.split_whitespace()
        .next()
        .and_then(|t| t.parse::<f64>().ok())
        .unwrap_or(default)
}

fn truthy(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

struct Thumb {
    preset: Rc<Preset>,
    cache: RefCell<Option<(i32, i32, ImageSurface)>>,
}

/// A 128px-high thumbnail; drawn once per size, then replayed (hover redraws stay cheap).
pub fn thumbnail(preset: Rc<Preset>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_height(128)
        .hexpand(true)
        .overflow(gtk::Overflow::Hidden)
        .can_target(false)
        .build()
        .with_classes(&["preset-thumb"]);
    let thumb = Thumb {
        preset,
        cache: RefCell::new(None),
    };
    area.set_draw_func(move |a, cr, w, h| thumb.draw_cached(a, cr, w, h));
    area
}

impl Thumb {
    fn get(&self, key: &str, default: &'static str) -> String {
        self.preset
            .settings
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    }

    fn draw_cached(&self, area: &gtk::DrawingArea, cr: &Context, w: i32, h: i32) {
        if w <= 0 || h <= 0 {
            return;
        }
        let stale = !matches!(&*self.cache.borrow(), Some((cw, ch, _)) if (*cw, *ch) == (w, h));
        if stale {
            let scale = area.scale_factor().max(1);
            let Ok(surf) = ImageSurface::create(cairo::Format::ARgb32, w * scale, h * scale) else {
                return;
            };
            surf.set_device_scale(scale as f64, scale as f64);
            if let Ok(c) = Context::new(&surf) {
                self.draw(&c, w as f64, h as f64);
            }
            surf.flush();
            *self.cache.borrow_mut() = Some((w, h, surf));
        }
        if let Some((_, _, surf)) = &*self.cache.borrow() {
            if cr.set_source_surface(surf, 0.0, 0.0).is_ok() {
                let _ = cr.paint();
            }
        }
    }

    fn wallpaper(&self) -> Vec<Rgba> {
        let wall: Vec<Rgba> = self
            .preset
            .preview
            .get("wallpaper")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_str).filter_map(hex).collect())
            .unwrap_or_default();
        if wall.is_empty() {
            ["#26384a", "#3b6074"]
                .iter()
                .filter_map(|c| hex(c))
                .collect()
        } else {
            wall
        }
    }

    fn draw(&self, cr: &Context, w: f64, h: f64) {
        let wall = self.wallpaper();
        let s = h / VIRTUAL_H;

        // wallpaper
        let bg = cairo::LinearGradient::new(0.0, 0.0, w, h);
        let n = wall.len();
        for (i, c) in wall.iter().enumerate() {
            bg.add_color_stop_rgb(i as f64 / (n.max(2) - 1) as f64, c[0], c[1], c[2]);
        }
        let _ = cr.set_source(&bg);
        let _ = cr.paint();
        let glow = cairo::RadialGradient::new(w * 0.85, h * 0.1, 0.0, w * 0.85, h * 0.1, w * 0.7);
        glow.add_color_stop_rgba(0.0, 1.0, 1.0, 1.0, 0.16);
        glow.add_color_stop_rgba(1.0, 1.0, 1.0, 1.0, 0.0);
        let _ = cr.set_source(&glow);
        let _ = cr.paint();
        let avg: Rgb = std::array::from_fn(|i| wall.iter().map(|c| c[i]).sum::<f64>() / n as f64);
        let light_wall = luma(&avg) > 0.6;

        // top bar
        let bar_h = 16.0 * s;
        rounded_rect(cr, 8.0 * s, 5.0 * s, w - 16.0 * s, bar_h, 5.0 * s);
        if light_wall {
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.45);
        } else {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.32);
        }
        let _ = cr.fill();
        for i in 0..3 {
            let r = if i == 0 { 3.0 } else { 2.3 } * s;
            cr.arc(
                20.0 * s + i as f64 * 11.0 * s,
                5.0 * s + bar_h / 2.0,
                r,
                0.0,
                2.0 * PI,
            );
            let a = if i == 0 { 0.8 } else { 0.35 };
            if light_wall {
                cr.set_source_rgba(0.2, 0.2, 0.2, a);
            } else {
                cr.set_source_rgba(1.0, 1.0, 1.0, if i == 0 { 0.85 } else { 0.35 });
            }
            let _ = cr.fill();
        }

        let go = num(&self.get("general:gaps_out", "20"), 0.0).max(0.0) * s * 1.1;
        let gi = num(&self.get("general:gaps_in", "5"), 0.0).max(0.0) * s * 1.1;
        let top = 5.0 * s + bar_h + 2.0 * s;
        let (ax, ay, aw, ah) = (go, top + go, w - 2.0 * go, h - top - 2.0 * go);
        if aw < 30.0 || ah < 30.0 {
            return;
        }
        let left_w = (aw - 2.0 * gi) * 0.56;
        let (rx, rw) = (ax + left_w + 2.0 * gi, aw - left_w - 2.0 * gi);
        let th = (ah - 2.0 * gi) * 0.5;
        let wins = [
            ((rx, ay + th + 2.0 * gi, rw, ah - th - 2.0 * gi), false),
            ((rx, ay, rw, th), false),
            ((ax, ay, left_w, ah), true),
        ];
        for (rect, active) in wins {
            self.window(cr, rect, active, s, &avg);
        }
    }

    fn window(&self, cr: &Context, rect: (f64, f64, f64, f64), active: bool, s: f64, avg: &Rgb) {
        let (x, y, w, h) = rect;
        let r = num(&self.get("decoration:rounding", "10"), 0.0).max(0.0) * s * 1.15;
        let mut bs = num(&self.get("general:border_size", "2"), 0.0).max(0.0) * s * 1.2;
        if bs > 0.0 {
            bs = bs.max(1.0);
        }
        let op_key = if active {
            "decoration:active_opacity"
        } else {
            "decoration:inactive_opacity"
        };
        let op = num(&self.get(op_key, "1"), 1.0).clamp(0.0, 1.0);
        let blur = truthy(&self.get("decoration:blur:enabled", "true"));

        // shadow
        if truthy(&self.get("decoration:shadow:enabled", "true")) {
            let rng = num(&self.get("decoration:shadow:range", "4"), 0.0) * s;
            let sc = colors::parse_color_or(
                &self.get("decoration:shadow:color", "rgba(1a1a1aee)"),
                [0.1, 0.1, 0.1, 0.93],
            );
            let offset = self.get("decoration:shadow:offset", "0 0");
            let mut off = offset.split_whitespace().take(2).map(|p| num(p, 0.0) * s);
            let (ox, oy) = (off.next().unwrap_or(0.0), off.next().unwrap_or(0.0));
            if truthy(&self.get("decoration:shadow:sharp", "false")) {
                rounded_rect(cr, x + ox, y + oy, w + rng, h + rng, r);
                cr.set_source_rgba(sc[0], sc[1], sc[2], sc[3] * 0.85);
                let _ = cr.fill();
            } else if rng > 0.3 {
                let steps = (rng as i32).clamp(4, 12);
                for i in (1..=steps).rev() {
                    let grow = rng * i as f64 / steps as f64;
                    let a = sc[3] * (1.0 - i as f64 / (steps + 1) as f64).powi(2) * 0.45;
                    rounded_rect(
                        cr,
                        x - grow + ox,
                        y - grow + oy,
                        w + 2.0 * grow,
                        h + 2.0 * grow,
                        r + grow,
                    );
                    cr.set_source_rgba(sc[0], sc[1], sc[2], a);
                    let _ = cr.fill();
                }
            }
        }

        // border ring (even-odd so translucent windows don't show the border fill)
        if bs > 0.0 {
            let key = if active {
                "general:col.active_border"
            } else {
                "general:col.inactive_border"
            };
            let (cols, deg) = colors::parse_gradient(&self.get(key, "rgba(ffffffff)"));
            rounded_rect(cr, x, y, w, h, r);
            rounded_rect(
                cr,
                x + bs,
                y + bs,
                w - 2.0 * bs,
                h - 2.0 * bs,
                (r - bs).max(0.0),
            );
            cr.set_fill_rule(cairo::FillRule::EvenOdd);
            let _ = cr.set_source(colors::gradient_pattern(&cols, deg as f64, x, y, w, h));
            let _ = cr.fill();
            cr.set_fill_rule(cairo::FillRule::Winding);
        }

        let (ix, iy, iw, ih) = (x + bs, y + bs, w - 2.0 * bs, h - 2.0 * bs);
        if iw <= 2.0 || ih <= 2.0 {
            return;
        }
        let _ = cr.save();
        rounded_rect(cr, ix, iy, iw, ih, (r - bs).max(0.0));
        cr.clip();
        let base = self
            .preset
            .preview
            .get("window")
            .and_then(Json::as_str)
            .and_then(hex)
            .unwrap_or([0.12, 0.13, 0.15, 1.0]);
        if op < 0.999 && blur {
            // frosted: a smoothed wash of the wallpaper behind the glass
            cr.set_source_rgba(avg[0], avg[1], avg[2], 0.85);
            let _ = cr.paint();
            cr.set_source_rgba(1.0, 1.0, 1.0, 0.08);
            let _ = cr.paint();
        }
        cr.set_source_rgba(base[0], base[1], base[2], op);
        let _ = cr.paint();
        let ink: Rgb = if luma(&base) > 0.6 {
            [0.15, 0.15, 0.18]
        } else {
            [0.92, 0.93, 0.95]
        };
        let accent =
            colors::parse_gradient(&self.get("general:col.active_border", "rgba(3584e4ff)"))
                .0
                .first()
                .copied()
                .unwrap_or([0.21, 0.52, 0.89, 1.0]);
        // title line + a few content lines
        cr.set_source_rgba(ink[0], ink[1], ink[2], 0.07);
        cr.rectangle(ix, iy, iw, 14.0 * s);
        let _ = cr.fill();
        let pad = 8.0 * s + (r - bs).max(0.0) * 0.3;
        let mut yy = iy + 22.0 * s;
        for (i, frac) in [0.5, 0.72, 0.38, 0.62, 0.45, 0.66, 0.3]
            .into_iter()
            .enumerate()
        {
            if yy + 5.0 * s > iy + ih - 6.0 * s {
                break;
            }
            let lw = (iw - 2.0 * pad) * frac;
            rounded_rect(cr, ix + pad, yy, lw.max(2.0), 4.5 * s, 2.25 * s);
            if i == 0 && active && luma(&accent) > 0.05 {
                cr.set_source_rgba(accent[0], accent[1], accent[2], 0.9 * op + 0.1);
            } else {
                cr.set_source_rgba(ink[0], ink[1], ink[2], 0.28 * op + 0.08);
            }
            let _ = cr.fill();
            yy += 11.0 * s;
        }
        if !active && truthy(&self.get("decoration:dim_inactive", "false")) {
            let dim =
                backend::parse_number(&self.get("decoration:dim_strength", "0.5")).unwrap_or(0.5);
            cr.set_source_rgba(0.0, 0.0, 0.0, dim.clamp(0.0, 1.0) * 0.7);
            let _ = cr.paint();
        }
        let _ = cr.restore();
    }
}
