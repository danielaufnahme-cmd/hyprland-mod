//! Presets: a gallery of ready-made looks plus the user's own saved presets.

#[path = "preset_thumb.rs"]
mod preset_thumb;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use crate::backend::presets::{LIST_KEYWORDS, PRESET_EXT};
use crate::backend::WithClasses;
use crate::backend::{self, Cm, ConfigManager, Json, Preset, PresetStore};
use crate::preview::LivePreview;

const STYLE: &str = include_str!("../resources/presets.css");

const FILTERS: &[(&str, &str)] = &[
    ("all", "All"),
    ("dark", "Dark"),
    ("light", "Light"),
    ("minimal", "Minimal"),
    ("colorful", "Colorful"),
    ("animated", "Animated"),
    ("fast", "Fast"),
];

thread_local! {
    static CSS_LOADED: Cell<bool> = const { Cell::new(false) };
}

/// Install resources/presets.css once per process (shared with the transfer page).
pub fn load_css() {
    if CSS_LOADED.get() {
        return;
    }
    let Some(display) = gdk::Display::default() else {
        return;
    };
    CSS_LOADED.set(true);
    let provider = gtk::CssProvider::new();
    provider.load_from_string(STYLE);
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

pub fn toast(overlay: &adw::ToastOverlay, text: &str, timeout: u32) -> adw::Toast {
    let t = adw::Toast::builder()
        .title(glib::markup_escape_text(text))
        .timeout(timeout)
        .build();
    overlay.add_toast(t.clone());
    t
}

pub fn toast_button(
    overlay: &adw::ToastOverlay,
    text: &str,
    timeout: u32,
    button: &str,
    on_button: impl Fn() + 'static,
) {
    let t = adw::Toast::builder()
        .title(glib::markup_escape_text(text))
        .timeout(timeout)
        .button_label(button)
        .build();
    t.connect_button_clicked(move |_| on_button());
    overlay.add_toast(t);
}

/// ``[(name, patterns)]`` as the list model Gtk.FileDialog wants.
pub fn file_filters(filters: &[(&str, &[&str])]) -> gio::ListStore {
    let store = gio::ListStore::new::<gtk::FileFilter>();
    for (name, patterns) in filters {
        let f = gtk::FileFilter::new();
        f.set_name(Some(name));
        for p in *patterns {
            f.add_pattern(p);
        }
        store.append(&f);
    }
    store
}

pub fn dismissed(e: &glib::Error) -> bool {
    e.matches(gtk::DialogError::Dismissed)
}

pub fn window_of(widget: &impl IsA<gtk::Widget>) -> Option<gtk::Window> {
    widget.as_ref().root().and_downcast::<gtk::Window>()
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Stage the manager state `before` again (the Undo of a toast).
fn restore_state(cm: &Cm, before: &Json) {
    if let Err(e) = backend::update(cm, |c| c.import_state(before.clone())) {
        eprintln!("hyprland-mod: undo failed: {e}");
    }
}

// ------------------------------------------------------------------ cards

#[derive(Clone, Copy, PartialEq)]
enum CardState {
    Current,
    Trying,
}

struct Card {
    child: gtk::FlowBoxChild,
    card: gtk::Box,
    badge: gtk::Label,
    preset: Rc<Preset>,
}

impl Card {
    fn new(preset: Rc<Preset>, menu: Option<gtk::MenuButton>) -> Self {
        let p = &preset;
        let child = gtk::FlowBoxChild::builder()
            .tooltip_text(if p.description.is_empty() {
                format!("Try “{}”", p.name)
            } else {
                p.description.clone()
            })
            .build()
            .with_classes(&["preset-child"]);
        child.update_property(&[gtk::accessible::Property::Label(&format!(
            "{}. {}",
            p.name, p.description
        ))]);

        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .width_request(200)
            .build()
            .with_classes(&["preset-card"]);
        let ov = gtk::Overlay::builder()
            .child(&preset_thumb::thumbnail(preset.clone()))
            .build();
        let badge = gtk::Label::builder()
            .label("In use")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::Start)
            .can_target(false)
            .visible(false)
            .build()
            .with_classes(&["preset-badge"]);
        ov.add_overlay(&badge);
        card.append(&ov);

        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(4)
            .margin_start(14)
            .margin_end(10)
            .margin_top(10)
            .margin_bottom(14)
            .build();
        let top = gtk::Box::builder().spacing(6).build();
        let name = gtk::Label::builder()
            .label(&p.name)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .max_width_chars(10)
            .build()
            .with_classes(&["preset-name"]);
        top.append(&name);
        if let Some(menu) = menu {
            top.append(&menu);
        }
        body.append(&top);
        if !p.description.is_empty() {
            let desc = gtk::Label::builder()
                .label(&p.description)
                .xalign(0.0)
                .wrap(true)
                .lines(2)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .natural_wrap_mode(gtk::NaturalWrapMode::Word)
                .width_chars(12)
                .max_width_chars(12)
                .build()
                .with_classes(&["preset-desc"]);
            body.append(&desc);
        }
        let mut tags: Vec<&str> = p.tags.iter().take(4).map(String::as_str).collect();
        if tags.is_empty() && !p.builtin && !p.original() {
            tags.push("mine");
        }
        if !tags.is_empty() || p.original() {
            let wrap = adw::WrapBox::builder()
                .child_spacing(4)
                .line_spacing(4)
                .margin_top(4)
                .natural_line_length(160)
                .build();
            if p.original() {
                let orig = gtk::Label::new(Some("Original")).with_classes(&["preset-original-tag"]);
                wrap.append(&orig);
            }
            for t in tags {
                wrap.append(&gtk::Label::new(Some(t)).with_classes(&["preset-tag"]));
            }
            body.append(&wrap);
        }
        card.append(&body);
        child.set_child(Some(&card));
        Card {
            child,
            card,
            badge,
            preset,
        }
    }

    fn set_state(&self, state: Option<CardState>) {
        let was = self.badge.is_visible();
        self.badge.set_visible(state.is_some());
        match state {
            Some(s) => {
                self.badge.set_label(if s == CardState::Trying {
                    "Previewing"
                } else {
                    "In use"
                });
                if !was {
                    self.badge.remove_css_class("pop");
                    self.badge.add_css_class("pop");
                }
                self.card.add_css_class("current");
            }
            None => self.card.remove_css_class("current"),
        }
    }

    fn matches_filter(&self, tag: &str, query: &str) -> bool {
        let p = &self.preset;
        if p.original() {
            return true; // pinned: always reachable
        }
        if tag != "all" && !p.tags.iter().any(|t| t == tag) {
            return false;
        }
        if query.is_empty() {
            return true;
        }
        let mut hay = format!("{} {} {}", p.name, p.description, p.author);
        for t in &p.tags {
            hay.push(' ');
            hay.push_str(t);
        }
        let hay = hay.to_lowercase();
        query.split_whitespace().all(|w| hay.contains(w))
    }
}

fn section_header(title: &str, count: Option<&gtk::Label>) -> gtk::Box {
    let b = gtk::Box::builder()
        .spacing(6)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .build();
    let lbl = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .build()
        .with_classes(&["presets-section-title"]);
    b.append(&lbl);
    if let Some(c) = count {
        c.add_css_class("presets-section-count");
        c.set_valign(gtk::Align::Center);
        b.append(c);
    }
    b
}

fn grid() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .column_spacing(18)
        .row_spacing(18)
        .activate_on_single_click(true)
        .valign(gtk::Align::Start)
        .build()
        .with_classes(&["preset-grid"])
}

