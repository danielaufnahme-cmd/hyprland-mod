//! Main application window.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use gtk::glib::{self, clone};

use crate::apply_bar::ApplyBar;
use crate::backend::WithClasses;
use crate::backend::{self, Cm, OptionSpec, Page};
use crate::backups_page::BackupsPage;
use crate::search_page::SearchPage;
use crate::settings_page::SettingsPage;
use crate::sidebar::{page_info, Sidebar, FIXED_PAGES};
use crate::{presets_page, transfer_page};

/// Pages of the second half of the rewrite: listed, but not built yet.
const COMING_SOON: &[&str] = &["bar", "keybinds", "rules", "monitors"];

/// Show a toast in the window that contains `widget` (if any).
pub fn toast_for(widget: &impl IsA<gtk::Widget>, text: &str, timeout: u32) {
    let overlay = widget
        .as_ref()
        .root()
        .and_downcast::<adw::ApplicationWindow>()
        .and_then(|w| w.content())
        .and_downcast::<adw::ToastOverlay>();
    if let Some(o) = overlay {
        o.add_toast(
            adw::Toast::builder()
                .title(glib::markup_escape_text(text))
                .timeout(timeout)
                .build(),
        );
    }
}

fn status_content(title: &str, description: &str, icon: &str) -> adw::ToolbarView {
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&adw::HeaderBar::new());
    let page = adw::StatusPage::builder()
        .title(title)
        .description(description)
        .icon_name(icon)
        .build();
    tv.set_content(Some(&page));
    tv
}

fn coming_soon(title: &str, icon: &str) -> gtk::Widget {
    adw::StatusPage::builder()
        .icon_name(icon)
        .title(title)
        .description("Coming soon")
        .build()
        .with_classes(&["compact", "dim-label"])
        .upcast()
}

pub struct MainWindow {
    pub window: adw::ApplicationWindow,
    pub state: Option<Rc<WindowState>>,
}

pub struct WindowState {
    window: adw::ApplicationWindow,
    cm: Cm,
    all_opts: HashMap<String, OptionSpec>,
    titles: HashMap<String, String>,
    page_specs: HashMap<String, Page>,
    current: RefCell<String>,
    search_timer: RefCell<Option<glib::SourceId>>,
    toasts: adw::ToastOverlay,
    stack: gtk::Stack,
    settings_pages: RefCell<Vec<SettingsPage>>,
    search_page: SearchPage,
    backups_page: BackupsPage,
    pub apply_bar: ApplyBar,
    title_widget: adw::WindowTitle,
    content_page: adw::NavigationPage,
    sidebar: OnceCell<Sidebar>,
    split: adw::NavigationSplitView,
}

impl MainWindow {
    pub fn new(app: &adw::Application, loaded: Result<(Cm, Vec<Page>), String>) -> Self {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title("Hyprland Mod")
            .default_width(1120)
            .default_height(780)
            .build();
        window.set_size_request(360, 480);

        let (cm, schema) = match loaded {
            Ok(v) => v,
            Err(e) => {
                window.set_content(Some(&status_content(
                    "Hyprland Mod Couldn't Start",
                    &glib::markup_escape_text(&e),
                    "dialog-error-symbolic",
                )));
                return Self {
                    window,
                    state: None,
                };
            }
        };
        let running = cm.borrow().hyprland_running();
        if !running && !backend::is_dry_run() {
            window.set_content(Some(&status_content(
                "Hyprland Mod Only Works on Hyprland",
                "Log in to a Hyprland session and open Hyprland Mod again.",
                "computer-fail-symbolic",
            )));
            return Self {
                window,
                state: None,
            };
        }
        let state = WindowState::new(&window, cm, schema);
        Self {
            window,
            state: Some(state),
        }
    }
}

impl WindowState {
    fn new(window: &adw::ApplicationWindow, cm: Cm, mut schema: Vec<Page>) -> Rc<Self> {
        schema.sort_by(|a, b| {
            a.order
                .total_cmp(&b.order)
                .then_with(|| a.title.cmp(&b.title))
        });
        let all_opts: HashMap<String, OptionSpec> = schema
            .iter()
            .flat_map(|p| &p.groups)
            .flat_map(|g| &g.options)
            .map(|o| (o.key.clone(), o.clone()))
            .collect();
        let mut titles: HashMap<String, String> = schema
            .iter()
            .map(|p| {
                (
                    p.id.clone(),
                    if p.title.is_empty() {
                        p.id.clone()
                    } else {
                        p.title.clone()
                    },
                )
            })
            .collect();
        titles.extend(
            FIXED_PAGES
                .iter()
                .map(|(id, t, _)| (id.to_string(), t.to_string())),
        );
        let current = schema
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| "backups".into());
        let page_specs = schema.iter().map(|p| (p.id.clone(), p.clone())).collect();

