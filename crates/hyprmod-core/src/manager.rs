//! ConfigManager: read, stage, write, live-apply, back up and undo Hyprland settings.
//!
//! # Events and `RefCell`
//!
//! The manager is single-threaded and is usually shared as `Rc<RefCell<ConfigManager>>`.
//! Mutating methods never call subscribers themselves: they queue events. Deliver them
//! after the mutable borrow has ended, either with
//! [`ConfigManager::update`] (borrow, run, release, dispatch) or by calling
//! [`ConfigManager::dispatch_events`] after your own `borrow_mut()` scope. Listeners
//! may then freely borrow the manager again. Without a `RefCell`,
//! [`ConfigManager::flush_events`] delivers the queue directly.

use std::cell::{OnceCell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anyhow::{anyhow, Result};
use indexmap::IndexMap;
use serde_json::{Map, Value as Json};

use crate::backup::{Backup, BackupStore};
use crate::fsutil::{atomic_write, read_text};
use crate::hyprctl::{self, HyprctlRunner, Runner};
use crate::items;
use crate::lua::{self, LuaTypes};
use crate::managed::{
    read_managed, render, validate_key, validate_value, with_source_line, without_source_line, ManagedState,
    EXTRA_FIELDS, LIST_ORDER,
};
use crate::parser::{normalize_key, parse_config_with_home, ParsedConfig};
use crate::pyjson::{dumps, Dumps};
use crate::util::{abspath, expanduser, path_str, py_str, strip, truthy};
use crate::values::format_float;

pub const HISTORY_LIMIT: usize = 50;
pub const DEFAULT_CONFIG_DIR: &str = "~/.config/hypr";

/// Which config language the user's Hyprland uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Conf,
    Lua,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Mode::Conf => "conf",
            Mode::Lua => "lua",
        }
    }
}

/// What happened; the second listener argument is the key for [`Event::Changed`] (None = many).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Changed,
    Discarded,
    Applied,
    Undo,
    Redo,
    Restored,
    MonitorsReverted,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::Changed => "changed",
            Event::Discarded => "discarded",
            Event::Applied => "applied",
            Event::Undo => "undo",
            Event::Redo => "redo",
            Event::Restored => "restored",
            Event::MonitorsReverted => "monitors_reverted",
        }
    }
}

pub type SubscriptionId = u64;
/// A subscriber: ``f(event, key)``.
pub type ListenerFn = dyn Fn(&Event, Option<&str>);
type Listener = Rc<ListenerFn>;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ApplyResult {
    pub ok: bool,
    pub errors: Vec<String>,
    pub live_applied: bool,
}

impl ApplyResult {
    fn new(ok: bool, errors: Vec<String>, live_applied: bool) -> Self {
        ApplyResult { ok, errors, live_applied }
    }
    fn fail(msg: String) -> Self {
        ApplyResult { ok: false, errors: vec![msg], live_applied: false }
    }
}

/// Construction options; every ``None`` falls back to the environment like the Python app.
#[derive(Default)]
pub struct ManagerOptions {
    pub config_path: Option<PathBuf>,
    pub managed_path: Option<PathBuf>,
    pub data_dir: Option<PathBuf>,
    pub runner: Option<Box<dyn Runner>>,
    /// Use this map instead of the process environment (HOME, HYPRLAND_MOD_*,
    /// HYPRLAND_INSTANCE_SIGNATURE). Meant for tests.
    pub env: Option<HashMap<String, String>>,
}

/// Return (config path, mode). A ``hyprland.lua`` next to the config means Lua mode.
pub fn detect_config(explicit: Option<&str>, home: Option<&str>) -> (PathBuf, Mode) {
    let raw = explicit.map(str::to_string).unwrap_or_else(|| format!("{DEFAULT_CONFIG_DIR}/hyprland.conf"));
    let mut path = abspath(&expanduser(&raw, home));
    let sibling = path.parent().map(|p| p.join("hyprland.lua")).unwrap_or_default();
    if !path_str(&path).ends_with(".lua") && sibling.is_file() {
        path = sibling;
    }
    let mode = if path_str(&path).ends_with(".lua") { Mode::Lua } else { Mode::Conf };
    (path, mode)
}

/// User values in Lua mode: live ``hyprctl -j descriptions`` (cached) plus scanned curves.
struct LuaUserView {
    lists: IndexMap<String, Vec<String>>,
    fetch: bool,
    values: OnceCell<IndexMap<String, String>>,
}

enum UserView {
    Conf(ParsedConfig),
    Lua(LuaUserView),
}