// ------------------------------------------------------------------ page

struct Inner {
    cm: Cm,
    toasts: adw::ToastOverlay,
    store: PresetStore,
    managed_keys: Vec<String>,
    root: adw::Bin,
    tag: RefCell<String>,
    query: RefCell<String>,
    refresh_queued: Cell<bool>,
    preview: LivePreview,
    back_banner: gtk::Box,
    original_box: gtk::Box,
    original_grid: gtk::FlowBox,
    original_card: RefCell<Option<Card>>,
    builtin_count: gtk::Label,
    builtin_grid: gtk::FlowBox,
    no_match: adw::StatusPage,
    cards: RefCell<Vec<Card>>,
    user_count: gtk::Label,
    user_grid: gtk::FlowBox,
    user_empty: gtk::Box,
    user_cards: RefCell<Vec<Card>>,
}

type Page = Rc<Inner>;
type CardAction = fn(&Page, &Rc<Preset>);

pub fn build(cm: Rc<RefCell<ConfigManager>>, toasts: &adw::ToastOverlay) -> gtk::Widget {
    load_css();
    let store = PresetStore::for_manager(&cm.borrow());
    let opts: HashMap<_, _> = store
        .options
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let (builtin, builtin_error) = match store.load_builtin() {
        Ok(b) => (b, None),
        // a broken install: show the rest of the page anyway
        Err(e) => (vec![], Some(e.to_string())),
    };
    let managed_keys = store.managed_look_keys();

    let root = adw::Bin::new();
    let outer = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(22)
        .margin_top(24)
        .margin_bottom(72)
        .margin_start(18)
        .margin_end(18)
        .build();
    let clamp = adw::Clamp::builder()
        .maximum_size(1080)
        .tightening_threshold(860)
        .child(&outer)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .vexpand(true)
        .build();
    root.set_child(Some(&scroller));

    // header
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build()
        .with_classes(&["presets-header"]);
    head.append(
        &gtk::Label::builder()
            .label("Presets")
            .xalign(0.0)
            .build()
            .with_classes(&["title-1"]),
    );
    head.append(
        &gtk::Label::builder()
            .label("Pick a look to preview it instantly. Press Apply to keep it, or Discard to go back.")
            .xalign(0.0)
            .wrap(true)
            .build()
            .with_classes(&["dim-label"]),
    );
    outer.append(&head);

    // the way back: shown whenever Hyprland Mod overrides the user's own look
    let (back_banner, back_btn) = back_banner();
    outer.append(&back_banner);

    // live preview
    let preview = LivePreview::new(&cm, opts);
    preview.area.set_content_height(190);
    let frame = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .overflow(gtk::Overflow::Hidden)
        .build()
        .with_classes(&["presets-preview"]);
    let pov = gtk::Overlay::builder().child(&preview.area).build();
    pov.add_overlay(
        &gtk::Label::builder()
            .label("Live preview")
            .halign(gtk::Align::Start)
            .valign(gtk::Align::End)
            .can_target(false)
            .build()
            .with_classes(&["preview-badge"]),
    );
    frame.append(&pov);
    outer.append(&frame);

    // pinned original look (first on the page, outside the filters)
    let original_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .visible(false)
        .build();
    original_box.append(&section_header("Your Original Look", None));
    let original_grid = grid();
    original_grid.set_halign(gtk::Align::Start); // one card: keep it card-sized
    original_box.append(&original_grid);
    outer.append(&original_box);

    // search + filter chips
    let bar = adw::WrapBox::builder()
        .child_spacing(6)
        .line_spacing(8)
        .build();
    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search presets")
        .width_chars(12)
        .margin_end(8)
        .build();
    search.update_property(&[gtk::accessible::Property::Label("Search presets")]);
    bar.append(&search);
    let mut chips = vec![];
    for (i, (tag, label)) in FILTERS.iter().enumerate() {
        let chip = gtk::ToggleButton::builder()
            .label(*label)
            .valign(gtk::Align::Center)
            .build()
            .with_classes(&["preset-chip", "flat"]);
        if i == 0 {
            chip.set_active(true);
        } else {
            chip.set_group(chips.first().map(|(_, c): &(&str, gtk::ToggleButton)| c));
        }
        bar.append(&chip);
        chips.push((*tag, chip));
    }
    outer.append(&bar);

    // built-in section
    let builtin_count = gtk::Label::new(None);
    outer.append(&section_header("Built-in", Some(&builtin_count)));
    let builtin_grid = grid();
    outer.append(&builtin_grid);
    let no_match = adw::StatusPage::builder()
        .icon_name("edit-find-symbolic")
        .title("No Matching Presets")
        .description("Try another word or filter")
        .visible(false)
        .build()
        .with_classes(&["compact"]);
    outer.append(&no_match);

    // user section
    let mine_head = gtk::Box::builder().spacing(6).margin_top(10).build();
    let user_count = gtk::Label::new(None);
    mine_head.append(&section_header("My Presets", Some(&user_count)));
    let imp = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("document-open-symbolic")
                .label("Import…")
                .build(),
        )
        .tooltip_text("Add a preset someone shared with you (.hyprmod-preset)")
        .build();
    let save = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("document-save-symbolic")
                .label("Save Current Look…")
                .build(),
        )
        .tooltip_text("Save your current look as a preset you can reuse or share")
        .build()
        .with_classes(&["suggested-action"]);
    mine_head.append(&imp);
    mine_head.append(&save);
    outer.append(&mine_head);
    let user_grid = grid();
    outer.append(&user_grid);
    let user_empty = empty_state();
    outer.append(&user_empty);

    let page: Page = Rc::new(Inner {
        cm: cm.clone(),
        toasts: toasts.clone(),
        store,
        managed_keys,
        root: root.clone(),
        tag: RefCell::new("all".into()),
        query: RefCell::new(String::new()),
        refresh_queued: Cell::new(false),
        preview,
        back_banner,
        original_box,
        original_grid,
        original_card: RefCell::new(None),
        builtin_count,
        builtin_grid,
        no_match,
        cards: RefCell::new(vec![]),
        user_count,
        user_grid,
        user_empty,
        user_cards: RefCell::new(vec![]),
    });

    // signals (weak: the map handler below keeps the page alive with its widget)
    let weak = Rc::downgrade(&page);
    let with = move |f: fn(&Page)| {
        let w = weak.clone();
        move || {
            if let Some(p) = w.upgrade() {
                f(&p);
            }
        }
    };
    let back = with(Inner::go_back_to_original);
    back_btn.connect_clicked(move |_| back());
    let im = with(Inner::import_dialog);
    imp.connect_clicked(move |_| im());
    let sv = with(Inner::save_dialog);
    save.connect_clicked(move |_| sv());
    let w = Rc::downgrade(&page);
    search.connect_search_changed(move |e| {
        if let Some(p) = w.upgrade() {
            *p.query.borrow_mut() = e.text().trim().to_lowercase();
            p.apply_filter();
        }
    });
    for (tag, chip) in chips {
        let w = Rc::downgrade(&page);
        chip.connect_toggled(move |b| {
            if let Some(p) = w.upgrade().filter(|_| b.is_active()) {
                *p.tag.borrow_mut() = tag.to_string();
                p.apply_filter();
            }
        });
    }
    let w = Rc::downgrade(&page);
    page.original_grid.connect_child_activated(move |_, _| {
        if let Some(p) = w.upgrade() {
            p.go_back_to_original();
        }
    });
    for g in [&page.builtin_grid, &page.user_grid] {
        let w = Rc::downgrade(&page);
        g.connect_child_activated(move |_, child| {
            let Some(p) = w.upgrade() else { return };
            if let Some(preset) = p.preset_for(child) {
                p.try_preset(&preset);
            }
        });
        let w = Rc::downgrade(&page);
        g.set_filter_func(move |child| w.upgrade().is_none_or(|p| p.filter(child)));
    }

    for preset in builtin {
        let card = Card::new(Rc::new(preset), None);
        page.builtin_grid.append(&card.child);
        page.cards.borrow_mut().push(card);
    }

    page.store.ensure_original(&cm.borrow()); // first launch: remember the user's own look
    page.reload_user();

    let w = Rc::downgrade(&page);
    cm.borrow_mut().subscribe(Box::new(move |_event, _key| {
        if let Some(p) = w.upgrade() {
            p.queue_refresh();
        }
    }));
    let strong = page.clone();
    root.connect_map(move |_| {
        strong.preview.update();
        strong.refresh_state();
    });
    if let Some(e) = builtin_error {
        let t = toasts.clone();
        glib::idle_add_local_once(move || {
            toast(&t, &format!("Couldn't load built-in presets: {e}"), 3);
        });
    }
    root.upcast()
}

