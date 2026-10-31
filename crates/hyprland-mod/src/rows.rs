//! Preference rows generated from schema options.

use std::cell::{Cell, RefCell};
use std::f64::consts::PI;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib::{self, clone};

use crate::backend::WithClasses;
use crate::backend::{self, Cm, OptionSpec, OptionType};
use crate::colors::{self, Rgba};

/// One option row: the widget plus a way to re-read its value from the manager.
#[derive(Clone)]
pub struct OptionRow {
    pub key: String,
    pub row: adw::PreferencesRow,
    refresh: Rc<dyn Fn()>,
}

impl OptionRow {
    pub fn refresh(&self) {
        (self.refresh)()
    }
}

/// Shared controller state of every row type.
struct Logic {
    cm: Cm,
    opt: OptionSpec,
    updating: Cell<bool>,
    row: adw::PreferencesRow,
    reset_rev: gtk::Revealer,
}

impl Logic {
    fn new(cm: &Cm, opt: &OptionSpec, row: &impl IsA<adw::PreferencesRow>) -> Rc<Self> {
        let row = row.clone().upcast::<adw::PreferencesRow>();
        row.set_title(backend::opt_label(opt));
        if let (Some(desc), Some(ar)) = (
            backend::opt_description(opt),
            row.downcast_ref::<adw::ActionRow>(),
        ) {
            ar.set_subtitle(&glib::markup_escape_text(desc));
        }
        row.add_css_class("option-row");
        if opt.advanced {
            row.add_css_class("advanced-row");
        }
        let btn = gtk::Button::builder()
            .icon_name("edit-undo-symbolic")
            .valign(gtk::Align::Center)
            .tooltip_text("Reset — go back to the value from your own config")
            .build()
            .with_classes(&["flat", "circular", "reset-button"]);
        let reset_rev = gtk::Revealer::builder()
            .transition_type(gtk::RevealerTransitionType::SlideLeft)
            .transition_duration(200)
            .valign(gtk::Align::Center)
            .child(&btn)
            .build();
        let logic = Rc::new(Self {
            cm: cm.clone(),
            opt: opt.clone(),
            updating: Cell::new(false),
            row,
            reset_rev,
        });
        logic.row.set_tooltip_text(Some(&opt.key));
        btn.connect_clicked(clone!(
            #[weak]
            logic,
            move |_| {
                backend::update(&logic.cm, |c| c.reset(&logic.opt.key));
            }
        ));
        logic
    }

    fn commit(&self, text: String, reload: &dyn Fn()) {
        if self.updating.get() || text == backend::current_text(&self.cm, &self.opt) {
            return;
        }
        let res = backend::update(&self.cm, |c| c.set(&self.opt.key, text));
        if let Err(e) = res {
            reload();
            crate::window::toast_for(
                &self.row,
                &format!("Couldn't change “{}”: {e}", backend::opt_label(&self.opt)),
                3,
            );
        }
    }

    /// Run `load` with change signals muted, then show the modified state.
    fn refresh(&self, load: &dyn Fn()) {
        self.updating.set(true);
        load();
        self.updating.set(false);
        let modified = self.cm.borrow().is_modified(&self.opt.key);
        self.reset_rev.set_reveal_child(modified);
        if modified {
            self.row.add_css_class("modified");
        } else {
            self.row.remove_css_class("modified");
        }
    }

    fn text(&self) -> String {
        backend::current_text(&self.cm, &self.opt)
    }
}

fn finish(logic: Rc<Logic>, load: impl Fn() + 'static) -> OptionRow {
    let load: Rc<dyn Fn()> = Rc::new(load);
    let refresh: Rc<dyn Fn()> = {
        let logic = logic.clone();
        let load = load.clone();
        Rc::new(move || logic.refresh(&*load))
    };
    refresh();
    OptionRow {
        key: logic.opt.key.clone(),
        row: logic.row.clone(),
        refresh,
    }
}

// ------------------------------------------------------------------ switch

fn switch_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let row = adw::SwitchRow::new();
    let logic = Logic::new(cm, opt, &row);
    row.add_suffix(&logic.reset_rev);
    let load = clone!(
        #[weak]
        row,
        #[weak]
        logic,
        move || row.set_active(backend::current_bool(&logic.cm, &logic.opt))
    );
    row.connect_active_notify(clone!(
        #[weak]
        logic,
        move |r| logic.commit(backend::format_bool(r.is_active()), &|| ())
    ));
    finish(logic, load)
}

// ------------------------------------------------------------------ spin

