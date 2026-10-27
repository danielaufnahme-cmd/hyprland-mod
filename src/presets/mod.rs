//! Hyprland theme presets and safe import/export.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct PresetPreview {
    #[serde(default)]
    pub gaps_in: u32,
    #[serde(default)]
    pub gaps_out: u32,
    #[serde(default)]
    pub rounding: u32,
    #[serde(default)]
    pub border_size: u32,
    #[serde(default)]
    pub active_border: String,
    #[serde(default)]
    pub inactive_border: String,
    #[serde(default)]
    pub blur: bool,
    #[serde(default)]
    pub shadow: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub preview: PresetPreview,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

const ALLOWED_SETTING_PREFIXES: &[&str] = &[
    "general:",
    "decoration:",
    "animations:",
    "misc:",
    "cursor:",
    "group:",
];

/// Presets are safety-checked: imported presets can only touch aesthetic keys.
pub fn validate_preset(preset: &Preset) -> Result<()> {
    for key in preset.settings.keys() {
        let allowed = ALLOWED_SETTING_PREFIXES.iter().any(|prefix| key.starts_with(prefix));
        if !allowed {
            bail!("Preset contains disallowed key: {:?}. Only visual settings are permitted.", key);
        }
    }
    Ok(())
}

pub fn presets_dir() -> PathBuf {
    if let Ok(env) = std::env::var("HYPRLAND_MOD_PRESETS_DIR") {
        return PathBuf::from(env);
    }
    dirs::data_local_dir()
        .map(|d| d.join("hyprland-mod").join("presets"))
        .unwrap_or_else(|| PathBuf::from(".local/share/hyprland-mod/presets"))
}

pub fn list_presets<P: AsRef<Path>>(custom_dir: Option<P>) -> Vec<Preset> {
    let mut map: BTreeMap<String, Preset> = BTreeMap::new();

    // 1. Embedded presets
    for p in embedded_presets() {
        map.insert(p.id.clone(), p);
    }

    // 2. Custom disk presets
    let dir = custom_dir.map(|p| p.as_ref().to_path_buf()).unwrap_or_else(presets_dir);
    if dir.is_dir() {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(text) = fs::read_to_string(&path) {
                        if let Ok(p) = serde_json::from_str::<Preset>(&text) {
                            if validate_preset(&p).is_ok() {
                                map.insert(p.id.clone(), p);
                            }
                        }
                    }
                }
            }
        }
    }

    map.into_values().collect()
}

pub fn find_preset(name_or_id: &str) -> Option<Preset> {
    let all = list_presets::<&str>(None);
    all.into_iter().find(|p| p.id.eq_ignore_ascii_case(name_or_id) || p.name.eq_ignore_ascii_case(name_or_id))
}

pub fn export_preset<P: AsRef<Path>>(preset: &Preset, path: P) -> Result<()> {
    let json = serde_json::to_string_pretty(preset)?;
    fs::write(path, json)?;
    Ok(())
}

pub fn import_preset<P: AsRef<Path>>(path: P) -> Result<Preset> {
    let text = fs::read_to_string(path)?;
    let preset: Preset = serde_json::from_str(&text)?;
    validate_preset(&preset)?;
    Ok(preset)
}

fn embedded_presets() -> Vec<Preset> {
    let data: &[(&str, &str)] = &[
        ("clean", include_str!("../../data/presets/clean.json")),
        ("glass", include_str!("../../data/presets/glass.json")),
        ("zen", include_str!("../../data/presets/zen.json")),
        ("nord", include_str!("../../data/presets/nord.json")),
        ("gruvbox", include_str!("../../data/presets/gruvbox.json")),
        ("tokyo-night", include_str!("../../data/presets/tokyo-night.json")),
        ("everforest", include_str!("../../data/presets/everforest.json")),
        ("monochrome", include_str!("../../data/presets/monochrome.json")),
        ("neon", include_str!("../../data/presets/neon.json")),
        ("bubbly", include_str!("../../data/presets/bubbly.json")),
        ("retro", include_str!("../../data/presets/retro.json")),
        ("catppuccin-latte", include_str!("../../data/presets/catppuccin-latte.json")),
        ("dracula-teal", include_str!("../../data/presets/dracula-teal.json")),
    ];

    let mut out = Vec::new();
    for (_name, json_str) in data {
        if let Ok(preset) = serde_json::from_str::<Preset>(json_str) {
            out.push(preset);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_presets() {
        let presets = list_presets::<&str>(None);
        assert!(!presets.is_empty());
        let ids: Vec<&str> = presets.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"clean"));
        assert!(ids.contains(&"nord"));
        assert!(ids.contains(&"tokyo-night"));
    }

    #[test]
    fn test_validate_preset() {
        let mut p = Preset::default();
        p.settings.insert("general:gaps_in".into(), "10".into());
        assert!(validate_preset(&p).is_ok());

        p.settings.insert("exec:malicious".into(), "rm -rf".into());
        assert!(validate_preset(&p).is_err());
    }
}