fn back_banner() -> (gtk::Box, gtk::Button) {
    let b = gtk::Box::builder()
        .spacing(12)
        .visible(false)
        .build()
        .with_classes(&["original-banner"]);
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text.append(
        &gtk::Label::builder()
            .label("Want your own look back?")
            .xalign(0.0)
            .build()
            .with_classes(&["heading"]),
    );
    text.append(
        &gtk::Label::builder()
            .label("Removes Hyprland Mod's look overrides so your own config takes effect again.")
            .xalign(0.0)
            .wrap(true)
            .build()
            .with_classes(&["dim-label"]),
    );
    b.append(&text);
    let btn = gtk::Button::builder()
        .child(
            &adw::ButtonContent::builder()
                .icon_name("edit-undo-symbolic")
                .label("Back to my original look")
                .build(),
        )
        .valign(gtk::Align::Center)
        .tooltip_text("Stage your own setup again (Apply to keep it)")
        .build()
        .with_classes(&["suggested-action"]);
    b.append(&btn);
    (b, btn)
}

fn empty_state() -> gtk::Box {
    let b = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .halign(gtk::Align::Fill)
        .build()
        .with_classes(&["preset-empty"]);
    b.append(
        &gtk::Image::builder()
            .icon_name("starred-symbolic")
            .pixel_size(32)
            .build(),
    );
    b.append(
        &gtk::Label::builder()
            .label("No presets of your own yet")
            .build()
            .with_classes(&["heading"]),
    );
    b.append(
        &gtk::Label::builder()
            .label("Save your current look to reuse it later or share it with friends.")
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build()
            .with_classes(&["dim-label"]),
    );
    b
}