pub struct ConfigManager {
    config_path: PathBuf,
    mode: Mode,
    managed_path: PathBuf,
    data_dir: PathBuf,
    runner: Box<dyn Runner>,
    dry_run: bool,
    hyprland_running: bool,
    home: Option<String>,
    lua_stubs: Option<PathBuf>,
    backups: BackupStore,
    listeners: Vec<(SubscriptionId, Listener)>,
    next_listener: SubscriptionId,
    queued: Vec<(Event, Option<String>)>,
    pending_set: IndexMap<String, String>,
    pending_reset: BTreeSet<String>,
    pending_lists: IndexMap<String, Option<Vec<String>>>,
    /// Staged binds / window_rules / monitors / autostart: the complete new value per field.
    pending_items: IndexMap<String, Json>,
    user: UserView,
    managed: ManagedState,
    lua_types: OnceCell<LuaTypes>,
    history: Vec<Json>,
    index: usize,
}

impl ConfigManager {
    pub fn new(opts: ManagerOptions) -> Result<Self> {
        let env = opts.env;
        let get = |k: &str| -> Option<String> {
            match &env {
                Some(map) => map.get(k).cloned(),
                None => std::env::var(k).ok(),
            }
        };
        let home = get("HOME");
        let explicit =
            opts.config_path.map(|p| path_str(&p)).or_else(|| get("HYPRLAND_MOD_CONFIG").filter(|s| !s.is_empty()));
        let (config_path, mode) = detect_config(explicit.as_deref(), home.as_deref());
        let ext = if mode == Mode::Lua { ".lua" } else { ".conf" };
        let managed_raw = opts.managed_path.map(|p| path_str(&p)).unwrap_or_else(|| {
            path_str(&config_path.parent().unwrap_or(Path::new("/")).join(format!("hyprland-mod{ext}")))
        });
        let managed_path = abspath(&expanduser(&managed_raw, home.as_deref()));
        let data_raw = opts
            .data_dir
            .map(|p| path_str(&p))
            .or_else(|| get("HYPRLAND_MOD_DATA").filter(|s| !s.is_empty()))
            .unwrap_or_else(|| "~/.local/share/hyprland-mod".into());
        let data_dir = abspath(&expanduser(&data_raw, home.as_deref()));
        let dry_run = hyprctl::is_dry_run_with(get);
        let hyprland_running = get("HYPRLAND_INSTANCE_SIGNATURE").is_some_and(|s| !s.is_empty());
        let lua_stubs = get("HYPRLAND_MOD_LUA_STUBS").filter(|s| !s.is_empty()).map(PathBuf::from);
        let mut cm = ConfigManager {
            backups: BackupStore::new(&data_dir),
            config_path,
            mode,
            managed_path,
            data_dir,
            runner: opts.runner.unwrap_or_else(|| Box::new(HyprctlRunner)),
            dry_run,
            hyprland_running,
            home,
            lua_stubs,
            listeners: Vec::new(),
            next_listener: 1,
            queued: Vec::new(),
            pending_set: IndexMap::new(),
            pending_reset: BTreeSet::new(),
            pending_lists: IndexMap::new(),
            pending_items: IndexMap::new(),
            user: UserView::Conf(ParsedConfig::default()),
            managed: ManagedState::default(),
            lua_types: OnceCell::new(),
            history: Vec::new(),
            index: 0,
        };
        cm.reload();
        let (history, index) = cm.load_history();
        cm.history = history;
        cm.index = index;
        Ok(cm)
    }

    // ------------------------------------------------------------ paths & flags

