//! Import & Export: save the whole setup to a file, or bring one in.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::backend::presets::SETUP_EXT;
use crate::backend::WithClasses;
use crate::backend::{self, Change, Cm, ConfigManager, Json, PresetStore};
use crate::presets_page::{dismissed, file_filters, file_name, load_css, toast, window_of};
use crate::settings_page::bottom_spacer;

const SHOWN_CHANGES: usize = 60;

fn short(text: Option<&str>) -> String {
    let Some(text) = text else {
        return "your own config".into();
    };
    if text.chars().count() <= 38 {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(36).collect::<String>())
    }
}

fn icon_tile(name: &str) -> gtk::Image {
    gtk::Image::builder()
        .icon_name(name)
        .valign(gtk::Align::Center)
        .build()
        .with_classes(&["transfer-icon"])
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n != 1 { "s" } else { "" })
}

struct Inner {
    cm: Cm,
    toasts: adw::ToastOverlay,
    store: PresetStore,
    root: adw::Bin,
    export_row: adw::ActionRow,
}

type Page = Rc<Inner>;

pub fn build(cm: Rc<RefCell<ConfigManager>>, toasts: &adw::ToastOverlay) -> gtk::Widget {
    load_css();
    let store = PresetStore::for_manager(&cm.borrow());
    let prefs = adw::PreferencesPage::builder()
        .description(
            "Move your whole Hyprland Mod setup to another computer, keep a copy somewhere safe, or try someone else’s.",
        )
        .build();
    let root = adw::Bin::builder().child(&prefs).build();

    let export = adw::PreferencesGroup::builder()
        .title("Export Your Setup")
        .build();
    let export_row = adw::ActionRow::builder().title("Save to a file").build();
    export_row.add_prefix(&icon_tile("document-save-symbolic"));
    let ebtn = gtk::Button::builder()
        .label("Export…")
        .valign(gtk::Align::Center)
        .build()
        .with_classes(&["suggested-action"]);
    export_row.add_suffix(&ebtn);
    export_row.set_activatable_widget(Some(&ebtn));
    export.add(&export_row);
    prefs.add(&export);

    let imp = adw::PreferencesGroup::builder()
        .title("Import a Setup")
        .description(
            "A backup of your current configuration is made first, so you can always go back from the \
             Backups page. Imported settings are staged: review them in the preview, then press Apply — \
             or Discard to cancel.",
        )
        .build();
    let row = adw::ActionRow::builder()
        .title("Open a .hyprmod file")
        .subtitle(
            "Replaces everything you changed with Hyprland Mod; your own settings stay as they are",
        )
        .build();
    row.add_prefix(&icon_tile("document-open-symbolic"));
    let ibtn = gtk::Button::builder()
        .label("Import…")
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&ibtn);
    row.set_activatable_widget(Some(&ibtn));
    imp.add(&row);
    prefs.add(&imp);

    let info = adw::PreferencesGroup::builder()
        .title("What’s Included")
        .build();
    for (icon, title, sub) in [
        (
            "preferences-desktop-appearance-symbolic",
            "Settings changed in Hyprland Mod",
            "Look, input, layouts and everything else you set here — including unapplied changes",
        ),
        (
            "media-playback-start-symbolic",
            "Animation curves and speeds",
            "Bezier and animation lines",
        ),
        (
            "security-high-symbolic",
            "Nothing that runs programs",
            "Commands, key bindings and startup apps are never exported or imported",
        ),
    ] {
        let r = adw::ActionRow::builder().title(title).subtitle(sub).build();
        r.add_prefix(
            &gtk::Image::builder()
                .icon_name(icon)
                .valign(gtk::Align::Center)
                .build(),
        );
        info.add(&r);
    }
    prefs.add(&info);
    prefs.add(&bottom_spacer());

    let page: Page = Rc::new(Inner {
        cm: cm.clone(),
        toasts: toasts.clone(),
        store,
        root: root.clone(),
        export_row,
    });
    let w = Rc::downgrade(&page);
    ebtn.connect_clicked(move |_| {
        if let Some(p) = w.upgrade() {
            p.export_dialog();
        }
    });
    let w = Rc::downgrade(&page);
    ibtn.connect_clicked(move |_| {
        if let Some(p) = w.upgrade() {
            p.import_dialog();
        }
    });
    let w = Rc::downgrade(&page);
    cm.borrow_mut().subscribe(Box::new(move |_, _| {
        if let Some(p) = w.upgrade() {
            p.update_summary();
        }
    }));
    page.update_summary();
    // the widget keeps the page alive
    let strong = page.clone();
    root.connect_map(move |_| strong.update_summary());
    root.upcast()
}

impl Inner {
    fn update_summary(&self) {
        let Ok(cm) = self.cm.try_borrow() else { return };
        let state = cm.export_state();
        drop(cm);
        let n = state
            .get("settings")
            .and_then(Json::as_object)
            .map_or(0, |m| m.len());
        let lines: usize = state.get("lists").and_then(Json::as_object).map_or(0, |m| {
            m.values()
                .map(|v| v.as_array().map_or(0, |a| a.len()))
                .sum()
        });
        let text = if n == 0 && lines == 0 {
            "Nothing changed with Hyprland Mod yet — the file would be empty".to_string()
        } else if lines > 0 {
            format!(
                "{} and {}",
                plural(n, "setting"),
                plural(lines, "animation line")
            )
        } else {
            plural(n, "setting")
        };
        self.export_row.set_subtitle(&text);
    }

    // -- export ----------------------------------------------------------

