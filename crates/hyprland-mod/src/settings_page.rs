//! A preferences page generated from one schema page.

use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::backend::WithClasses;
use crate::backend::{self, Cm, OptionSpec, Page};
use crate::preview::LivePreview;
use crate::rows::{build_row, OptionRow};

const PREVIEW_PAGES: &[&str] = &["general", "decoration"];

/// Room below the last group so the floating apply bar never hides a row.
pub fn bottom_spacer() -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    g.set_size_request(-1, 40);
    g
}

pub struct SettingsPage {
    pub widget: gtk::Widget,
    pub page_id: String,
    pub rows: Vec<OptionRow>,
    pub preview: Option<LivePreview>,
    pub advanced_switch: Option<adw::SwitchRow>,
}

fn set_show_advanced(widgets: &[gtk::Widget], show: bool, animate: bool) {
    for w in widgets {
        w.set_visible(show);
        w.remove_css_class("reveal-in");
        if show && animate {
            w.add_css_class("reveal-in");
        }
    }
}

impl SettingsPage {
    pub fn new(cm: &Cm, page: &Page, all_opts: &HashMap<String, OptionSpec>) -> Self {
        let prefs = adw::PreferencesPage::new();
        if let Some(d) = backend::non_empty(&page.description) {
            prefs.set_description(d);
        }

        let mut preview = None;
        if PREVIEW_PAGES.contains(&page.id.as_str()) {
            let p = LivePreview::new(cm, all_opts.clone());
            let frame = gtk::Box::builder()
                .orientation(gtk::Orientation::Vertical)
                .overflow(gtk::Overflow::Hidden)
                .build()
                .with_classes(&["preview-card"]);
            let ov = gtk::Overlay::builder().child(&p.area).build();
            let badge = gtk::Label::builder()
                .label("Live preview")
                .halign(gtk::Align::Start)
                .valign(gtk::Align::End)
                .can_target(false)
                .build()
                .with_classes(&["preview-badge"]);
            ov.add_overlay(&badge);
            frame.append(&ov);
            let group = adw::PreferencesGroup::builder()
                .build()
                .with_classes(&["preview-group"]);
            group.add(&frame);
            prefs.add(&group);
            preview = Some(p);
        }

        let mut rows = vec![];
        let mut advanced: Vec<gtk::Widget> = vec![];
        let mut n_advanced = 0;
        for g in &page.groups {
            let group = adw::PreferencesGroup::builder()
                .title(glib::markup_escape_text(&g.title))
                .build();
            if let Some(d) = backend::non_empty(&g.description) {
                group.set_description(Some(&glib::markup_escape_text(d)));
            }
            for opt in &g.options {
                let row = build_row(cm, opt);
                group.add(&row.row);
                if opt.advanced {
                    advanced.push(row.row.clone().upcast());
                    n_advanced += 1;
                }
                rows.push(row);
            }
            if !g.options.is_empty() && g.options.iter().all(|o| o.advanced) {
                advanced.push(group.clone().upcast());
            }
            prefs.add(&group);
        }

        let mut advanced_switch = None;
        if n_advanced > 0 {
            let adv = adw::PreferencesGroup::builder()
                .build()
                .with_classes(&["advanced-toggle-group"]);
            let sw = adw::SwitchRow::builder()
                .title("Show advanced options")
                .subtitle(format!(
                    "{n_advanced} rarely needed settings for fine-tuning"
                ))
                .build();
            sw.add_prefix(&gtk::Image::from_icon_name("emblem-system-symbolic"));
            set_show_advanced(&advanced, false, false);
            let advanced = Rc::new(advanced);
            sw.connect_active_notify(move |r| set_show_advanced(&advanced, r.is_active(), true));
            adv.add(&sw);
            prefs.add(&adv);
            advanced_switch = Some(sw);
        }
        prefs.add(&bottom_spacer());

        Self {
            widget: prefs.upcast(),
            page_id: page.id.clone(),
            rows,
            preview,
            advanced_switch,
        }
    }
}