impl Inner {
    // -- building blocks -------------------------------------------------

    fn user_menu_button(self: &Rc<Self>, preset: &Rc<Preset>) -> gtk::MenuButton {
        let menu = gio::Menu::new();
        menu.append(Some("Export / Share…"), Some("card.export"));
        menu.append(Some("Rename…"), Some("card.rename"));
        if !preset.original() {
            let section = gio::Menu::new();
            section.append(Some("Delete"), Some("card.delete"));
            menu.append_section(None, &section);
        }
        let btn = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .valign(gtk::Align::Center)
            .tooltip_text("More")
            .build()
            .with_classes(&["flat", "circular", "preset-menu-button"]);
        let group = gio::SimpleActionGroup::new();
        let actions: [(&str, CardAction); 3] = [
            ("export", Self::export_dialog),
            ("rename", Self::rename_dialog),
            ("delete", Self::delete_dialog),
        ];
        for (name, f) in actions {
            let act = gio::SimpleAction::new(name, None);
            let (w, p) = (Rc::downgrade(self), preset.clone());
            act.connect_activate(move |_, _| {
                if let Some(page) = w.upgrade() {
                    f(&page, &p);
                }
            });
            group.add_action(&act);
        }
        btn.insert_action_group("card", Some(&group));
        btn
    }