        let toasts = adw::ToastOverlay::new();
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(180)
            .vexpand(true)
            .hexpand(true)
            .build();

        let state = Rc::new_cyclic(|weak: &Weak<WindowState>| {
            let w = weak.clone();
            let open: Rc<dyn Fn(&str)> = Rc::new(move |pid: &str| {
                if let Some(s) = w.upgrade() {
                    s.open_page(pid);
                }
            });
            let search_page = SearchPage::new(&cm, schema.clone(), open, page_info());
            stack.add_named(&search_page.widget, Some("search"));
            stack.add_named(&presets_page::build(cm.clone(), &toasts), Some("presets"));
            stack.add_named(&transfer_page::build(cm.clone(), &toasts), Some("transfer"));
            let backups_page = BackupsPage::new(&cm, &toasts);
            stack.add_named(&backups_page.widget, Some("backups"));

            let (w1, w2) = (weak.clone(), weak.clone());
            let apply_bar = ApplyBar::new(
                move || {
                    if let Some(s) = w1.upgrade() {
                        s.discard();
                    }
                },
                move || {
                    if let Some(s) = w2.upgrade() {
                        s.apply();
                    }
                },
            );
            apply_bar.set_margin_bottom(18);
            let overlay = gtk::Overlay::builder().child(&stack).build();
            overlay.add_overlay(&apply_bar);

            let title_widget = adw::WindowTitle::new("", "");
            let header = adw::HeaderBar::builder()
                .title_widget(&title_widget)
                .build();
            let undo_btn = gtk::Button::builder()
                .icon_name("edit-undo-symbolic")
                .tooltip_text("Undo (Ctrl+Z)")
                .action_name("win.undo")
                .build();
            let redo_btn = gtk::Button::builder()
                .icon_name("edit-redo-symbolic")
                .tooltip_text("Redo (Ctrl+Shift+Z)")
                .action_name("win.redo")
                .build();
            header.pack_start(&undo_btn);
            header.pack_start(&redo_btn);
            let content_tv = adw::ToolbarView::builder().content(&overlay).build();
            content_tv.add_top_bar(&header);
            let content_page = adw::NavigationPage::builder()
                .title("Settings")
                .tag("content")
                .child(&content_tv)
                .build();

            let split = adw::NavigationSplitView::builder()
                .content(&content_page)
                .min_sidebar_width(230.0)
                .max_sidebar_width(290.0)
                .sidebar_width_fraction(0.26)
                .build();

            WindowState {
                window: window.clone(),
                cm: cm.clone(),
                all_opts,
                titles,
                page_specs,
                current: RefCell::new(current.clone()),
                search_timer: RefCell::new(None),
                toasts: toasts.clone(),
                stack: stack.clone(),
                settings_pages: RefCell::new(vec![]),
                search_page,
                backups_page,
                apply_bar,
                title_widget,
                content_page,
                sidebar: OnceCell::new(),
                split,
            }
        });

        let menu = gio::Menu::new();
        menu.append(Some("Keyboard Shortcuts"), Some("win.show-help-overlay"));
        menu.append(Some("About Hyprland Mod"), Some("app.about"));
        let (w1, w2) = (Rc::downgrade(&state), Rc::downgrade(&state));
        let sidebar = Sidebar::new(
            &schema,
            move |pid| {
                if let Some(s) = w1.upgrade() {
                    s.on_sidebar_select(pid);
                }
            },
            move |text| {
                if let Some(s) = w2.upgrade() {
                    s.on_search_changed(text);
                }
            },
            &menu,
        );
        state.split.set_sidebar(Some(&sidebar.page));
        sidebar.search.connect_activate(clone!(
            #[weak(rename_to = split)]
            state.split,
            move |_| split.set_show_content(true)
        ));
        let _ = state.sidebar.set(sidebar);

        toasts.set_child(Some(&state.split));
        window.set_content(Some(&toasts));