fn spin_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let is_float = opt.kind == OptionType::Float;
    let lo = opt.min.unwrap_or(-1_000_000.0);
    let hi = opt.max.unwrap_or(1_000_000.0);
    let step = opt.step.unwrap_or(if is_float { 0.05 } else { 1.0 });
    let digits = opt.digits.unwrap_or(if is_float { 2 } else { 0 });
    let adj = gtk::Adjustment::new(lo, lo, hi, step, step * 10.0, 0.0);
    let row = adw::SpinRow::new(Some(&adj), 1.0, digits as u32);
    let logic = Logic::new(cm, opt, &row);
    if let Some(unit) = opt
        .unit
        .as_deref()
        .and_then(backend::non_empty)
        .map(str::to_string)
    {
        let u = unit.clone();
        row.connect_output(move |r| {
            let v = r.adjustment().value();
            r.set_text(&format!("{:.*} {}", r.digits() as usize, v, u));
            true
        });
        row.connect_input(move |r| {
            let txt = r.text().replace(&unit, "");
            Some(txt.trim().replace(',', ".").parse::<f64>().map_err(|_| ()))
        });
    }
    row.add_suffix(&logic.reset_rev);
    let load = clone!(
        #[weak]
        row,
        #[weak]
        logic,
        move || {
            if let Some(v) = backend::current_number(&logic.cm, &logic.opt) {
                row.set_value(v);
            }
        }
    );
    row.connect_value_notify(clone!(
        #[weak]
        logic,
        move |r| {
            let v = r.value();
            let text = if logic.opt.kind == OptionType::Int {
                backend::format_number(&logic.opt, v.round())
            } else {
                let p = 10f64.powi(r.digits().max(2) as i32);
                backend::format_number(&logic.opt, (v * p).round() / p)
            };
            logic.commit(text, &|| ());
        }
    ));
    finish(logic, load)
}

// ------------------------------------------------------------------ combo

fn same_choice(value: &str, text: &str) -> bool {
    if value.eq_ignore_ascii_case(text.trim()) {
        return true;
    }
    let is_bool_word = |s: &str| matches!(s.to_lowercase().as_str(), "true" | "false");
    is_bool_word(value)
        && backend::parse_bool(value) == backend::parse_bool(text)
        && backend::parse_bool(text).is_some()
}

fn combo_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let row = adw::ComboRow::new();
    let logic = Logic::new(cm, opt, &row);
    let values: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(
        backend::choices(opt)
            .iter()
            .map(backend::choice_value)
            .collect(),
    ));
    let labels: Vec<String> = backend::choices(opt)
        .iter()
        .map(backend::choice_label)
        .collect();
    let model = gtk::StringList::new(&labels.iter().map(String::as_str).collect::<Vec<_>>());
    row.set_model(Some(&model));
    row.add_suffix(&logic.reset_rev);
    let load = clone!(
        #[weak]
        row,
        #[weak]
        logic,
        #[strong]
        values,
        move || {
            let text = logic.text();
            let found = values.borrow().iter().position(|v| same_choice(v, &text));
            let i = found.unwrap_or_else(|| {
                values.borrow_mut().push(text.clone());
                model.append(&format!("Custom ({text})"));
                values.borrow().len() - 1
            });
            row.set_selected(i as u32);
        }
    );
    row.connect_selected_notify(clone!(
        #[weak]
        logic,
        #[strong]
        values,
        move |r| {
            let v = values.borrow().get(r.selected() as usize).cloned();
            if let Some(v) = v {
                logic.commit(v, &|| ());
            }
        }
    ));
    finish(logic, load)
}

// ------------------------------------------------------------------ color

fn color_button(title: Option<&str>) -> gtk::ColorDialogButton {
    let dialog = gtk::ColorDialog::builder().with_alpha(true).build();
    if let Some(t) = title {
        dialog.set_title(t);
    }
    let b = gtk::ColorDialogButton::new(Some(dialog));
    b.set_valign(gtk::Align::Center);
    b.add_css_class("color-swatch");
    b
}

fn color_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let row = adw::ActionRow::new();
    let logic = Logic::new(cm, opt, &row);
    let btn = color_button(Some(backend::opt_label(opt)));
    row.add_suffix(&btn);
    row.set_activatable_widget(Some(&btn));
    row.add_suffix(&logic.reset_rev);
    let load = clone!(
        #[weak]
        btn,
        #[weak]
        logic,
        move || btn.set_rgba(&colors::to_gdk(colors::parse_color_or(
            &logic.text(),
            [0.5, 0.5, 0.5, 1.0]
        )))
    );
    btn.connect_rgba_notify(clone!(
        #[weak]
        logic,
        move |b| logic.commit(colors::format_color(colors::from_gdk(&b.rgba())), &|| ())
    ));
    finish(logic, load)
}

// ------------------------------------------------------------------ gradient

