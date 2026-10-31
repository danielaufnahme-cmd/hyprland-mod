//! Single import point for the core API (real `hyprmod-core`, or the dev fake),
//! plus small helpers the UI uses to read option values as text.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

pub use hyprmod_core::*;
pub use serde_json::Value as Json;

pub type Cm = Rc<RefCell<ConfigManager>>;

/// Explicit dry-run only (the core also treats "not on Hyprland" as dry-run).
pub fn is_dry_run() -> bool {
    matches!(
        std::env::var("HYPRLAND_MOD_DRY_RUN")
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Root of the shipped data (schema, presets, icons).
pub fn data_root() -> PathBuf {
    if let Some(p) = std::env::var_os("HYPRLAND_MOD_DATA_ROOT") {
        return PathBuf::from(p);
    }
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.to_path_buf()))
    {
        let installed = dir.join("../share/hyprland-mod");
        if installed.join("schema").is_dir() {
            return installed;
        }
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../data"))
}

pub fn non_empty(s: &str) -> Option<&str> {
    Some(s).filter(|s| !s.trim().is_empty())
}

/// Python ``str()`` of a schema JSON value as config text.
pub fn json_text(v: &Json) -> String {
    match v {
        Json::Null => String::new(),
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

pub fn choices(opt: &OptionSpec) -> &[Choice] {
    opt.choices.as_deref().unwrap_or(&[])
}

pub fn opt_label(opt: &OptionSpec) -> &str {
    if opt.label.is_empty() {
        &opt.key
    } else {
        &opt.label
    }
}

pub fn opt_description(opt: &OptionSpec) -> Option<&str> {
    non_empty(&opt.description)
}

pub fn choice_value(c: &Choice) -> String {
    json_text(&c.value)
}

pub fn choice_label(c: &Choice) -> String {
    if c.label.is_empty() {
        json_text(&c.value)
    } else {
        c.label.clone()
    }
}

fn default_text(opt: &OptionSpec) -> String {
    format_value(opt, &parse_value(opt, None)).unwrap_or_else(|_| json_text(&opt.default))
}

/// Mutate the manager, release the borrow, then deliver its queued events
/// (listeners may read the manager again).
pub fn update<R>(cm: &Cm, f: impl FnOnce(&mut ConfigManager) -> R) -> R {
    ConfigManager::update(cm, f)
}

/// Current value of an option as config text (falls back to the schema default).
pub fn current_text(cm: &Cm, opt: &OptionSpec) -> String {
    let v = cm.borrow().get(&opt.key);
    v.unwrap_or_else(|| default_text(opt))
}

pub fn parse_bool(text: &str) -> Option<bool> {
    match text.trim().to_lowercase().as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

/// First number of a value ("20 20 20 20" gaps count as 20; hex ints allowed).
pub fn parse_number(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', " ");
    let first = t.split_whitespace().next()?;
    if let Some(h) = first.strip_prefix("0x") {
        return i64::from_str_radix(h, 16).ok().map(|v| v as f64);
    }
    first.parse::<f64>().ok()
}

pub fn current_bool(cm: &Cm, opt: &OptionSpec) -> bool {
    parse_bool(&current_text(cm, opt))
        .or_else(|| parse_bool(&default_text(opt)))
        .unwrap_or(false)
}

pub fn current_number(cm: &Cm, opt: &OptionSpec) -> Option<f64> {
    parse_number(&current_text(cm, opt)).or_else(|| parse_number(&default_text(opt)))
}

/// Hyprland config text for a float, trimmed like the core does ("0.5", "1").
pub fn format_float(v: f64, digits: Option<usize>) -> String {
    let mut t = format!("{:.*}", digits.unwrap_or(6), v);
    if t.contains('.') {
        t = t.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if t == "-0" || t.is_empty() {
        "0".into()
    } else {
        t
    }
}

pub fn format_number(opt: &OptionSpec, v: f64) -> String {
    match opt.kind {
        OptionType::Int => format!("{}", v.round() as i64),
        _ => format_float(v, opt.digits),
    }
}

pub fn format_bool(v: bool) -> String {
    if v { "true" } else { "false" }.into()
}

pub fn pending_count(cm: &Cm) -> usize {
    let c = cm.borrow();
    let n = c.pending_count();
    if n == 0 && c.is_dirty() {
        1
    } else {
        n
    }
}

/// Add style classes after building: a builder's ``css_classes`` would replace the
/// classes GTK adds itself (``text-button``, ``image-button``, ``activatable``...).
pub trait WithClasses: Sized {
    fn with_classes(self, classes: &[&str]) -> Self;
}

impl<W: gtk::prelude::IsA<gtk::Widget>> WithClasses for W {
    fn with_classes(self, classes: &[&str]) -> Self {
        for c in classes {
            gtk::prelude::WidgetExt::add_css_class(self.as_ref(), c);
        }
        self
    }
}