        let bp = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 680sp").expect("breakpoint"),
        );
        bp.add_setter(&state.split, "collapsed", Some(&true.to_value()));
        window.add_breakpoint(bp);

        state.install_actions();
        let weak = Rc::downgrade(&state);
        // every mutation goes through backend::update, which delivers events with no borrow held
        cm.borrow_mut().subscribe(Box::new(move |_event, key| {
            if let Some(s) = weak.upgrade() {
                s.on_cm_event(key);
            }
        }));
        state.show(&current);
        state.sidebar().select(Some(&current));
        state.sync_state();
        state
    }

    fn sidebar(&self) -> &Sidebar {
        self.sidebar.get().expect("sidebar")
    }

    // -- actions ---------------------------------------------------------

    fn install_actions(self: &Rc<Self>) {
        let add = |name: &str, f: fn(&Rc<WindowState>)| {
            let act = gio::SimpleAction::new(name, None);
            let weak = Rc::downgrade(self);
            act.connect_activate(move |_, _| {
                if let Some(s) = weak.upgrade() {
                    f(&s);
                }
            });
            self.window.add_action(&act);
        };
        add("undo", |s| s.history(true));
        add("redo", |s| s.history(false));
        add("apply", |s| s.apply());
        add("discard", |s| s.discard());
        add("search", |s| s.focus_search());
        add("show-help-overlay", |s| s.show_shortcuts());
        if let Some(app) = self.window.application() {
            app.set_accels_for_action("win.undo", &["<Control>z"]);
            app.set_accels_for_action("win.redo", &["<Control><Shift>z", "<Control>y"]);
            app.set_accels_for_action("win.apply", &["<Control>s"]);
            app.set_accels_for_action("win.search", &["<Control>f"]);
        }
    }

    fn set_enabled(&self, name: &str, on: bool) {
        if let Some(a) = self
            .window
            .lookup_action(name)
            .and_downcast::<gio::SimpleAction>()
        {
            a.set_enabled(on);
        }
    }

    pub fn toast(&self, text: &str, timeout: u32) {
        self.toasts.add_toast(
            adw::Toast::builder()
                .title(glib::markup_escape_text(text))
                .timeout(timeout)
                .build(),
        );
    }

    fn history(&self, undo: bool) {
        let res = backend::update(&self.cm, |c| if undo { c.undo() } else { c.redo() });
        self.on_cm_event(None);
        if !res.ok {
            let which = if undo { "undo" } else { "redo" };
            let msg = res
                .errors
                .first()
                .cloned()
                .unwrap_or_else(|| format!("Couldn't {which}"));
            self.toast(&msg, 3);
        } else {
            self.toast(if undo { "Undone" } else { "Redone" }, 2);
        }
    }

    fn show_shortcuts(&self) {
        let dlg = adw::ShortcutsDialog::new();
        let sec = adw::ShortcutsSection::new(Some("General"));
        for (title, accel) in [
            ("Search", "<Control>f"),
            ("Apply changes", "<Control>s"),
            ("Undo", "<Control>z"),
            ("Redo", "<Control><Shift>z"),
        ] {
            sec.add(adw::ShortcutsItem::new(title, accel));
        }
        dlg.add(sec);
        dlg.present(Some(&self.window));
    }

    fn focus_search(&self) {
        self.split.set_show_content(false);
        self.sidebar().search.grab_focus();
    }

    // -- navigation ------------------------------------------------------

    fn ensure_page(&self, pid: &str) {
        if self.stack.child_by_name(pid).is_some() {
            return;
        }
        if COMING_SOON.contains(&pid) {
            let (title, icon) = FIXED_PAGES
                .iter()
                .find(|p| p.0 == pid)
                .map(|p| (p.1, p.2))
                .unwrap_or(("", "preferences-other-symbolic"));
            self.stack.add_named(&coming_soon(title, icon), Some(pid));
            return;
        }
        if let Some(spec) = self.page_specs.get(pid) {
            let sp = SettingsPage::new(&self.cm, spec, &self.all_opts);
            self.stack.add_named(&sp.widget, Some(pid));
            self.settings_pages.borrow_mut().push(sp);
        }
    }

    fn show(&self, pid: &str) {
        self.ensure_page(pid);
        self.stack.set_visible_child_name(pid);
        let title = if pid == "search" {
            "Search".to_string()
        } else {
            self.titles
                .get(pid)
                .cloned()
                .unwrap_or_else(|| pid.to_string())
        };
        self.title_widget.set_title(&title);
        self.title_widget.set_subtitle("");
        self.content_page.set_title(&title);
    }

    fn on_sidebar_select(&self, pid: &str) {
        *self.current.borrow_mut() = pid.to_string();
        let search = &self.sidebar().search;
        if !search.text().is_empty() {
            search.set_text("");
        }
        self.show(pid);
        self.split.set_show_content(true);
    }

    pub fn open_page(&self, pid: &str) {
        self.sidebar().search.set_text("");
        *self.current.borrow_mut() = pid.to_string();
        self.sidebar().select(Some(pid));
        self.show(pid);
        self.split.set_show_content(true);
    }

    fn on_search_changed(self: &Rc<Self>, text: &str) {
        if let Some(id) = self.search_timer.borrow_mut().take() {
            id.remove();
        }
        if text.trim().is_empty() {
            self.search_page.set_query("");
            let cur = self.current.borrow().clone();
            self.sidebar().select(Some(&cur));
            self.show(&cur);
            return;
        }
        let weak = Rc::downgrade(self);
        let text = text.to_string();
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(120), move || {
            if let Some(s) = weak.upgrade() {
                s.search_timer.borrow_mut().take();
                s.run_search(&text);
            }
        });
        *self.search_timer.borrow_mut() = Some(id);
    }

    fn run_search(&self, text: &str) {
        self.search_page.set_query(text);
        self.sidebar().select(None);
        self.show("search");
    }

    // -- state -----------------------------------------------------------

    fn on_cm_event(&self, key: Option<&str>) {
        let matches = |k: &str| key.is_none_or(|key| key == k);
        for sp in self.settings_pages.borrow().iter() {
            for row in sp.rows.iter().filter(|r| matches(&r.key)) {
                row.refresh();
            }
            if let Some(p) = &sp.preview {
                if p.handles(key) {
                    p.update();
                }
            }
        }
        let search_rows = self.search_page.rows.borrow().clone();
        for row in search_rows.iter().filter(|r| matches(&r.key)) {
            row.refresh();
        }
        if key.is_none() {
            self.backups_page.refresh();
        }
        self.sync_state();
    }

    fn sync_state(&self) {
        let (undo, redo) = {
            let cm = self.cm.borrow();
            (cm.can_undo(), cm.can_redo())
        };
        self.set_enabled("undo", undo);
        self.set_enabled("redo", redo);
        let n = backend::pending_count(&self.cm);
        self.set_enabled("apply", n > 0);
        self.set_enabled("discard", n > 0);
        self.apply_bar.set_count(n);
    }

    pub fn discard(&self) {
        if !self.cm.borrow().is_dirty() {
            return;
        }
        backend::update(&self.cm, |c| c.discard());
        self.on_cm_event(None);
        self.toast("Changes discarded", 2);
    }

    pub fn apply(&self) {
        if !self.cm.borrow().is_dirty() {
            return;
        }
        let res = backend::update(&self.cm, |c| c.apply());
        self.on_cm_event(None);
        if res.ok && res.errors.is_empty() {
            let title = if res.live_applied {
                "Changes applied"
            } else {
                "Saved — takes effect next time Hyprland starts"
            };
            self.toast(title, 2);
        } else {
            let errors = if res.errors.is_empty() {
                vec!["Unknown error".to_string()]
            } else {
                res.errors
            };
            self.error_toast(errors);
        }
    }

    fn error_toast(&self, errors: Vec<String>) {
        let title = if errors.len() == 1 {
            format!("Couldn't apply: {}", errors[0])
        } else {
            format!("Couldn't apply {} changes", errors.len())
        };
        let toast = adw::Toast::builder()
            .title(glib::markup_escape_text(&title))
            .timeout(6)
            .button_label("Details")
            .priority(adw::ToastPriority::High)
            .build();
        let window = self.window.clone();
        toast.connect_button_clicked(move |_| {
            let body: Vec<String> = errors
                .iter()
                .map(|e| format!("• {}", glib::markup_escape_text(e)))
                .collect();
            let dlg = adw::AlertDialog::builder()
                .heading("Some Changes Weren't Applied")
                .body(body.join("\n"))
                .body_use_markup(true)
                .build();
            dlg.add_response("ok", "OK");
            dlg.present(Some(&window));
        });
        self.toasts.add_toast(toast);
    }

    // -- dev helpers -------------------------------------------------------

    pub fn dev_prepare(
        self: &Rc<Self>,
        page: Option<&str>,
        search: Option<&str>,
        sets: &[(String, String)],
        advanced: bool,
    ) {
        if let Some(page) = page {
            self.open_page(page);
            if COMING_SOON.contains(&page) {
                let weak = Rc::downgrade(self);
                glib::idle_add_local_once(move || {
                    if let Some(s) = weak.upgrade() {
                        s.sidebar().scroll_to_end();
                    }
                });
            }
        }
        for (k, v) in sets {
            let res = backend::update(&self.cm, |c| c.set(k, v.clone()));
            if let Err(e) = res {
                eprintln!("--set {k}: {e}");
            }
        }
        if advanced {
            let cur = self.current.borrow().clone();
            for sp in self.settings_pages.borrow().iter() {
                if sp.page_id == cur {
                    if let Some(sw) = &sp.advanced_switch {
                        sw.set_active(true);
                    }
                }
            }
        }
        if let Some(q) = search {
            self.sidebar().search.set_text(q);
            self.run_search(q);
        }
    }
}
