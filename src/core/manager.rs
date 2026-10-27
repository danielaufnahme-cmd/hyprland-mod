//! ConfigManager: coordinate reading, staging, writing, live-apply, backups, and undo.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::backup::BackupStore;
use super::fsutil::{atomic_write, read_text};
use super::hyprctl::Hyprctl;
use super::lua::{read_lua_managed, render_lua, with_require_line};
use super::managed::{
    read_managed, render, validate_key, validate_value, with_source_line, ManagedState,
};
use super::parser::{parse_config, ParsedConfig};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigMode {
    Conf,
    Lua,
}

#[derive(Debug, Clone, Default)]
pub struct ApplyResult {
    pub ok: bool,
    pub errors: Vec<String>,
    pub live_applied: bool,
    pub backup_id: Option<String>,
}

pub struct ConfigManager {
    pub config_path: PathBuf,
    pub managed_path: PathBuf,
    pub mode: ConfigMode,
    pub data_dir: PathBuf,
    pub backups: BackupStore,
    pub hyprctl: Hyprctl,

    user: ParsedConfig,
    managed: ManagedState,

    pending_set: HashMap<String, String>,
    pending_reset: HashSet<String>,
}

pub fn detect_config(explicit: Option<&Path>) -> (PathBuf, ConfigMode) {
    let base_dir = dirs::home_dir()
        .map(|h| h.join(".config").join("hypr"))
        .unwrap_or_else(|| PathBuf::from(".config/hypr"));

    let config_path = if let Some(p) = explicit {
        p.to_path_buf()
    } else if let Ok(env) = std::env::var("HYPRLAND_MOD_CONFIG") {
        PathBuf::from(env)
    } else {
        base_dir.join("hyprland.conf")
    };

    let sibling_lua = config_path.with_file_name("hyprland.lua");
    if !config_path.to_string_lossy().ends_with(".lua") && sibling_lua.is_file() {
        (sibling_lua, ConfigMode::Lua)
    } else if config_path.to_string_lossy().ends_with(".lua") {
        (config_path, ConfigMode::Lua)
    } else {
        (config_path, ConfigMode::Conf)
    }
}

impl ConfigManager {
    pub fn new(
        config_path: Option<&Path>,
        managed_path: Option<&Path>,
        data_dir: Option<&Path>,
        dry_run: bool,
    ) -> Result<Self> {
        let (config_path, mode) = detect_config(config_path);

        let parent_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
        let managed_path = if let Some(m) = managed_path {
            m.to_path_buf()
        } else {
            let ext = if mode == ConfigMode::Lua { ".lua" } else { ".conf" };
            parent_dir.join(format!("hyprland-mod{}", ext))
        };

        let data_dir = if let Some(d) = data_dir {
            d.to_path_buf()
        } else if let Ok(env) = std::env::var("HYPRLAND_MOD_DATA") {
            PathBuf::from(env)
        } else {
            dirs::data_local_dir()
                .map(|d| d.join("hyprland-mod"))
                .unwrap_or_else(|| PathBuf::from(".local/share/hyprland-mod"))
        };

        let backups = BackupStore::new(&data_dir);
        let hyprctl = Hyprctl::new(dry_run);

        let mut mgr = Self {
            config_path,
            managed_path,
            mode,
            data_dir,
            backups,
            hyprctl,
            user: ParsedConfig::default(),
            managed: ManagedState::default(),
            pending_set: HashMap::new(),
            pending_reset: HashSet::new(),
        };

        mgr.reload()?;
        Ok(mgr)
    }

    pub fn reload(&mut self) -> Result<()> {
        if self.mode == ConfigMode::Conf {
            self.user = parse_config(&self.config_path, &[self.managed_path.clone()]);
            self.managed = read_managed(&self.managed_path);
        } else {
            self.managed = read_lua_managed(&self.managed_path);
        }
        self.pending_set.clear();
        self.pending_reset.clear();
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        if self.pending_reset.contains(key) {
            return self.user.get(key).map(|s| s.to_string());
        }
        if let Some(val) = self.pending_set.get(key) {
            return Some(val.clone());
        }
        if let Some(val) = self.managed.settings.get(key) {
            return Some(val.clone());
        }
        self.user.get(key).map(|s| s.to_string())
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        validate_key(key)?;
        validate_value(value)?;
        self.pending_reset.remove(key);
        self.pending_set.insert(key.to_string(), value.to_string());
        Ok(())
    }

    pub fn reset(&mut self, key: &str) {
        self.pending_set.remove(key);
        if self.managed.settings.contains_key(key) {
            self.pending_reset.insert(key.to_string());
        }
    }

    pub fn is_dirty(&self) -> bool {
        !self.pending_set.is_empty() || !self.pending_reset.is_empty()
    }

    pub fn pending_changes(&self) -> Vec<(String, Option<String>)> {
        let mut changes = Vec::new();
        for (k, v) in &self.pending_set {
            changes.push((k.clone(), Some(v.clone())));
        }
        for k in &self.pending_reset {
            changes.push((k.clone(), None));
        }
        changes.sort();
        changes
    }

    pub fn apply(&mut self) -> Result<ApplyResult> {
        let mut result = ApplyResult::default();
        if !self.is_dirty() && self.managed_path.is_file() {
            result.ok = true;
            return Ok(result);
        }

        let mut next_state = self.managed.clone();
        for (k, v) in self.pending_set.drain() {
            next_state.settings.insert(k, v);
        }
        for k in self.pending_reset.drain() {
            next_state.settings.remove(&k);
        }

        // Render file content
        let content = if self.mode == ConfigMode::Lua {
            render_lua(&next_state)
        } else {
            render(&next_state)
        };

        // Create backup
        let mut files_to_backup = vec![self.managed_path.clone()];
        if self.config_path.is_file() {
            files_to_backup.push(self.config_path.clone());
        }

        match self.backups.create("Apply changes", &files_to_backup, None) {
            Ok(id) => result.backup_id = Some(id),
            Err(e) => result.errors.push(format!("Backup failed: {}", e)),
        }

        // Write managed file atomically
        atomic_write(&self.managed_path, &content)?;

        // Ensure include line in user's main config
        if self.config_path.is_file() {
            if let Ok(main_text) = read_text(&self.config_path) {
                let updated = if self.mode == ConfigMode::Lua {
                    with_require_line(&main_text)
                } else {
                    with_source_line(&main_text, &self.config_path, &self.managed_path)
                };
                if let Some(new_content) = updated {
                    let _ = atomic_write(&self.config_path, &new_content);
                }
            }
        }

        // Live apply reload via hyprctl
        if let Err(e) = self.hyprctl.reload() {
            result.errors.push(format!("hyprctl reload failed: {}", e));
        } else {
            result.live_applied = true;
        }

        self.managed = next_state;
        result.ok = result.errors.is_empty();
        Ok(result)
    }
}