    pub fn mode(&self) -> Mode {
        self.mode
    }
    pub fn config_path(&self) -> &Path {
        &self.config_path
    }
    pub fn managed_path(&self) -> &Path {
        &self.managed_path
    }
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }
    pub fn dry_run(&self) -> bool {
        self.dry_run
    }
    pub fn hyprland_running(&self) -> bool {
        self.hyprland_running
    }
    /// Parser warnings about the user's config.
    pub fn warnings(&self) -> Vec<String> {
        match &self.user {
            UserView::Conf(p) => p.warnings.clone(),
            UserView::Lua(_) => Vec::new(),
        }
    }

    // ------------------------------------------------------------ reading

    /// Re-read the user's config and the managed file from disk.
    pub fn reload(&mut self) {
        let exclude = [self.managed_path.clone()];
        if self.mode == Mode::Lua {
            let files = lua::required_files(&self.config_path, &exclude);
            self.user = UserView::Lua(LuaUserView {
                lists: lua::scan_lists(&files),
                fetch: !self.dry_run,
                values: OnceCell::new(),
            });
            self.managed = lua::read_lua_state(&self.managed_path);
        } else {
            self.user = UserView::Conf(parse_config_with_home(&self.config_path, &exclude, self.home.as_deref()));
            self.managed = read_managed(&self.managed_path);
        }
    }

    fn hyprctl(&self, args: &[&str]) -> Result<(i32, String, String)> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        self.runner.run(&args)
    }

    /// Current option values from the running Hyprland (one read-only call).
    fn fetch_descriptions(&self) -> IndexMap<String, String> {
        let data: Json = match self.hyprctl(&["-j", "descriptions"]) {
            Ok((0, out, _)) => serde_json::from_str(&out).unwrap_or(Json::Null),
            _ => return IndexMap::new(),
        };
        let mut values = IndexMap::new();
        for item in data.as_array().into_iter().flatten() {
            let Some(obj) = item.as_object() else { continue };
            let name = ["value", "name", "key"].iter().find_map(|k| obj.get(*k).filter(|v| truthy(v)));
            let payload = match obj.get("data") {
                Some(Json::Object(d)) => d,
                _ => obj,
            };
            if let (Some(Json::String(name)), Some(current)) = (name, payload.get("current")) {
                if let Some(text) = lua::describe_value(name, current) {
                    values.insert(lua::dotted(name), text);
                }
            }
        }
        values
    }

    fn lua_types(&self) -> &LuaTypes {
        self.lua_types.get_or_init(|| lua::load_lua_types(self.lua_stubs.as_deref()))
    }

    fn user_get(&self, key: &str) -> Option<String> {
        match &self.user {
            UserView::Conf(p) => p.get(key).map(str::to_string),
            UserView::Lua(v) => {
                let values = v.values.get_or_init(|| if v.fetch { self.fetch_descriptions() } else { IndexMap::new() });
                values.get(&lua::dotted(key)).cloned()
            }
        }
    }

    fn user_get_all(&self, keyword: &str) -> Vec<String> {
        match &self.user {
            UserView::Conf(p) => p.get_all(keyword),
            UserView::Lua(v) => v.lists.get(keyword).cloned().unwrap_or_default(),
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let key = normalize_key(key);
        if let Some(v) = self.pending_set.get(&key) {
            return Some(v.clone());
        }
        if !self.pending_reset.contains(&key) {
            if let Some(v) = self.managed.settings.get(&key) {
                return Some(v.clone());
            }
        }
        self.user_get(&key)
    }

    /// Value from the user's own config only (ignores Hyprland Mod).
    pub fn user_value(&self, key: &str) -> Option<String> {
        self.user_get(&normalize_key(key))
    }

    pub fn is_modified(&self, key: &str) -> bool {
        let key = normalize_key(key);
        self.pending_set.contains_key(&key)
            || (!self.pending_reset.contains(&key) && self.managed.settings.contains_key(&key))
    }

    pub fn get_list(&self, keyword: &str) -> Vec<String> {
        let kw = normalize_key(keyword);
        if let Some(staged) = self.pending_lists.get(&kw) {
            return staged.clone().unwrap_or_else(|| self.user_get_all(&kw));
        }
        if let Some(v) = self.managed.lists.get(&kw) {
            return v.clone();
        }
        self.user_get_all(&kw)
    }

    pub fn is_list_modified(&self, keyword: &str) -> bool {
        let kw = normalize_key(keyword);
        match self.pending_lists.get(&kw) {
            Some(staged) => staged.is_some(),
            None => self.managed.lists.contains_key(&kw),
        }
    }

    // ------------------------------------------------------------ staging

    /// Stage ``value`` for ``key``; an empty value acts as [`ConfigManager::reset`].
    pub fn set(&mut self, key: &str, value: String) -> Result<()> {
        validate_key(key)?;
        let key = normalize_key(key);
        let value = strip(&value).to_string();
        if value.is_empty() {
            self.reset(&key);
            return Ok(());
        }
        validate_value(&value)?;
        self.pending_reset.remove(&key);
        if self.managed.settings.get(&key) == Some(&value) {
            self.pending_set.shift_remove(&key);
        } else {
            self.pending_set.insert(key.clone(), value);
        }
        self.emit(Event::Changed, Some(key));
        Ok(())
    }

    pub fn reset(&mut self, key: &str) {
        let key = normalize_key(key);
        self.pending_set.shift_remove(&key);
        if self.managed.settings.contains_key(&key) {
            self.pending_reset.insert(key.clone());
        }
        self.emit(Event::Changed, Some(key));
    }

    pub fn set_list(&mut self, keyword: &str, values: Vec<String>) -> Result<()> {
        validate_key(keyword)?;
        let kw = normalize_key(keyword);
        let clean: Vec<String> = values.iter().map(|v| strip(v).to_string()).collect();
        for v in &clean {
            validate_value(v)?;
        }
        if self.mode == Mode::Lua {
            lua::list_calls(&kw, &clean)?;
        }
        if self.managed.lists.get(&kw) == Some(&clean) {
            self.pending_lists.shift_remove(&kw);
        } else {
            self.pending_lists.insert(kw.clone(), Some(clean));
        }
        self.emit(Event::Changed, Some(kw));
        Ok(())
    }

    pub fn reset_list(&mut self, keyword: &str) {
        let kw = normalize_key(keyword);
        if self.managed.lists.contains_key(&kw) {
            self.pending_lists.insert(kw.clone(), None);
        } else {
            self.pending_lists.shift_remove(&kw);
        }
        self.emit(Event::Changed, Some(kw));
    }

    pub fn pending(&self) -> BTreeMap<String, String> {
        self.pending_set.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    pub fn pending_resets(&self) -> Vec<String> {
        self.pending_reset.iter().cloned().collect()
    }

    /// Number of staged changes (settings set/reset, lists, item fields).
    pub fn pending_count(&self) -> usize {
        self.pending_set.len() + self.pending_reset.len() + self.pending_lists.len() + self.pending_items.len()
    }

    pub fn is_dirty(&self) -> bool {
        self.pending_count() > 0
    }

    pub fn discard(&mut self) {
        self.clear_pending();
        self.emit(Event::Discarded, None);
    }

    fn clear_pending(&mut self) {
        self.pending_set.clear();
        self.pending_reset.clear();
        self.pending_lists.clear();
        self.pending_items.clear();
    }

    fn staged_state(&self) -> ManagedState {
        let mut state = self.managed.clone();
        for key in &self.pending_reset {
            state.settings.shift_remove(key);
        }
        for (k, v) in &self.pending_set {
            state.settings.insert(k.clone(), v.clone());
        }
        for (kw, values) in &self.pending_lists {
            match values {
                None => {
                    state.lists.shift_remove(kw);
                }
                Some(v) => {
                    state.lists.insert(kw.clone(), v.clone());
                }
            }
        }
        for (name, value) in &self.pending_items {
            state.set_item(name, value);
        }
        state
    }

    /// Staged copy of an item field (``binds``/``window_rules``/``monitors``/``autostart``).
    pub fn staged_item(&self, name: &str) -> Json {
        self.pending_items.get(name).cloned().or_else(|| self.managed.item(name)).unwrap_or(Json::Null)
    }

    /// Stage the complete new value of an item field (already validated by the caller).
    pub fn stage_item(&mut self, name: &str, value: Json) {
        if !EXTRA_FIELDS.contains(&name) {
            return;
        }
        if self.managed.item(name).as_ref() == Some(&value) {
            self.pending_items.shift_remove(name);
        } else {
            self.pending_items.insert(name.to_string(), value);
        }
        self.emit(Event::Changed, Some(name.to_string()));
    }

    // ------------------------------------------------------------ applying

    /// Write staged changes to the managed file and apply them live.
    pub fn apply(&mut self) -> ApplyResult {
        if !self.is_dirty() {
            return ApplyResult::new(true, Vec::new(), false);
        }
        let (result, written) = self.commit(self.staged_state(), "Before applying changes");
        if written {
            self.push_history();
            self.emit(Event::Applied, None);
        }
        result
    }

    /// Back up if needed, write ``new``, ensure the source line, live-apply.
    fn commit(&mut self, new: ManagedState, label: &str) -> (ApplyResult, bool) {
        if !self.config_path.is_file() {
            return (ApplyResult::fail(format!("Hyprland config not found: {}", path_str(&self.config_path))), false);
        }
        let old = self.managed.clone();
        let first = match (|| -> Result<bool> {
            let text = self.render_state(&new)?; // before any write: a render error leaves files untouched
            let first = self.ensure_initial_backup()?;
            atomic_write(&self.managed_path, &text)?;
            Ok(first)
        })() {
            Ok(first) => first,
            Err(e) => return (ApplyResult::fail(format!("Could not write settings: {e}")), false),
        };
        let mut errors = Vec::new();
        let loader = if new.is_empty() {
            self.remove_source_line() // nothing managed any more: leave the user's config as it was
        } else {
            self.ensure_source_line(!first, label)
        };
        if let Err(e) = loader {
            errors.push(format!("Could not update the source line in {}: {e}", path_str(&self.config_path)));
        }
        self.clear_pending();
        self.reload();
        let (live, live_errors) = self.live_apply(&old);
        errors.extend(live_errors);
        (ApplyResult::new(errors.is_empty(), errors, live), true)
    }

    /// The managed file for ``state`` in the active config language.
    fn render_state(&self, state: &ManagedState) -> Result<String> {
        Ok(match self.mode {
            Mode::Lua => lua::render_lua(state, self.lua_types(), &items::lua_calls(state)?)?,
            Mode::Conf => render(state) + &items::render_conf_extras(state)?,
        })
    }

    /// Back up the user's config the very first time anything is written.
    fn ensure_initial_backup(&mut self) -> Result<bool> {
        let mut state = self.read_state();
        if state.get("initial_backup").is_some_and(truthy) {
            return Ok(false);
        }
        // Taken before the source line is added. If the line is somehow already there
        // (e.g. the app's data folder was deleted), it is left out of the copy.
        let mut texts = HashMap::new();
        if let Ok(current) = read_text(&self.config_path) {
            if let Some(stripped) = self.without_loader(&current) {
                texts.insert(self.config_path.clone(), stripped);
            }
        }
        let id = self.backups.create(
            "Original config (before Hyprland Mod)",
            std::slice::from_ref(&self.config_path),
            &texts,
        )?;
        state.insert("initial_backup".into(), Json::String(id));
        self.write_json("state.json", &Json::Object(state))?;
        Ok(true)
    }

    fn ensure_source_line(&mut self, backup_first: bool, label: &str) -> Result<()> {
        let text = read_text(&self.config_path)?;
        let new_text = match self.mode {
            Mode::Lua => lua::with_require_line(&text, &self.config_path, &self.managed_path),
            Mode::Conf => with_source_line(&text, &self.config_path, &self.managed_path, self.home.as_deref()),
        };
        let Some(new_text) = new_text else { return Ok(()) };
        if backup_first {
            self.create_backup(&format!("{label} (adding source line)"))?;
        }
        atomic_write(&self.config_path, &new_text)?;
        Ok(())
    }

    fn without_loader(&self, text: &str) -> Option<String> {
        match self.mode {
            Mode::Lua => lua::without_require_line(text, &self.config_path, &self.managed_path),
            Mode::Conf => without_source_line(text, &self.config_path, &self.managed_path, self.home.as_deref()),
        }
    }

    /// Drop our source/require line (and its marker comment) from the user's config.
    fn remove_source_line(&mut self) -> Result<()> {
        if !self.config_path.is_file() {
            return Ok(());
        }
        if let Some(new_text) = self.without_loader(&read_text(&self.config_path)?) {
            atomic_write(&self.config_path, &new_text)?;
        }
        Ok(())
    }

    fn live_apply(&self, old: &ManagedState) -> (bool, Vec<String>) {
        if self.dry_run {
            return (false, Vec::new());
        }
        let new = &self.managed;
        if self.mode == Mode::Lua {
            return self.live_apply_lua(old, new);
        }
        let mut commands: Vec<(String, String)> = Vec::new();
        let mut need_reload = false;
        let mut changed: Vec<(&String, &String)> =
            new.settings.iter().filter(|(k, v)| old.settings.get(*k) != Some(*v)).collect();
        changed.sort();
        commands.extend(changed.into_iter().map(|(k, v)| (k.clone(), v.clone())));
        let mut removed: Vec<&String> = old.settings.keys().filter(|k| !new.settings.contains_key(*k)).collect();
        removed.sort();
        for key in removed {
            match self.user_get(key) {
                None => need_reload = true, // default unknown here; let Hyprland re-read files
                Some(fallback) => commands.push((key.clone(), fallback)),
            }
        }
        let mut keywords: Vec<String> = LIST_ORDER
            .iter()
            .filter(|k| old.lists.contains_key(**k) || new.lists.contains_key(**k))
            .map(|k| k.to_string())
            .collect();
        let mut rest: Vec<String> = old
            .lists
            .keys()
            .chain(new.lists.keys())
            .filter(|k| !keywords.contains(k))
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        keywords.append(&mut rest);
        for kw in keywords {
            let before = old.lists.get(&kw).cloned().unwrap_or_else(|| self.user_get_all(&kw));
            let after = new.lists.get(&kw).cloned().unwrap_or_else(|| self.user_get_all(&kw));
            if before == after {
                continue;
            }
            if before.iter().any(|v| !after.contains(v)) {
                need_reload = true;
            }
            commands.extend(after.iter().filter(|v| !before.contains(v)).map(|v| (kw.clone(), v.clone())));
        }
        let (added, items_reload) = items::item_changes(old, new);
        need_reload |= items_reload;
        if let Some(added) = added {
            match items::conf_lines(&added) {
                Ok(lines) => commands.extend(lines),
                Err(e) => return (false, vec![e.to_string()]),
            }
        }
        if commands.iter().any(|(_, v)| v.contains(';')) {
            need_reload = true;
        }
        if need_reload {
            self.run(&["reload".into()])
        } else if !commands.is_empty() {
            self.run(&["--batch".into(), hyprctl::build_batch(&commands)])
        } else {
            (true, Vec::new())
        }
    }

    /// One ``hyprctl eval`` with the changed keys; removals need a full reload.
    fn live_apply_lua(&self, old: &ManagedState, new: &ManagedState) -> (bool, Vec<String>) {
        let changed: IndexMap<String, String> = new
            .settings
            .iter()
            .filter(|(k, v)| old.settings.get(*k) != Some(*v))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut need_reload = old.settings.keys().any(|k| !new.settings.contains_key(k));
        let mut added: IndexMap<String, Vec<String>> = IndexMap::new();
        for kw in lua::LUA_LIST_KEYWORDS {
            let before = old.lists.get(*kw).cloned().unwrap_or_else(|| self.user_get_all(kw));
            let after = new.lists.get(*kw).cloned().unwrap_or_else(|| self.user_get_all(kw));
            if before.iter().any(|v| !after.contains(v)) {
                need_reload = true;
            }
            added.insert(kw.to_string(), after.into_iter().filter(|v| !before.contains(v)).collect());
        }
        let (new_items, items_reload) = items::item_changes(old, new);
        if need_reload || items_reload {
            return self.run(&["reload".into()]);
        }
        let code = (|| -> Result<String> {
            let mut parts = vec![lua::eval_code(&changed, &added, self.lua_types())?];
            if let Some(state) = &new_items {
                parts.extend(items::lua_calls(state)?);
            }
            Ok(parts.into_iter().filter(|c| !c.is_empty()).collect::<Vec<_>>().join("; "))
        })();
        match code {
            Err(e) => (false, vec![e.to_string()]),
            Ok(code) if code.is_empty() => (true, Vec::new()),
            Ok(code) => self.run(&["eval".into(), code]),
        }
    }

    fn run(&self, args: &[String]) -> (bool, Vec<String>) {
        match self.runner.run(args) {
            Err(e) => (false, vec![format!("hyprctl failed: {e}")]), // a broken runner must never break saving
            Ok((code, out, err)) => {
                let errors = hyprctl::batch_errors(code, &out, &err);
                (errors.is_empty(), errors)
            }
        }
    }

    // ------------------------------------------------------------ undo / redo

    pub fn can_undo(&self) -> bool {
        self.index > 0
    }

    pub fn can_redo(&self) -> bool {
        self.index + 1 < self.history.len()
    }

    pub fn undo(&mut self) -> ApplyResult {
        self.step(-1, Event::Undo)
    }

    pub fn redo(&mut self) -> ApplyResult {
        self.step(1, Event::Redo)
    }

    fn step(&mut self, delta: isize, event: Event) -> ApplyResult {
        let target = self.index as isize + delta;
        if target < 0 || target as usize >= self.history.len() {
            return ApplyResult::fail(format!("Nothing to {}", event.name()));
        }
        let target = target as usize;
        let state = match ManagedState::from_dict(&self.history[target]) {
            Ok(s) => s,
            Err(e) => return ApplyResult::fail(format!("Could not write settings: {e}")),
        };
        let (result, written) = self.commit(state, &format!("Before {}", event.name()));
        if written {
            self.index = target;
            self.save_history();
            self.emit(event, None);
        }
        result
    }

    fn push_history(&mut self) {
        let snap = self.managed.to_dict();
        if self.history.get(self.index) == Some(&snap) {
            return;
        }
        self.history.truncate(self.index + 1);
        self.history.push(snap);
        if self.history.len() > HISTORY_LIMIT {
            let extra = self.history.len() - HISTORY_LIMIT;
            self.history.drain(..extra);
        }
        self.index = self.history.len() - 1;
        self.save_history();
    }

    fn load_history(&self) -> (Vec<Json>, usize) {
        let current = self.managed.to_dict();
        let loaded = std::fs::read_to_string(self.data_dir.join("history.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Json>(&t).ok())
            .and_then(|data| {
                let states = data.get("states")?.as_array()?.clone();
                let index = match data.get("index")? {
                    Json::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64))?,
                    Json::String(s) => crate::util::parse_int(s)?,
                    Json::Bool(b) => *b as i64,
                    _ => return None,
                };
                (index >= 0 && (index as usize) < states.len() && states[index as usize] == current)
                    .then_some((states, index as usize))
            });
        loaded.unwrap_or((vec![current], 0))
    }

    fn save_history(&self) {
        let data = serde_json::json!({"states": self.history, "index": self.index});
        let _ = self.write_json("history.json", &data);
    }

    // ------------------------------------------------------------ backups

    fn backup_files(&self) -> Vec<PathBuf> {
        vec![self.config_path.clone(), self.managed_path.clone()]
    }

    pub fn list_backups(&self) -> Vec<Backup> {
        self.backups.list()
    }

    pub fn create_backup(&mut self, label: &str) -> Result<String> {
        self.backups.create(label, &self.backup_files(), &HashMap::new())
    }

    /// Restore files from a backup (after saving a safety backup of the current state).
    pub fn restore_backup(&mut self, backup_id: &str) -> ApplyResult {
        let Some(known) = self.list_backups().into_iter().find(|b| b.id == backup_id) else {
            return ApplyResult::fail(format!("Unknown backup: {backup_id}"));
        };
        let restored = (|| -> Result<()> {
            self.create_backup(&format!("Before restoring \"{}\"", known.label))?;
            self.backups.restore(backup_id)?;
            if !known.files.contains(&self.managed_path) {
                // The managed file did not exist then: restore "no Hyprland Mod settings",
                // written in the config's own language (a conf header is invalid Lua).
                let empty = self.render_state(&ManagedState::default())?;
                atomic_write(&self.managed_path, &empty)?;
            }
            Ok(())
        })();
        if let Err(e) = restored {
            return ApplyResult::fail(format!("Could not restore backup: {e}"));
        }
        self.clear_pending();
        self.reload();
        self.push_history();
        let (live, errors) = if self.dry_run { (false, Vec::new()) } else { self.run(&["reload".into()]) };
        self.emit(Event::Restored, None);
        ApplyResult::new(errors.is_empty(), errors, live)
    }

    // ------------------------------------------------------------ current look (for presets)

    /// The user's own look: values in effect without Hyprland Mod's overrides.
    ///
    /// Keys Hyprland Mod does not manage: live ``hyprctl -j descriptions`` when running,
    /// else the user's parsed config (conf). Managed keys: the user's parsed value (conf);
    /// in Lua mode that is unknowable without running their config, so None.
    /// None also means "not set": Hyprland's default. Staged changes are ignored.
    pub fn snapshot_current_look(&self, keys: &[&str]) -> BTreeMap<String, Option<String>> {
        let live = if !self.dry_run && self.mode == Mode::Conf { self.fetch_descriptions() } else { IndexMap::new() };
        keys.iter()
            .map(|key| {
                let k = normalize_key(key);
                let value = if self.managed.settings.contains_key(&k) {
                    if self.mode == Mode::Conf {
                        self.user_get(&k)
                    } else {
                        None
                    }
                } else if self.mode == Mode::Lua {
                    self.user_get(&k) // live descriptions (cached); None in dry-run
                } else {
                    live.get(&lua::dotted(&k)).filter(|v| !v.is_empty()).cloned().or_else(|| self.user_get(&k))
                };
                (key.to_string(), value)
            })
            .collect()
    }

    /// The user's own (curves, animations): live ``hyprctl -j animations`` when nothing is
    /// overridden by Hyprland Mod, else what their config files define.
    pub fn current_animations(&self) -> (Vec<String>, Vec<String>) {
        if !self.dry_run && !["bezier", "animation"].iter().any(|k| self.managed.lists.contains_key(*k)) {
            if let Some(live) = self.fetch_animations() {
                return live;
            }
        }
        (self.user_get_all("bezier"), self.user_get_all("animation"))
    }

    fn fetch_animations(&self) -> Option<(Vec<String>, Vec<String>)> {
        let data: Json = match self.hyprctl(&["-j", "animations"]) {
            Ok((0, out, _)) => serde_json::from_str(&out).ok()?,
            _ => return None,
        };
        let pair = data.as_array().filter(|a| a.len() == 2 && a.iter().all(Json::is_array))?;
        let (anims, curves) = (pair[0].as_array()?, pair[1].as_array()?);
        let num = |v: &Json| -> Option<String> {
            let f = match v {
                Json::Number(n) => n.as_f64()?,
                Json::String(s) => crate::util::parse_float(s)?,
                Json::Bool(b) => *b as i64 as f64,
                _ => return None,
            };
            Some(format_float(f, None))
        };
        let mut beziers = Vec::new();
        for c in curves {
            let Some(c) = c.as_object() else { continue };
            let Some(name) = c.get("name").filter(|n| truthy(n)) else { continue };
            let Json::String(name) = name else { continue };
            if name == "default" || name.starts_with("__") {
                continue;
            }
            let pts: Option<Vec<String>> = ["X0", "Y0", "X1", "Y1"].iter().map(|k| c.get(*k).and_then(num)).collect();
            if let Some(pts) = pts {
                beziers.push(format!("{name}, {}", pts.join(", ")));
            }
        }
        let mut animations = Vec::new();
        for a in anims {
            let Some(a) = a.as_object() else { continue };
            if !a.get("overridden").is_some_and(truthy) || !a.get("name").is_some_and(truthy) {
                continue;
            }
            let enabled = a.get("enabled").is_none_or(truthy);
            let mut parts = vec![py_str(&a["name"]), if enabled { "1" } else { "0" }.to_string()];
            let speed = a.get("speed").filter(|s| s.is_number() || s.is_boolean());
            if let (true, Some(speed), Some(bezier)) = (enabled, speed, a.get("bezier").filter(|b| truthy(b))) {
                parts.push(num(speed).unwrap_or_default());
                parts.push(py_str(bezier));
                if let Some(style) = a.get("style").filter(|s| truthy(s)) {
                    parts.push(py_str(style));
                }
            }
            animations.push(parts.join(", "));
        }
        Some((beziers, animations))
    }

    // ------------------------------------------------------------ export / import

    /// Managed setup including staged changes, as plain JSON data.
    pub fn export_state(&self) -> Json {
        self.staged_state().to_dict()
    }

    /// Stage ``data`` (from [`ConfigManager::export_state`]) as the complete managed setup.
    ///
    /// Item fields missing from ``data`` stay as they are (older exports and presets
    /// don't have them); present ones replace them. Nothing changes on error.
    pub fn import_state(&mut self, data: Json) -> Result<()> {
        let state = ManagedState::from_dict(&data)?;
        let obj = data.as_object().ok_or_else(|| anyhow!("state must be a dict"))?;
        let conf = self.mode == Mode::Conf;
        let mut staged_items: IndexMap<String, Json> = IndexMap::new();
        if obj.contains_key("binds") {
            let clean: Result<Vec<Json>> = state
                .binds
                .iter()
                .map(|b| items::clean_bind(&Json::Object(b.clone()), conf, None).map(Json::Object))
                .collect();
            staged_items.insert("binds".into(), Json::Array(clean?));
        }
        if obj.contains_key("window_rules") {
            let clean: Result<Vec<Json>> = state
                .window_rules
                .iter()
                .map(|r| items::clean_rule(&Json::Object(r.clone()), conf, None).map(Json::Object))
                .collect();
            staged_items.insert("window_rules".into(), Json::Array(clean?));
        }
        if obj.contains_key("monitors") {
            let mut out = Map::new();
            for (name, spec) in &state.monitors {
                out.insert(
                    items::clean_monitor_name(name)?,
                    Json::Object(items::clean_monitor(&Json::Object(spec.clone()))?),
                );
            }
            staged_items.insert("monitors".into(), Json::Object(out));
        }
        if obj.contains_key("autostart") {
            let mut out = Map::new();
            for (id, cmd) in &state.autostart {
                out.insert(items::clean_autostart_id(id)?, Json::String(items::clean_command(cmd)?));
            }
            staged_items.insert("autostart".into(), Json::Object(out));
        }
        for (key, value) in &state.settings {
            validate_key(key)?;
            validate_value(value)?;
        }
        for (kw, values) in &state.lists {
            validate_key(kw)?;
            for v in values {
                validate_value(v)?;
            }
            if self.mode == Mode::Lua {
                lua::list_calls(kw, values)?;
            }
        }
        let keep: Vec<(String, Json)> = self
            .pending_items
            .iter()
            .filter(|(k, _)| !obj.contains_key(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        self.clear_pending();
        for key in self.managed.settings.keys() {
            if !state.settings.contains_key(key) {
                self.pending_reset.insert(key.clone());
            }
        }
        for (key, value) in &state.settings {
            if self.managed.settings.get(key) != Some(value) {
                self.pending_set.insert(key.clone(), value.clone());
            }
        }
        for kw in self.managed.lists.keys() {
            if !state.lists.contains_key(kw) {
                self.pending_lists.insert(kw.clone(), None);
            }
        }
        for (kw, values) in &state.lists {
            if self.managed.lists.get(kw) != Some(values) {
                self.pending_lists.insert(kw.clone(), Some(values.clone()));
            }
        }
        self.pending_items.extend(keep);
        for (name, value) in staged_items {
            if self.managed.item(&name).as_ref() != Some(&value) {
                self.pending_items.insert(name, value);
            }
        }
        self.emit(Event::Changed, None);
        Ok(())
    }

    // ------------------------------------------------------------ events & state

    /// Register ``f(event, key)``; see the module docs for when it is called.
    pub fn subscribe(&mut self, f: Box<ListenerFn>) -> SubscriptionId {
        let id = self.next_listener;
        self.next_listener += 1;
        self.listeners.push((id, Rc::from(f)));
        id
    }

    pub fn unsubscribe(&mut self, id: SubscriptionId) {
        self.listeners.retain(|(i, _)| *i != id);
    }

    fn emit(&mut self, event: Event, key: Option<String>) {
        if !self.listeners.is_empty() {
            self.queued.push((event, key));
        }
    }

    /// Queued events (oldest first), removing them from the queue.
    pub fn take_events(&mut self) -> Vec<(Event, Option<String>)> {
        std::mem::take(&mut self.queued)
    }

    /// Deliver queued events while ``self`` is borrowed (listeners must not touch the manager).
    pub fn flush_events(&mut self) {
        let events = self.take_events();
        let listeners: Vec<Listener> = self.listeners.iter().map(|(_, l)| l.clone()).collect();
        call_listeners(&listeners, &events);
    }

    /// Deliver queued events with no borrow held, so listeners may use the manager again.
    pub fn dispatch_events(cell: &RefCell<ConfigManager>) {
        loop {
            let (events, listeners) = {
                let mut cm = cell.borrow_mut();
                let events = cm.take_events();
                let listeners: Vec<Listener> = cm.listeners.iter().map(|(_, l)| l.clone()).collect();
                (events, listeners)
            };
            if events.is_empty() {
                break;
            }
            call_listeners(&listeners, &events);
        }
    }

    /// Borrow mutably, run ``f``, release the borrow, then dispatch the queued events.
    pub fn update<R>(cell: &RefCell<ConfigManager>, f: impl FnOnce(&mut ConfigManager) -> R) -> R {
        let result = f(&mut cell.borrow_mut());
        Self::dispatch_events(cell);
        result
    }

    fn read_state(&self) -> Map<String, Json> {
        std::fs::read_to_string(self.data_dir.join("state.json"))
            .ok()
            .and_then(|t| serde_json::from_str::<Json>(&t).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default()
    }

    fn write_json(&self, name: &str, data: &Json) -> Result<()> {
        atomic_write(&self.data_dir.join(name), &dumps(data, Dumps::indented(1)))?;
        Ok(())
    }
}

fn call_listeners(listeners: &[Listener], events: &[(Event, Option<String>)]) {
    for (event, key) in events {
        for l in listeners {
            let call = std::panic::AssertUnwindSafe(|| l(event, key.as_deref()));
            if std::panic::catch_unwind(call).is_err() {
                // one bad listener must not break the engine
                eprintln!("hyprland-mod: listener error on {}", event.name());
            }
        }
    }
}