    fn reload_user(self: &Rc<Self>) {
        for c in self.user_cards.borrow().iter() {
            self.user_grid.remove(&c.child);
        }
        if let Some(c) = self.original_card.borrow_mut().take() {
            self.original_grid.remove(&c.child);
        }
        let (user, problems) = self.store.load_user();
        let mut fresh = vec![];
        let mut original = None;
        for p in user {
            let p = Rc::new(p);
            let card = Card::new(p.clone(), Some(self.user_menu_button(&p)));
            if p.original() {
                self.original_grid.append(&card.child);
                original = Some(card);
            } else {
                self.user_grid.append(&card.child);
                fresh.push(card);
            }
        }
        self.original_box.set_visible(original.is_some());
        *self.original_card.borrow_mut() = original;
        let n = fresh.len();
        *self.user_cards.borrow_mut() = fresh;
        self.user_grid.set_visible(n > 0);
        self.user_empty.set_visible(n == 0);
        self.user_count.set_label(&n.to_string());
        self.user_count.set_visible(n > 0);
        if let Some(first) = problems.first() {
            toast(
                &self.toasts,
                &format!("Skipped a damaged preset — {first}"),
                5,
            );
        }
        self.apply_filter();
        self.refresh_state();
    }

    fn preset_for(&self, child: &gtk::FlowBoxChild) -> Option<Rc<Preset>> {
        let find = |cards: &[Card]| {
            cards
                .iter()
                .find(|c| &c.child == child)
                .map(|c| c.preset.clone())
        };
        let cards = self.cards.try_borrow().ok()?;
        let user = self.user_cards.try_borrow().ok()?;
        find(&cards).or_else(|| find(&user))
    }

