//! Backups: create snapshots and restore them.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib::{self, clone};

use crate::backend::WithClasses;
use crate::backend::{self, Backup, Cm};

/// Friendly "Today at 14:05" style timestamp.
pub fn when(created: &str) -> String {
    let parsed = created
        .trim()
        .parse::<f64>()
        .ok()
        .and_then(|ts| glib::DateTime::from_unix_local(ts as i64).ok())
        .or_else(|| {
            glib::DateTime::from_iso8601(created.trim(), Some(&glib::TimeZone::local())).ok()
        })
        .and_then(|d| d.to_local().ok());
    let (Some(dt), Ok(now)) = (parsed, glib::DateTime::now_local()) else {
        return created.to_string();
    };
    let ymd = |d: &glib::DateTime| (d.year(), d.day_of_year());
    let fmt = |f: &str| dt.format(f).map(|s| s.to_string()).unwrap_or_default();
    let day = if ymd(&dt) == ymd(&now) {
        "Today".to_string()
    } else if now.add_days(-1).is_ok_and(|y| ymd(&y) == ymd(&dt)) {
        "Yesterday".to_string()
    } else if dt.year() == now.year() {
        fmt("%A, %-d %B")
    } else {
        fmt("%-d %B %Y")
    };
    format!("{day} at {}", fmt("%H:%M"))
}

struct Inner {
    cm: Cm,
    toasts: adw::ToastOverlay,
    label_row: adw::EntryRow,
    list_group: adw::PreferencesGroup,
    rows: RefCell<Vec<adw::ActionRow>>,
}

#[derive(Clone)]
pub struct BackupsPage {
    pub widget: adw::Bin,
    inner: Rc<Inner>,
}

fn toast(toasts: &adw::ToastOverlay, text: &str) {
    let t = adw::Toast::builder()
        .title(glib::markup_escape_text(text))
        .timeout(3)
        .build();
    toasts.add_toast(t);
}

impl BackupsPage {
    pub fn new(cm: &Cm, toasts: &adw::ToastOverlay) -> Self {
        let prefs = adw::PreferencesPage::builder()
            .description(
                "A backup is a snapshot of your whole Hyprland configuration. Restore one any time to go back.",
            )
            .build();
        let widget = adw::Bin::builder().child(&prefs).build();

        let create = adw::PreferencesGroup::builder().title("New Backup").build();
        let label_row = adw::EntryRow::builder().title("Name (optional)").build();
        let btn = gtk::Button::builder()
            .label("Create Backup")
            .valign(gtk::Align::Center)
            .build()
            .with_classes(&["suggested-action"]);
        label_row.add_suffix(&btn);
        create.add(&label_row);
        prefs.add(&create);

        let list_group = adw::PreferencesGroup::builder()
            .title("Saved Backups")
            .build();
        prefs.add(&list_group);

        let inner = Rc::new(Inner {
            cm: cm.clone(),
            toasts: toasts.clone(),
            label_row: label_row.clone(),
            list_group,
            rows: RefCell::new(vec![]),
        });
        let page = Self { widget, inner };
        label_row.connect_entry_activated(clone!(
            #[strong]
            page,
            move |_| page.create()
        ));
        btn.connect_clicked(clone!(
            #[strong]
            page,
            move |_| page.create()
        ));
        page.refresh();
        page
    }

    pub fn refresh(&self) {
        let inner = &self.inner;
        for r in inner.rows.borrow_mut().drain(..) {
            inner.list_group.remove(&r);
        }
        let backups: Vec<Backup> = inner.cm.borrow().list_backups();
        let n = backups.len();
        let desc = (n > 0).then(|| format!("{n} backup{}", if n != 1 { "s" } else { "" }));
        inner.list_group.set_description(desc.as_deref());
        if backups.is_empty() {
            let row = adw::ActionRow::builder()
                .title("No backups yet")
                .subtitle("Create one before experimenting — it only takes a second")
                .build()
                .with_classes(&["empty-row"]);
            row.add_prefix(&gtk::Image::from_icon_name("document-open-recent-symbolic"));
            inner.list_group.add(&row);
            inner.rows.borrow_mut().push(row);
            return;
        }
        for b in backups {
            let label = b.label.trim().to_string();
            let nfiles = b.files.len();
            let when = when(&b.created);
            let files = format!("{nfiles} file{}", if nfiles != 1 { "s" } else { "" });
            let subtitle = if label.is_empty() {
                files
            } else {
                format!("{when} · {files}")
            };
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(if label.is_empty() {
                    &when
                } else {
                    &label
                }))
                .subtitle(glib::markup_escape_text(&subtitle))
                .build();
            let icon = gtk::Image::builder()
                .icon_name("document-open-recent-symbolic")
                .build()
                .with_classes(&["backup-icon"]);
            row.add_prefix(&icon);
            let restore = gtk::Button::builder()
                .label("Restore")
                .valign(gtk::Align::Center)
                .build();
            let id = b.id.clone();
            restore.connect_clicked(clone!(
                #[strong(rename_to = page)]
                self,
                move |btn| page.confirm(btn, &id, &when)
            ));
            row.add_suffix(&restore);
            inner.list_group.add(&row);
            inner.rows.borrow_mut().push(row);
        }
    }

    fn create(&self) {
        let inner = &self.inner;
        let label = inner.label_row.text().trim().to_string();
        let res = backend::update(&inner.cm, |c| c.create_backup(&label));
        if let Err(e) = res {
            toast(&inner.toasts, &format!("Couldn't create backup: {e}"));
            return;
        }
        inner.label_row.set_text("");
        self.refresh();
        toast(&inner.toasts, "Backup created");
    }

    fn confirm(&self, from: &gtk::Button, id: &str, when: &str) {
        let mut body = format!("Your configuration will be replaced with the backup from {when}.");
        if self.inner.cm.borrow().is_dirty() {
            body.push_str(" Unsaved changes will be lost.");
        }
        let dlg = adw::AlertDialog::new(Some("Restore Backup?"), Some(&body));
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("restore", "Restore");
        dlg.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        dlg.set_default_response(Some("cancel"));
        dlg.set_close_response("cancel");
        let id = id.to_string();
        dlg.connect_response(
            None,
            clone!(
                #[strong(rename_to = page)]
                self,
                move |_, r| {
                    if r == "restore" {
                        page.restore(&id);
                    }
                }
            ),
        );
        dlg.present(Some(from));
    }

    fn restore(&self, id: &str) {
        let inner = &self.inner;
        let res = backend::update(&inner.cm, |c| c.restore_backup(id));
        if !res.ok {
            let msg = match res.errors.first() {
                Some(e) => format!("Couldn't restore backup: {e}"),
                None => "Couldn't restore backup".to_string(),
            };
            toast(&inner.toasts, &msg);
            return;
        }
        self.refresh();
        toast(&inner.toasts, "Backup restored");
    }
}
