//! Search results across every schema page.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use crate::backend::WithClasses;
use crate::backend::{self, Cm, Group, OptionSpec, Page};
use crate::rows::{build_row, OptionRow};
use crate::settings_page::bottom_spacer;
use crate::sidebar::PageInfo;

const MAX_RESULTS: usize = 60;

fn haystack(page: &Page, group: &Group, opt: &OptionSpec) -> String {
    let choices: Vec<String> = backend::choices(opt)
        .iter()
        .map(backend::choice_label)
        .collect();
    [
        opt.label.as_str(),
        opt.description.as_str(),
        opt.key.as_str(),
        group.title.as_str(),
        page.title.as_str(),
        &choices.join(" "),
    ]
    .join(" ")
    .to_lowercase()
}

pub struct SearchPage {
    pub widget: adw::Bin,
    cm: Cm,
    pages: Vec<Page>,
    extra_pages: Vec<PageInfo>,
    on_open_page: Rc<dyn Fn(&str)>,
    pub rows: RefCell<Vec<OptionRow>>,
    stack: gtk::Stack,
    empty: adw::StatusPage,
    results_holder: adw::Bin,
}

impl SearchPage {
    pub fn new(
        cm: &Cm,
        pages: Vec<Page>,
        on_open_page: Rc<dyn Fn(&str)>,
        extra_pages: Vec<PageInfo>,
    ) -> Self {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .transition_duration(150)
            .build();
        let empty = adw::StatusPage::builder()
            .icon_name("system-search-symbolic")
            .title("No Results Found")
            .description("Try a different search term")
            .build()
            .with_classes(&["compact"]);
        stack.add_named(&empty, Some("empty"));
        let results_holder = adw::Bin::new();
        stack.add_named(&results_holder, Some("results"));
        let widget = adw::Bin::builder().child(&stack).build();
        Self {
            widget,
            cm: cm.clone(),
            pages,
            extra_pages,
            on_open_page,
            rows: RefCell::new(vec![]),
            stack,
            empty,
            results_holder,
        }
    }

    pub fn set_query(&self, query: &str) {
        let lower = query.to_lowercase();
        let tokens: Vec<&str> = lower.split_whitespace().collect();
        let mut rows = vec![];
        let prefs = adw::PreferencesPage::new();
        let mut count = 0;

        // whole pages (shortcuts, displays...) matched by name or keyword
        let page_hits: Vec<&PageInfo> = self
            .extra_pages
            .iter()
            .filter(|p| {
                let hay = format!("{} {} {}", p.title, p.description, p.keywords).to_lowercase();
                tokens.iter().all(|t| hay.contains(t))
            })
            .collect();
        if !page_hits.is_empty() {
            let group = adw::PreferencesGroup::builder().title("Pages").build();
            for p in &page_hits {
                let row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(p.title))
                    .subtitle(glib::markup_escape_text(p.description))
                    .activatable(true)
                    .build()
                    .with_classes(&["search-result"]);
                row.add_prefix(&gtk::Image::from_icon_name(p.icon));
                row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
                let open = self.on_open_page.clone();
                let pid = p.id;
                row.connect_activated(move |_| open(pid));
                group.add(&row);
            }
            prefs.add(&group);
        }

        for page in &self.pages {
            let matches: Vec<&OptionSpec> = page
                .groups
                .iter()
                .flat_map(|g| g.options.iter().map(move |o| (g, o)))
                .filter(|(g, o)| {
                    let hay = haystack(page, g, o);
                    tokens.iter().all(|t| hay.contains(t))
                })
                .map(|(_, o)| o)
                .collect();
            if matches.is_empty() || count >= MAX_RESULTS {
                continue;
            }
            let title = if page.title.is_empty() {
                &page.id
            } else {
                &page.title
            };
            let group = adw::PreferencesGroup::builder()
                .title(glib::markup_escape_text(title))
                .build();
            let open_btn = gtk::Button::builder()
                .label("Open Page")
                .valign(gtk::Align::Center)
                .build()
                .with_classes(&["flat", "open-page-button"]);
            let open = self.on_open_page.clone();
            let pid = page.id.clone();
            open_btn.connect_clicked(move |_| open(&pid));
            group.set_header_suffix(Some(&open_btn));
            for opt in matches.into_iter().take(MAX_RESULTS - count) {
                let row = build_row(&self.cm, opt);
                row.row.add_css_class("search-result");
                group.add(&row.row);
                rows.push(row);
                count += 1;
            }
            prefs.add(&group);
        }
        *self.rows.borrow_mut() = rows;

        if count > 0 || !page_hits.is_empty() {
            prefs.add(&bottom_spacer());
            if count > 0 {
                prefs.set_description(&format!(
                    "{count} matching setting{}",
                    if count != 1 { "s" } else { "" }
                ));
            }
            self.results_holder.set_child(Some(&prefs));
            self.stack.set_visible_child_name("results");
        } else {
            self.results_holder.set_child(gtk::Widget::NONE);
            self.empty.set_description(Some(&format!(
                "Nothing matches “{}”",
                glib::markup_escape_text(query)
            )));
            self.stack.set_visible_child_name("empty");
        }
    }
}
