//! Sidebar: title, search and page list.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio};

use crate::backend::Page;
use crate::backend::WithClasses;

/// Pages that are not generated from the schema: (id, title, icon).
pub const FIXED_PAGES: &[(&str, &str, &str)] = &[
    ("presets", "Presets", "starred-symbolic"),
    ("bar", "Bar", "view-continuous-symbolic"),
    (
        "keybinds",
        "Keyboard Shortcuts",
        "preferences-desktop-keyboard-symbolic",
    ),
    ("rules", "Window Rules", "focus-windows-symbolic"),
    ("monitors", "Displays", "view-dual-symbolic"),
    ("transfer", "Import & Export", "document-send-symbolic"),
    ("backups", "Backups", "document-open-recent-symbolic"),
];

/// What global search matches for the fixed pages: (id, description, keywords).
const KEYWORDS: &[(&str, &str, &str)] = &[
    (
        "presets",
        "Ready-made looks and your saved presets",
        "theme look style gallery",
    ),
    (
        "bar",
        "Design and run your top bar",
        "waybar panel status clock tray widgets modules",
    ),
    (
        "keybinds",
        "Add shortcuts that launch apps, switch workspaces and more",
        "keybind keybinding bind hotkey key shortcut keyboard launch",
    ),
    (
        "rules",
        "Make apps open floating, sized or on a workspace",
        "window rule windowrule float size workspace opacity app class",
    ),
    (
        "monitors",
        "Arrange screens, resolution, refresh rate and scale",
        "display monitor screen resolution refresh rate hz scale scaling rotation arrange",
    ),
    (
        "transfer",
        "Move your setup to another computer",
        "import export share file",
    ),
    (
        "backups",
        "Snapshots of your configuration",
        "backup restore snapshot undo",
    ),
];

#[derive(Clone)]
pub struct PageInfo {
    pub id: &'static str,
    pub title: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub keywords: &'static str,
}

pub fn page_info() -> Vec<PageInfo> {
    FIXED_PAGES
        .iter()
        .map(|&(id, title, icon)| {
            let (description, keywords) = KEYWORDS
                .iter()
                .find(|k| k.0 == id)
                .map(|k| (k.1, k.2))
                .unwrap_or(("", ""));
            PageInfo {
                id,
                title,
                icon,
                description,
                keywords,
            }
        })
        .collect()
}

fn icon_or_fallback(name: &str) -> String {
    let has = gdk::Display::default()
        .map(|d| gtk::IconTheme::for_display(&d).has_icon(name))
        .unwrap_or(false);
    if !name.is_empty() && has {
        name.to_string()
    } else {
        "preferences-other-symbolic".to_string()
    }
}

pub struct Sidebar {
    pub page: adw::NavigationPage,
    pub search: gtk::SearchEntry,
    list: gtk::ListBox,
    scroller: gtk::ScrolledWindow,
    ids: Vec<String>,
    silent: Rc<Cell<bool>>,
}

impl Sidebar {
    pub fn new(
        pages: &[Page],
        on_select: impl Fn(&str) + 'static,
        on_search: impl Fn(&str) + 'static,
        menu_model: &gio::Menu,
    ) -> Self {
        let header = adw::HeaderBar::new();
        header.set_title_widget(Some(&adw::WindowTitle::new("Hyprland Mod", "")));
        let menu = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(menu_model)
            .tooltip_text("Main Menu")
            .primary(true)
            .build();
        header.pack_end(&menu);

        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search settings")
            .hexpand(true)
            .build()
            .with_classes(&["sidebar-search"]);
        search.connect_search_changed(move |e| on_search(&e.text()));
        search.connect_stop_search(|e| e.set_text(""));
        let search_box = gtk::Box::builder()
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .build();
        search_box.append(&search);

        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .build();
        list.add_css_class("navigation-sidebar");
        let mut ids = vec![];
        let mut add_row = |pid: &str, title: &str, icon: &str| {
            let bx = gtk::Box::builder().spacing(12).build();
            bx.append(&gtk::Image::from_icon_name(icon));
            bx.append(
                &gtk::Label::builder()
                    .label(title)
                    .xalign(0.0)
                    .hexpand(true)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .build(),
            );
            let row = gtk::ListBoxRow::builder()
                .child(&bx)
                .build()
                .with_classes(&["sidebar-row"]);
            list.append(&row);
            ids.push(pid.to_string());
        };
        for p in pages {
            let title = if p.title.is_empty() { &p.id } else { &p.title };
            add_row(&p.id, title, &icon_or_fallback(&p.icon));
        }
        let n_schema = pages.len() as i32;
        for (pid, title, icon) in FIXED_PAGES {
            add_row(pid, title, icon);
        }
        let first_fixed = n_schema;
        list.set_header_func(move |row, _before| {
            if row.index() == first_fixed && first_fixed > 0 {
                let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
                sep.add_css_class("sidebar-separator");
                row.set_header(Some(&sep));
            } else {
                row.set_header(gtk::Widget::NONE);
            }
        });
        let silent = Rc::new(Cell::new(false));
        let ids_sel: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(ids.clone()));
        let silent_sel = silent.clone();
        list.connect_row_selected(move |_, row| {
            let Some(row) = row else { return };
            if silent_sel.get() {
                return;
            }
            let id = ids_sel.borrow().get(row.index() as usize).cloned();
            if let Some(id) = id {
                on_select(&id);
            }
        });

        let scroller = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        let tv = adw::ToolbarView::new();
        tv.add_top_bar(&header);
        tv.add_top_bar(&search_box);
        tv.set_content(Some(&scroller));
        let page = adw::NavigationPage::builder()
            .title("Hyprland Mod")
            .tag("sidebar")
            .child(&tv)
            .build();
        Self {
            page,
            search,
            list,
            scroller,
            ids,
            silent,
        }
    }

    /// Highlight a page without triggering navigation (None clears the selection).
    pub fn select(&self, pid: Option<&str>) {
        self.silent.set(true);
        match pid.and_then(|p| self.ids.iter().position(|i| i == p)) {
            Some(i) => {
                if let Some(row) = self.list.row_at_index(i as i32) {
                    self.list.select_row(Some(&row));
                }
            }
            None => self.list.unselect_all(),
        }
        self.silent.set(false);
    }

    pub fn scroll_to_end(&self) {
        let adj = self.scroller.vadjustment();
        adj.set_value(adj.upper() - adj.page_size());
    }
}
