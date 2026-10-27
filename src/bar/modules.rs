//! Catalog of modules and their Waybar configs.

use serde_json::{json, Value};
use std::collections::BTreeMap;

pub fn module_waybar_name(mod_type: &str) -> &'static str {
    match mod_type {
        "workspaces" => "hyprland/workspaces",
        "window" => "hyprland/window",
        "clock" => "clock",
        "battery" => "battery",
        "cpu" => "cpu",
        "memory" => "memory",
        "pulseaudio" => "pulseaudio",
        "network" => "network",
        "bluetooth" => "bluetooth",
        "backlight" => "backlight",
        "power" => "custom/power",
        "tray" => "tray",
        _ => "custom/unknown",
    }
}

pub fn default_module_config(
    mod_type: &str,
    _options: &BTreeMap<String, Value>,
    icons: bool,
) -> Value {
    match mod_type {
        "workspaces" => json!({
            "format": "{name}",
            "on-click": "activate",
            "sort-by-number": true
        }),
        "window" => json!({
            "format": "{}",
            "max-length": 40
        }),
        "clock" => json!({
            "format": if icons { "󰥔 {:%H:%M}" } else { "{:%H:%M}" },
            "tooltip-format": "{:%A, %B %d, %Y}"
        }),
        "battery" => json!({
            "format": if icons { "󰁹 {capacity}%" } else { "{capacity}%" },
            "format-charging": if icons { "󰂄 {capacity}%" } else { "{capacity}%+" }
        }),
        "cpu" => json!({
            "format": if icons { "󰻠 {usage}%" } else { "CPU {usage}%" },
            "interval": 2
        }),
        "memory" => json!({
            "format": if icons { "󰍛 {percentage}%" } else { "RAM {percentage}%" },
            "interval": 2
        }),
        "pulseaudio" => json!({
            "format": if icons { "󰕾 {volume}%" } else { "VOL {volume}%" },
            "format-muted": if icons { "󰝟 muted" } else { "muted" },
            "on-click": "pavucontrol"
        }),
        "network" => json!({
            "format-wifi": if icons { "󰤨 {essid}" } else { "WiFi {essid}" },
            "format-ethernet": if icons { "󰈀 {ipaddr}" } else { "ETH {ipaddr}" },
            "format-disconnected": if icons { "󰤭 off" } else { "offline" }
        }),
        "tray" => json!({
            "spacing": 10
        }),
        _ => json!({}),
    }
}
