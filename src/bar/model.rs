//! Waybar model definitions and validation.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BarModuleEntry {
    pub r#type: String,
    #[serde(default)]
    pub options: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BarModules {
    #[serde(default)]
    pub left: Vec<BarModuleEntry>,
    #[serde(default)]
    pub center: Vec<BarModuleEntry>,
    #[serde(default)]
    pub right: Vec<BarModuleEntry>,
}

impl Default for BarModules {
    fn default() -> Self {
        Self {
            left: vec![BarModuleEntry {
                r#type: "workspaces".into(),
                options: BTreeMap::new(),
            }],
            center: vec![BarModuleEntry {
                r#type: "clock".into(),
                options: BTreeMap::new(),
            }],
            right: vec![
                BarModuleEntry {
                    r#type: "cpu".into(),
                    options: BTreeMap::new(),
                },
                BarModuleEntry {
                    r#type: "memory".into(),
                    options: BTreeMap::new(),
                },
                BarModuleEntry {
                    r#type: "pulseaudio".into(),
                    options: BTreeMap::new(),
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Bar {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_position")]
    pub position: String, // "top" or "bottom"
    #[serde(default = "default_height")]
    pub height: u32,
    #[serde(default = "default_spacing")]
    pub spacing: u32,
    #[serde(default = "default_floating")]
    pub floating: bool,
    #[serde(default = "default_margin")]
    pub margin: u32,
    #[serde(default = "default_icons")]
    pub icons: bool,
    #[serde(default)]
    pub modules: BarModules,
    #[serde(default = "default_bg")]
    pub bg_color: String,
    #[serde(default = "default_fg")]
    pub fg_color: String,
    #[serde(default = "default_radius")]
    pub radius: u32,
}

fn default_name() -> String { "default".to_string() }
fn default_position() -> String { "top".to_string() }
fn default_height() -> u32 { 34 }
fn default_spacing() -> u32 { 6 }
fn default_floating() -> bool { true }
fn default_margin() -> u32 { 8 }
fn default_icons() -> bool { true }
fn default_bg() -> String { "rgba(24, 24, 37, 0.90)".to_string() }
fn default_fg() -> String { "#cdd6f4".to_string() }
fn default_radius() -> u32 { 10 }

impl Default for Bar {
    fn default() -> Self {
        Self {
            name: default_name(),
            position: default_position(),
            height: default_height(),
            spacing: default_spacing(),
            floating: default_floating(),
            margin: default_margin(),
            icons: default_icons(),
            modules: BarModules::default(),
            bg_color: default_bg(),
            fg_color: default_fg(),
            radius: default_radius(),
        }
    }
}