pub fn rounded_rect(cr: &gtk::cairo::Context, x: f64, y: f64, w: f64, h: f64, r: f64) {
    let r = r.min(w / 2.0).min(h / 2.0).max(0.0);
    if r < 0.5 {
        cr.rectangle(x, y, w, h);
        return;
    }
    cr.new_sub_path();
    cr.arc(x + w - r, y + r, r, -PI / 2.0, 0.0);
    cr.arc(x + w - r, y + h - r, r, 0.0, PI / 2.0);
    cr.arc(x + r, y + h - r, r, PI / 2.0, PI);
    cr.arc(x + r, y + r, r, PI, 3.0 * PI / 2.0);
    cr.close_path();
}

/// Small checkerboard-backed preview of a gradient.
fn gradient_swatch(state: Rc<RefCell<(Vec<Rgba>, i32)>>) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::builder()
        .content_width(64)
        .content_height(24)
        .valign(gtk::Align::Center)
        .build()
        .with_classes(&["gradient-swatch"]);
    area.set_draw_func(move |_, cr, w, h| {
        let (w, h) = (w as f64, h as f64);
        rounded_rect(cr, 0.0, 0.0, w, h, 6.0);
        cr.clip();
        let sq = 6.0;
        let mut y = 0.0;
        while y < h {
            let mut x = 0.0;
            while x < w {
                let g = if ((x / sq) as i64 + (y / sq) as i64) % 2 == 1 {
                    0.85
                } else {
                    0.65
                };
                cr.set_source_rgb(g, g, g);
                cr.rectangle(x, y, sq, sq);
                let _ = cr.fill();
                x += sq;
            }
            y += sq;
        }
        let st = state.borrow();
        let pat = colors::gradient_pattern(&st.0, st.1 as f64, 0.0, 0.0, w, h);
        let _ = cr.set_source(&pat);
        let _ = cr.paint();
    });
    area
}

const MAX_COLORS: usize = 4;

struct Gradient {
    logic: Rc<Logic>,
    cols: RefCell<Vec<Rgba>>,
    deg: Cell<i32>,
    shown: Rc<RefCell<(Vec<Rgba>, i32)>>,
    swatch: gtk::DrawingArea,
    buttons_box: gtk::Box,
    buttons: RefCell<Vec<gtk::ColorDialogButton>>,
    add_btn: gtk::Button,
    rm_btn: gtk::Button,
    angle: gtk::SpinButton,
    angle_rev: gtk::Revealer,
}

impl Gradient {
    fn set_swatch(&self) {
        *self.shown.borrow_mut() = (self.cols.borrow().clone(), self.deg.get());
        self.swatch.queue_draw();
    }

    fn emit(self: &Rc<Self>, force: bool) {
        if self.logic.updating.get() {
            return;
        }
        if !force {
            self.deg.set(self.angle.value() as i32);
        }
        let text = colors::format_gradient(&self.cols.borrow(), self.deg.get());
        let me = self.clone();
        self.logic.commit(text, &move || me.load());
        self.set_swatch();
    }

    fn rebuild_buttons(self: &Rc<Self>) {
        while let Some(c) = self.buttons_box.first_child() {
            self.buttons_box.remove(&c);
        }
        let mut buttons = vec![];
        for (i, col) in self.cols.borrow().iter().enumerate() {
            let b = color_button(None);
            b.set_rgba(&colors::to_gdk(*col));
            let weak = Rc::downgrade(self);
            b.connect_rgba_notify(move |b| {
                let Some(me) = weak.upgrade() else { return };
                if me.logic.updating.get() || i >= me.cols.borrow().len() {
                    return;
                }
                me.cols.borrow_mut()[i] = colors::from_gdk(&b.rgba());
                me.emit(false);
            });
            self.buttons_box.append(&b);
            buttons.push(b);
        }
        *self.buttons.borrow_mut() = buttons;
    }

    fn load(self: &Rc<Self>) {
        let (cols, deg) = colors::parse_gradient(&self.logic.text());
        let n = cols.len();
        let same_len = n == self.cols.borrow().len();
        *self.cols.borrow_mut() = cols.clone();
        if same_len {
            for (b, c) in self.buttons.borrow().iter().zip(&cols) {
                b.set_rgba(&colors::to_gdk(*c));
            }
        } else {
            self.rebuild_buttons();
        }
        self.deg.set(deg);
        self.angle.set_value(deg as f64);
        self.angle_rev.set_reveal_child(n > 1);
        self.rm_btn.set_visible(n > 1);
        self.add_btn.set_sensitive(n < MAX_COLORS);
        self.set_swatch();
    }
}

