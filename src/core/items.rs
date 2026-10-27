//! Data structures for keybinds, window rules, monitors, and animations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Bind {
    #[serde(default)]
    pub mods: Vec<String>,
    pub key: String,
    pub dispatcher: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub flags: String, // e.g. "l", "r", "e", "m"
}

impl Bind {
    pub fn to_config_line(&self) -> String {
        let keyword = if self.flags.is_empty() {
            "bind"
        } else {
            match self.flags.as_str() {
                "l" => "bindl",
                "r" => "bindr",
                "e" => "binde",
                "m" => "bindm",
                _ => "bind",
            }
        };
        let mods_str = self.mods.join(" ");
        format!("{} = {}, {}, {}, {}", keyword, mods_str, self.key, self.dispatcher, self.args)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WindowRule {
    pub rule: String,       // e.g. "float", "size 800 600", "opacity 0.9"
    pub pattern: String,    // regex pattern matching class or title
    #[serde(default)]
    pub is_v2: bool,
}

impl WindowRule {
    pub fn to_config_line(&self) -> String {
        if self.is_v2 {
            format!("windowrulev2 = {}, {}", self.rule, self.pattern)
        } else {
            format!("windowrule = {}, {}", self.rule, self.pattern)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Monitor {
    pub name: String,
    pub resolution: String, // e.g. "1920x1080@144"
    pub position: String,   // e.g. "0x0"
    pub scale: String,      // e.g. "1.0"
    #[serde(default)]
    pub transform: Option<u8>,
    #[serde(default)]
    pub disabled: bool,
}

impl Monitor {
    pub fn to_config_line(&self) -> String {
        if self.disabled {
            return format!("monitor = {}, disable", self.name);
        }
        let mut line = format!(
            "monitor = {}, {}, {}, {}",
            self.name, self.resolution, self.position, self.scale
        );
        if let Some(t) = self.transform {
            line.push_str(&format!(", transform, {}", t));
        }
        line
    }
}