    fn export_dialog(self: &Rc<Self>) {
        let date = glib::DateTime::now_local()
            .ok()
            .and_then(|d| d.format("%Y-%m-%d").ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let dlg = gtk::FileDialog::builder()
            .title("Export Setup")
            .initial_name(format!("hyprland-mod-setup-{date}{SETUP_EXT}"))
            .filters(&file_filters(&[("Hyprland Mod setups", &["*.hyprmod"])]))
            .build();
        let w = Rc::downgrade(self);
        dlg.save(
            window_of(&self.root).as_ref(),
            gio::Cancellable::NONE,
            move |res| {
                let Some(page) = w.upgrade() else { return };
                match res {
                    Err(e) if !dismissed(&e) => {
                        toast(
                            &page.toasts,
                            &format!("Couldn't export: {}", e.message()),
                            5,
                        );
                    }
                    Err(_) => {}
                    Ok(f) => {
                        if let Some(path) = f.path() {
                            page.export_to(&path);
                        }
                    }
                }
            },
        );
    }

    fn export_to(&self, path: &Path) -> bool {
        if let Err(e) = self.store.export_setup(&self.cm.borrow(), path) {
            toast(&self.toasts, &format!("Couldn't export: {e}"), 5);
            return false;
        }
        toast(
            &self.toasts,
            &format!("Setup exported to {}", file_name(path)),
            3,
        );
        true
    }

    // -- import ----------------------------------------------------------

    fn import_dialog(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::builder()
            .title("Import Setup")
            .filters(&file_filters(&[
                ("Hyprland Mod setups", &["*.hyprmod", "*.json"]),
                ("All files", &["*"]),
            ]))
            .build();
        let w = Rc::downgrade(self);
        dlg.open(
            window_of(&self.root).as_ref(),
            gio::Cancellable::NONE,
            move |res| {
                let Some(page) = w.upgrade() else { return };
                match res {
                    Err(e) if !dismissed(&e) => {
                        toast(
                            &page.toasts,
                            &format!("Couldn't open the file: {}", e.message()),
                            5,
                        );
                    }
                    Err(_) => {}
                    Ok(f) => {
                        if let Some(path) = f.path() {
                            page.review(&path);
                        }
                    }
                }
            },
        );
    }

    fn alert(&self, heading: &str, body: &str) -> adw::AlertDialog {
        let dlg = adw::AlertDialog::new(Some(heading), Some(body));
        dlg.add_response("ok", "OK");
        dlg.present(Some(&self.root));
        dlg
    }

    /// Validate the file and show what would change before anything is staged.
    fn review(self: &Rc<Self>, path: &Path) -> adw::AlertDialog {
        let state = match self.store.read_setup(path) {
            Ok(s) => s,
            Err(e) => return self.alert("Couldn't Import Setup", &e.to_string()),
        };
        let changes = self.store.setup_changes(&self.cm.borrow(), &state);
        if changes.is_empty() {
            return self.alert(
                "Nothing to Import",
                "This setup is the same as what you have now.",
            );
        }
        let n = changes.len();
        let mut body = format!(
            "{} will change. A backup is made first, and nothing is saved until you press Apply.",
            plural(n, "setting")
        );
        if self.cm.borrow().is_dirty() {
            body.push_str(" Your unapplied changes will be replaced.");
        }
        let dlg = adw::AlertDialog::new(Some("Import This Setup?"), Some(&body));
        dlg.set_extra_child(Some(&change_list(&changes)));
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("import", "Import");
        dlg.set_response_appearance("import", adw::ResponseAppearance::Suggested);
        dlg.set_default_response(Some("import"));
        dlg.set_close_response("cancel");
        let w = Rc::downgrade(self);
        dlg.connect_response(None, move |_, r| {
            if let Some(p) = w.upgrade().filter(|_| r == "import") {
                p.import(state.clone(), n);
            }
        });
        dlg.present(Some(&self.root));
        dlg
    }

    fn import(&self, state: Json, n: usize) -> bool {
        if let Err(e) = backend::update(&self.cm, |c| self.store.import_setup(c, state)) {
            self.alert("Couldn't Import Setup", &e.to_string());
            return false;
        }
        toast(
            &self.toasts,
            &format!("Imported {} — Apply to keep them", plural(n, "change")),
            5,
        );
        true
    }
}

fn change_list(changes: &[Change]) -> gtk::ScrolledWindow {
    let lb = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build()
        .with_classes(&["boxed-list", "change-list"]);
    for ch in changes.iter().take(SHOWN_CHANGES) {
        let b = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .build();
        let name = gtk::Label::builder()
            .label(&ch.label)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .tooltip_text(&ch.key)
            .build()
            .with_classes(&["heading"]);
        b.append(&name);
        let vals = gtk::Box::builder().spacing(6).build();
        let value = |text: Option<&str>, class: &str| {
            gtk::Label::builder()
                .label(short(text))
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .build()
                .with_classes(&[class, "mono-value"])
        };
        let arrow = gtk::Image::builder()
            .icon_name("go-next-symbolic")
            .pixel_size(12)
            .build()
            .with_classes(&["dim-label"]);
        vals.append(&value(ch.old.as_deref(), "change-old"));
        vals.append(&arrow);
        vals.append(&value(ch.new.as_deref(), "change-new"));
        b.append(&vals);
        lb.append(&b);
    }
    if changes.len() > SHOWN_CHANGES {
        let more = gtk::Label::builder()
            .label(format!("and {} more", changes.len() - SHOWN_CHANGES))
            .xalign(0.0)
            .build()
            .with_classes(&["dim-label"]);
        lb.append(&more);
    }
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(300)
        .child(&lb)
        .build()
}