fn gradient_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let row = adw::ActionRow::new();
    let logic = Logic::new(cm, opt, &row);
    let bx = gtk::Box::builder()
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    let shown = Rc::new(RefCell::new((vec![[0.5, 0.5, 0.5, 1.0]], 0)));
    let swatch = gradient_swatch(shown.clone());
    bx.append(&swatch);
    let buttons_box = gtk::Box::builder()
        .spacing(4)
        .build()
        .with_classes(&["linked-swatches"]);
    bx.append(&buttons_box);
    let small = |icon: &str, tip: &str| {
        gtk::Button::builder()
            .icon_name(icon)
            .tooltip_text(tip)
            .build()
            .with_classes(&["flat", "circular", "small-icon-button"])
    };
    let add_btn = small("list-add-symbolic", "Add color");
    let rm_btn = small("list-remove-symbolic", "Remove last color");
    bx.append(&rm_btn);
    bx.append(&add_btn);
    let angle = gtk::SpinButton::builder()
        .adjustment(&gtk::Adjustment::new(0.0, 0.0, 360.0, 15.0, 45.0, 0.0))
        .valign(gtk::Align::Center)
        .tooltip_text("Gradient angle")
        .width_chars(3)
        .build();
    angle.connect_output(|sb| {
        sb.set_text(&format!("{}°", sb.value() as i32));
        glib::Propagation::Stop
    });
    angle.connect_input(|sb| {
        Some(
            sb.text()
                .replace('°', "")
                .trim()
                .parse::<f64>()
                .map_err(|_| ()),
        )
    });
    let angle_rev = gtk::Revealer::builder()
        .transition_type(gtk::RevealerTransitionType::SlideLeft)
        .transition_duration(200)
        .child(&angle)
        .build();
    bx.append(&angle_rev);
    row.add_suffix(&bx);
    row.add_suffix(&logic.reset_rev);

    let g = Rc::new(Gradient {
        logic: logic.clone(),
        cols: RefCell::new(vec![]),
        deg: Cell::new(0),
        shown,
        swatch,
        buttons_box,
        buttons: RefCell::new(vec![]),
        add_btn: add_btn.clone(),
        rm_btn: rm_btn.clone(),
        angle: angle.clone(),
        angle_rev,
    });
    angle.connect_value_notify(clone!(
        #[weak]
        g,
        move |_| g.emit(false)
    ));
    add_btn.connect_clicked(clone!(
        #[weak]
        g,
        move |_| {
            {
                let mut cols = g.cols.borrow_mut();
                if cols.len() >= MAX_COLORS {
                    return;
                }
                let last = *cols.last().unwrap_or(&[0.5, 0.5, 0.5, 1.0]);
                cols.push(last);
                if cols.len() == 2 && g.deg.get() == 0 {
                    g.deg.set(45);
                }
            }
            g.emit(true);
        }
    ));
    rm_btn.connect_clicked(clone!(
        #[weak]
        g,
        move |_| {
            if g.cols.borrow().len() > 1 {
                g.cols.borrow_mut().pop();
                g.emit(true);
            }
        }
    ));
    // the closure below keeps the controller alive as long as the row handle
    finish(logic, move || g.load())
}

// ------------------------------------------------------------------ entry

fn valid_vec2(text: &str) -> bool {
    let parts: Vec<&str> = text.split_whitespace().collect();
    parts.len() == 2 && parts.iter().all(|p| p.parse::<f64>().is_ok())
}

fn entry_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    let row = adw::EntryRow::builder().show_apply_button(true).build();
    let logic = Logic::new(cm, opt, &row);
    if let Some(desc) = backend::opt_description(opt) {
        row.set_tooltip_text(Some(&format!("{desc}\n{}", opt.key)));
    }
    row.add_suffix(&logic.reset_rev);
    let load = clone!(
        #[weak]
        row,
        #[weak]
        logic,
        move || row.set_text(&logic.text())
    );
    row.connect_apply(clone!(
        #[weak]
        logic,
        move |r| {
            let text = r.text().trim().to_string();
            if logic.opt.kind == OptionType::Vec2 && !valid_vec2(&text) {
                r.add_css_class("error");
                return;
            }
            logic.commit(text, &|| ());
        }
    ));
    row.connect_changed(|r| r.remove_css_class("error"));
    finish(logic, load)
}

pub fn build_row(cm: &Cm, opt: &OptionSpec) -> OptionRow {
    match opt.kind {
        OptionType::Bool => switch_row(cm, opt),
        OptionType::Int | OptionType::Float => spin_row(cm, opt),
        OptionType::Enum if !backend::choices(opt).is_empty() => combo_row(cm, opt),
        OptionType::Color => color_row(cm, opt),
        OptionType::Gradient => gradient_row(cm, opt),
        _ => entry_row(cm, opt),
    }
}