    // -- filtering -------------------------------------------------------

    fn filter(&self, child: &gtk::FlowBoxChild) -> bool {
        let (Ok(tag), Ok(query)) = (self.tag.try_borrow(), self.query.try_borrow()) else {
            return true;
        };
        let check = |cards: &[Card]| {
            cards
                .iter()
                .find(|c| &c.child == child)
                .map(|c| c.matches_filter(&tag, &query))
        };
        let (Ok(cards), Ok(user)) = (self.cards.try_borrow(), self.user_cards.try_borrow()) else {
            return true;
        };
        check(&cards).or_else(|| check(&user)).unwrap_or(true)
    }

    fn apply_filter(&self) {
        self.builtin_grid.invalidate_filter();
        self.user_grid.invalidate_filter();
        let (tag, query) = (self.tag.borrow().clone(), self.query.borrow().clone());
        let cards = self.cards.borrow();
        let total = cards.len();
        let shown = cards
            .iter()
            .filter(|c| c.matches_filter(&tag, &query))
            .count();
        self.builtin_count.set_label(&if shown == total {
            shown.to_string()
        } else {
            format!("{shown} of {total}")
        });
        self.builtin_grid.set_visible(shown > 0);
        self.no_match.set_visible(shown == 0);
    }

    // -- state -----------------------------------------------------------

    /// Staging a preset fires one event per key: settle once afterwards.
    fn queue_refresh(self: &Rc<Self>) {
        if self.refresh_queued.replace(true) {
            return;
        }
        let w = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(p) = w.upgrade() {
                p.refresh_queued.set(false);
                p.preview.update();
                p.refresh_state();
            }
        });
    }

    fn refresh_state(&self) {
        let original = self.original_card.borrow();
        let cards = self.cards.borrow();
        let user = self.user_cards.borrow();
        let all: Vec<&Card> = cards
            .iter()
            .chain(user.iter())
            .chain(original.iter())
            .collect();
        let (banner, states) = {
            let cm = self.cm.borrow();
            let mut pending: BTreeSet<String> = cm.pending().into_keys().collect();
            pending.extend(cm.pending_resets());
            let overrides = self.managed_keys.iter().any(|k| cm.is_modified(k))
                || LIST_KEYWORDS.iter().any(|kw| cm.is_list_modified(kw));
            let states: Vec<Option<CardState>> = all
                .iter()
                .map(|c| {
                    if !c.preset.matches(&cm, &self.store.options) {
                        None
                    } else if c.preset.settings.keys().any(|k| pending.contains(k)) {
                        Some(CardState::Trying)
                    } else {
                        Some(CardState::Current)
                    }
                })
                .collect();
            (original.is_some() && overrides, states)
        };
        self.back_banner.set_visible(banner);
        for (card, state) in all.iter().zip(states) {
            card.set_state(state);
        }
    }

    // -- actions ---------------------------------------------------------

    /// Stage the user's own look again by dropping every look override.
    fn go_back_to_original(self: &Rc<Self>) {
        let before = self.cm.borrow().export_state();
        backend::update(&self.cm, |c| self.store.restore_original(c));
        let cm = self.cm.clone();
        toast_button(
            &self.toasts,
            "Your original look is staged — Apply to keep it",
            4,
            "Undo",
            move || restore_state(&cm, &before),
        );
    }

    fn try_preset(self: &Rc<Self>, preset: &Rc<Preset>) {
        if preset.original() {
            self.go_back_to_original();
            return;
        }
        if preset.matches(&self.cm.borrow(), &self.store.options) {
            toast(
                &self.toasts,
                &format!("“{}” is already your look", preset.name),
                2,
            );
            return;
        }
        let before = self.cm.borrow().export_state();
        if let Err(e) = backend::update(&self.cm, |c| self.store.stage_preset(c, preset)) {
            restore_state(&self.cm, &before);
            toast(
                &self.toasts,
                &format!("Couldn't use “{}”: {e}", preset.name),
                5,
            );
            return;
        }
        let cm = self.cm.clone();
        toast_button(
            &self.toasts,
            &format!("Previewing “{}” — Apply to keep it", preset.name),
            4,
            "Undo",
            move || restore_state(&cm, &before),
        );
    }

    /// AlertDialog with entry rows; `fields` is `[(title, initial, required)]`.
    fn entry_dialog(
        &self,
        heading: &str,
        body: Option<&str>,
        response_label: &str,
        fields: &[(&str, &str, bool)],
        on_ok: impl Fn(Vec<String>) + 'static,
    ) -> adw::AlertDialog {
        let dlg = adw::AlertDialog::new(Some(heading), body);
        let group = adw::PreferencesGroup::builder().margin_top(6).build();
        let rows: Vec<(adw::EntryRow, bool)> = fields
            .iter()
            .map(|(title, initial, required)| {
                let row = adw::EntryRow::builder()
                    .title(*title)
                    .text(*initial)
                    .build();
                group.add(&row);
                (row, *required)
            })
            .collect();
        dlg.set_extra_child(Some(&group));
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("ok", response_label);
        dlg.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
        dlg.set_default_response(Some("ok"));
        dlg.set_close_response("cancel");

        let rows = Rc::new(rows);
        let values = {
            let rows = rows.clone();
            move || -> Vec<String> {
                rows.iter()
                    .map(|(r, _)| r.text().trim().to_string())
                    .collect()
            }
        };
        let validate = {
            let (rows, dlg) = (rows.clone(), dlg.downgrade());
            move || {
                if let Some(d) = dlg.upgrade() {
                    let ok = rows
                        .iter()
                        .all(|(r, req)| !req || !r.text().trim().is_empty());
                    d.set_response_enabled("ok", ok);
                }
            }
        };
        let validate = Rc::new(validate);
        let on_ok = Rc::new(on_ok);
        for (r, _) in rows.iter() {
            let v = validate.clone();
            r.connect_changed(move |_| v());
            let (d, values, on_ok) = (dlg.downgrade(), values.clone(), on_ok.clone());
            r.connect_entry_activated(move |_| {
                let Some(d) = d.upgrade() else { return };
                if d.is_response_enabled("ok") {
                    let vals = values();
                    d.close();
                    on_ok(vals);
                }
            });
        }
        validate();
        dlg.connect_response(None, move |_, resp| {
            if resp == "ok" {
                on_ok(values());
            }
        });
        dlg.present(Some(&self.root));
        if let Some((first, _)) = rows.first() {
            first.grab_focus();
        }
        dlg
    }

    fn save_dialog(self: &Rc<Self>) {
        let w = Rc::downgrade(self);
        self.entry_dialog(
            "Save Current Look",
            Some("Saves your colors, borders, gaps, blur, shadows and animations as a preset you can reuse or share."),
            "Save",
            &[("Name", "", true), ("Short description (optional)", "", false)],
            move |v| {
                if let Some(p) = w.upgrade() {
                    p.save(&v[0], &v[1]);
                }
            },
        );
    }

    fn save(self: &Rc<Self>, name: &str, desc: &str) -> Option<Rc<Preset>> {
        let captured = self.store.capture(&self.cm.borrow(), name, desc, "");
        let res = captured.map_err(|e| e.to_string()).and_then(|mut p| {
            self.store
                .save_user(&mut p)
                .map(|_| p)
                .map_err(|e| e.to_string())
        });
        match res {
            Err(e) => {
                toast(&self.toasts, &format!("Couldn't save the preset: {e}"), 5);
                None
            }
            Ok(p) => {
                self.reload_user();
                toast(
                    &self.toasts,
                    &format!("Saved “{}” to My Presets", p.name),
                    3,
                );
                Some(Rc::new(p))
            }
        }
    }

    fn rename_dialog(self: &Rc<Self>, preset: &Rc<Preset>) {
        let (w, p) = (Rc::downgrade(self), preset.clone());
        self.entry_dialog(
            "Rename Preset",
            None,
            "Rename",
            &[("Name", &preset.name, true)],
            move |v| {
                if let Some(page) = w.upgrade() {
                    page.rename(&p, &v[0]);
                }
            },
        );
    }

    fn rename(self: &Rc<Self>, preset: &Preset, name: &str) -> bool {
        let mut p = preset.clone();
        if let Err(e) = self.store.rename_user(&mut p, name) {
            toast(&self.toasts, &format!("Couldn't rename: {e}"), 5);
            return false;
        }
        self.reload_user();
        true
    }

    fn delete_dialog(self: &Rc<Self>, preset: &Rc<Preset>) {
        let body = format!(
            "“{}” will be removed from My Presets. Your current setup isn't affected.",
            glib::markup_escape_text(&preset.name)
        );
        let dlg = adw::AlertDialog::new(Some("Delete Preset?"), Some(&body));
        dlg.set_body_use_markup(true);
        dlg.add_response("cancel", "Cancel");
        dlg.add_response("delete", "Delete");
        dlg.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dlg.set_default_response(Some("cancel"));
        dlg.set_close_response("cancel");
        let (w, p) = (Rc::downgrade(self), preset.clone());
        dlg.connect_response(None, move |_, r| {
            if let Some(page) = w.upgrade().filter(|_| r == "delete") {
                page.delete(&p);
            }
        });
        dlg.present(Some(&self.root));
    }

    fn delete(self: &Rc<Self>, preset: &Preset) -> bool {
        if let Err(e) = self.store.delete_user(preset) {
            toast(&self.toasts, &format!("Couldn't delete: {e}"), 5);
            return false;
        }
        self.reload_user();
        toast(&self.toasts, &format!("Deleted “{}”", preset.name), 2);
        true
    }

    fn export_dialog(self: &Rc<Self>, preset: &Rc<Preset>) {
        let dlg = gtk::FileDialog::builder()
            .title("Export Preset")
            .initial_name(format!("{}{PRESET_EXT}", preset.id))
            .filters(&file_filters(&[(
                "Hyprland Mod presets",
                &["*.hyprmod-preset"],
            )]))
            .build();
        let (w, p) = (Rc::downgrade(self), preset.clone());
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
                            page.export_to(&p, &path);
                        }
                    }
                }
            },
        );
    }

    fn export_to(&self, preset: &Preset, path: &Path) -> bool {
        if let Err(e) = self.store.export_preset(preset, path) {
            toast(&self.toasts, &format!("Couldn't export: {e}"), 5);
            return false;
        }
        toast(
            &self.toasts,
            &format!("Exported “{}” — share the file with anyone", preset.name),
            3,
        );
        true
    }

    fn import_dialog(self: &Rc<Self>) {
        let dlg = gtk::FileDialog::builder()
            .title("Import Preset")
            .filters(&file_filters(&[
                ("Hyprland Mod presets", &["*.hyprmod-preset", "*.json"]),
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
                            page.import_file(&path);
                        }
                    }
                }
            },
        );
    }

    fn import_file(self: &Rc<Self>, path: &Path) -> Option<Rc<Preset>> {
        let preset = match self.store.import_preset(path) {
            Ok(p) => Rc::new(p),
            Err(e) => {
                let dlg =
                    adw::AlertDialog::new(Some("Couldn't Import Preset"), Some(&e.to_string()));
                dlg.add_response("ok", "OK");
                dlg.present(Some(&self.root));
                return None;
            }
        };
        self.reload_user();
        let (w, p) = (Rc::downgrade(self), preset.clone());
        toast_button(
            &self.toasts,
            &format!("Imported “{}”", preset.name),
            4,
            "Try It",
            move || {
                if let Some(page) = w.upgrade() {
                    page.try_preset(&p);
                }
            },
        );
        Some(preset)
    }
}
