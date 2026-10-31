//! Waybar module catalog: per-module options, generated config and preview labels.
//!
//! Every module the bar builder offers lives in [`MODULES`]. Each option has a
//! kind (bool/int/choice) so the same spec drives validation, the option rows in
//! the UI and the generated Waybar config. Users never type free text here, which
//! keeps every value that reaches config.jsonc/style.css tame.

use chrono::NaiveDate;
use serde_json::{json, Map, Value as Json};

/// Options of one module entry (``{"format": "24h", ...}``), in file order.
pub type Opts = Map<String, Json>;

/// Nerd Font v3 glyphs (Material Design range).
pub mod glyph {
    pub const BATTERY: [&str; 11] = [
        "\u{F008E}",
        "\u{F007A}",
        "\u{F007B}",
        "\u{F007C}",
        "\u{F007D}",
        "\u{F007E}",
        "\u{F007F}",
        "\u{F0080}",
        "\u{F0081}",
        "\u{F0082}",
        "\u{F0079}",
    ];
    pub const CHARGING: &str = "\u{F0084}";
    pub const WIFI: &str = "\u{F0928}";
    pub const ETHERNET: &str = "\u{F0200}";
    pub const OFFLINE: &str = "\u{F092D}";
    pub const VOLUME: [&str; 3] = ["\u{F057F}", "\u{F0580}", "\u{F057E}"];
    pub const MUTED: &str = "\u{F075F}";
    pub const CPU: &str = "\u{F035B}";
    pub const MEMORY: &str = "\u{F061A}";
    pub const TEMPERATURE: &str = "\u{F050F}";
    pub const BACKLIGHT: &str = "\u{F00E0}";
    pub const BLUETOOTH: &str = "\u{F00AF}";
    pub const BT_CONNECTED: &str = "\u{F00B1}";
    pub const BT_OFF: &str = "\u{F00B2}";
    pub const POWER: &str = "\u{F0425}";
    pub const COFFEE: &str = "\u{F0176}";
    pub const COFFEE_OFF: &str = "\u{F0FAA}";
    pub const MUSIC: &str = "\u{F0386}";
    pub const KEYBOARD: &str = "\u{F030C}";
    pub const LOCK: &str = "\u{F033E}";
    pub const UNLOCK: &str = "\u{F033F}";
}
use glyph as g;

pub const ROMAN: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];
pub const KANJI: [&str; 10] = ["一", "二", "三", "四", "五", "六", "七", "八", "九", "十"];

pub const CLOCK_FORMATS: &[(&str, &str)] = &[
    ("24h", "%H:%M"),
    ("12h", "%I:%M %p"),
    ("day-24h", "%a %d %b  %H:%M"),
    ("full", "%A, %B %d  %H:%M"),
    ("iso", "%Y-%m-%d  %H:%M"),
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    /// ``(value, label)`` pairs.
    Choice(&'static [(&'static str, &'static str)]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Default {
    Bool(bool),
    Int(i64),
    Str(&'static str),
}

impl Default {
    pub fn to_json(self) -> Json {
        match self {
            Default::Bool(b) => Json::Bool(b),
            Default::Int(i) => Json::from(i),
            Default::Str(s) => Json::from(s),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OptSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub default: Default,
    /// ``""`` when the option has no unit.
    pub unit: &'static str,
    /// ``""`` when the option has no description.
    pub description: &'static str,
}

#[derive(Debug)]
pub struct ModuleSpec {
    /// Our id (``"workspaces"``).
    pub id: &'static str,
    /// The module name in config.jsonc.
    pub waybar: &'static str,
    /// The ``#id`` Waybar gives its widget.
    pub css: &'static str,
    pub label: &'static str,
    pub icon: &'static str,
    pub description: &'static str,
    pub options: &'static [OptSpec],
}

const fn b(key: &'static str, label: &'static str, default: bool) -> OptSpec {
    OptSpec { key, label, kind: Kind::Bool, default: Default::Bool(default), unit: "", description: "" }
}

const fn i(key: &'static str, label: &'static str, default: i64, min: i64, max: i64, unit: &'static str) -> OptSpec {
    OptSpec { key, label, kind: Kind::Int { min, max }, default: Default::Int(default), unit, description: "" }
}

const fn c(
    key: &'static str,
    label: &'static str,
    default: &'static str,
    choices: &'static [(&'static str, &'static str)],
) -> OptSpec {
    OptSpec { key, label, kind: Kind::Choice(choices), default: Default::Str(default), unit: "", description: "" }
}

const VOLUME_OPTS: &[OptSpec] = &[b("percent", "Show percentage", true), i("step", "Scroll step", 5, 1, 20, "%")];

/// Every module, in catalog order (the "Add Module" picker lists them like this).
pub static MODULES: &[ModuleSpec] = &[
    ModuleSpec {
        id: "workspaces",
        waybar: "hyprland/workspaces",
        css: "workspaces",
        label: "Workspaces",
        icon: "view-grid-symbolic",
        description: "Buttons for your workspaces",
        options: &[
            c(
                "style",
                "Style",
                "numbers",
                &[("numbers", "Numbers"), ("dots", "Dots"), ("roman", "Roman numerals"), ("kanji", "Kanji")],
            ),
            OptSpec {
                description: "Keep this many workspaces visible even when empty",
                ..i("persistent", "Always show", 5, 0, 10, "workspaces")
            },
            b("scroll", "Scroll to switch", true),
        ],
    },
    ModuleSpec {
        id: "window",
        waybar: "hyprland/window",
        css: "window",
        label: "Window title",
        icon: "focus-windows-symbolic",
        description: "Title of the focused window",
        options: &[i("max_length", "Longest title", 48, 10, 120, "characters")],
    },
    ModuleSpec {
        id: "clock",
        waybar: "clock",
        css: "clock",
        label: "Clock",
        icon: "preferences-system-time-symbolic",
        description: "Time and date",
        options: &[
            c(
                "format",
                "Format",
                "24h",
                &[
                    ("24h", "14:32"),
                    ("12h", "2:32 PM"),
                    ("day-24h", "Tue 21 Oct  14:32"),
                    ("full", "Tuesday, October 21  14:32"),
                    ("iso", "2026-10-21  14:32"),
                ],
            ),
            b("seconds", "Show seconds", false),
            b("calendar", "Calendar on hover", true),
        ],
    },
    ModuleSpec {
        id: "battery",
        waybar: "battery",
        css: "battery",
        label: "Battery",
        icon: "battery-good-symbolic",
        description: "Charge level and warnings",
        options: &[
            i("warning", "Warning below", 30, 5, 60, "%"),
            i("critical", "Critical below", 15, 3, 40, "%"),
            b("percent", "Show percentage", true),
        ],
    },
    ModuleSpec {
        id: "network",
        waybar: "network",
        css: "network",
        label: "Network",
        icon: "network-wireless-symbolic",
        description: "Wi-Fi or wired connection",
        options: &[c("label", "Show", "ssid", &[("icon", "Icon only"), ("ssid", "Network name"), ("ip", "IP address")])],
    },
    ModuleSpec {
        id: "pulseaudio",
        waybar: "pulseaudio",
        css: "pulseaudio",
        label: "Volume",
        icon: "audio-volume-high-symbolic",
        description: "Volume (PulseAudio / PipeWire)",
        options: VOLUME_OPTS,
    },
    ModuleSpec {
        id: "wireplumber",
        waybar: "wireplumber",
        css: "wireplumber",
        label: "Volume (WirePlumber)",
        icon: "audio-speakers-symbolic",
        description: "Volume straight from WirePlumber",
        options: VOLUME_OPTS,
    },
    ModuleSpec {
        id: "cpu",
        waybar: "cpu",
        css: "cpu",
        label: "CPU",
        icon: "power-profile-performance-symbolic",
        description: "Processor usage",
        options: &[i("interval", "Update every", 5, 1, 60, "s")],
    },
    ModuleSpec {
        id: "memory",
        waybar: "memory",
        css: "memory",
        label: "Memory",
        icon: "drive-multidisk-symbolic",
        description: "RAM in use",
        options: &[i("interval", "Update every", 10, 1, 60, "s")],
    },
    ModuleSpec {
        id: "temperature",
        waybar: "temperature",
        css: "temperature",
        label: "Temperature",
        icon: "weather-clear-symbolic",
        description: "CPU temperature",
        options: &[i("critical", "Hot above", 80, 50, 110, "°C")],
    },
    ModuleSpec {
        id: "tray",
        waybar: "tray",
        css: "tray",
        label: "System tray",
        icon: "view-app-grid-symbolic",
        description: "Icons of background apps",
        options: &[i("icon_size", "Icon size", 16, 12, 32, "px"), i("spacing", "Icon spacing", 8, 0, 20, "px")],
    },
    ModuleSpec {
        id: "backlight",
        waybar: "backlight",
        css: "backlight",
        label: "Brightness",
        icon: "display-brightness-symbolic",
        description: "Screen brightness",
        options: &[b("percent", "Show percentage", true)],
    },
    ModuleSpec {
        id: "bluetooth",
        waybar: "bluetooth",
        css: "bluetooth",
        label: "Bluetooth",
        icon: "bluetooth-active-symbolic",
        description: "Bluetooth state",
        options: &[b("device", "Show connected device", false)],
    },
    ModuleSpec {
        id: "power",
        waybar: "custom/power",
        css: "custom-power",
        label: "Power menu",
        icon: "system-shutdown-symbolic",
        description: "Lock, log out, restart, shut down",
        options: &[b("lock", "Lock in menu", true), b("suspend", "Suspend in menu", true)],
    },
    ModuleSpec {
        id: "idle_inhibitor",
        waybar: "idle_inhibitor",
        css: "idle_inhibitor",
        label: "Stay awake",
        icon: "night-light-symbolic",
        description: "Keep the screen on with one click",
        options: &[],
    },
    ModuleSpec {
        id: "mpris",
        waybar: "mpris",
        css: "mpris",
        label: "Now playing",
        icon: "media-playback-start-symbolic",
        description: "Song from any media player",
        options: &[i("max_length", "Longest text", 30, 10, 80, "characters")],
    },
    ModuleSpec {
        id: "keyboard_state",
        waybar: "keyboard-state",
        css: "keyboard-state",
        label: "Caps & Num Lock",
        icon: "input-keyboard-symbolic",
        description: "Shows when a lock key is on",
        options: &[b("capslock", "Caps Lock", true), b("numlock", "Num Lock", false)],
    },
    ModuleSpec {
        id: "language",
        waybar: "hyprland/language",
        css: "language",
        label: "Keyboard layout",
        icon: "preferences-desktop-locale-symbolic",
        description: "Current keyboard language",
        options: &[c("style", "Show", "short", &[("short", "Short (EN)"), ("long", "Full name")])],
    },
];

/// ``(id, label, command)`` of the power menu, in menu order.
pub const POWER_ACTIONS: &[(&str, &str, &str)] = &[
    ("lock", "Lock", "loginctl lock-session"),
    ("suspend", "Suspend", "systemctl suspend"),
    ("logout", "Log Out", "hyprctl dispatch exit"),
    ("reboot", "Restart", "systemctl reboot"),
    ("shutdown", "Shut Down", "systemctl poweroff"),
];

/// The catalog entry for a module id.
pub fn module(id: &str) -> Option<&'static ModuleSpec> {
    MODULES.iter().find(|m| m.id == id)
}

/// The catalog entry for an id that was already validated.
///
/// Panics on an unknown id: only call it with types taken from a validated [`crate::Bar`].
pub fn spec(id: &str) -> &'static ModuleSpec {
    module(id).unwrap_or_else(|| panic!("unknown bar module {id:?}"))
}

pub fn option_spec(mtype: &str, key: &str) -> Option<&'static OptSpec> {
    module(mtype)?.options.iter().find(|o| o.key == key)
}

pub fn defaults(mtype: &str) -> Opts {
    module(mtype).map(|m| m.options.iter().map(|o| (o.key.to_string(), o.default.to_json())).collect()).unwrap_or_default()
}

/// ``{**defaults(mtype), **opts}``.
pub fn merged(mtype: &str, opts: &Opts) -> Opts {
    let mut o = defaults(mtype);
    for (k, v) in opts {
        o.insert(k.clone(), v.clone());
    }
    o
}

pub fn opt_bool(o: &Opts, key: &str) -> bool {
    o.get(key).and_then(Json::as_bool).unwrap_or(false)
}

pub fn opt_int(o: &Opts, key: &str) -> i64 {
    o.get(key).and_then(Json::as_i64).unwrap_or(0)
}

pub fn opt_str<'a>(o: &'a Opts, key: &str) -> &'a str {
    o.get(key).and_then(Json::as_str).unwrap_or("")
}

/// Power menu actions enabled by the power module's options (missing = on).
pub fn power_actions(opts: &Opts) -> Vec<(&'static str, &'static str, &'static str)> {
    POWER_ACTIONS.iter().copied().filter(|a| opts.get(a.0).and_then(Json::as_bool).unwrap_or(true)).collect()
}

/// The clock's strftime pattern for its options.
pub fn clock_strftime(opts: &Opts) -> String {
    let fmt = CLOCK_FORMATS
        .iter()
        .find(|(k, _)| opts.get("format").and_then(Json::as_str) == Some(k))
        .map(|(_, f)| *f)
        .unwrap_or(CLOCK_FORMATS[0].1);
    if opt_bool(opts, "seconds") {
        fmt.replacen("%M", "%M:%S", 1)
    } else {
        fmt.to_string()
    }
}

pub fn workspace_labels(style: &str) -> Vec<String> {
    match style {
        "roman" => ROMAN.iter().map(|s| s.to_string()).collect(),
        "kanji" => KANJI.iter().map(|s| s.to_string()).collect(),
        _ => (1..=10).map(|n| n.to_string()).collect(),
    }
}

// ---------------------------------------------------------------- waybar config

/// The block for one module in config.jsonc (``opts`` already validated).
pub fn module_config(mtype: &str, opts: &Opts, icons: bool, menu_file: &str) -> Json {
    let o = merged(mtype, opts);
    let ob = |k: &str| opt_bool(&o, k);
    let oi = |k: &str| opt_int(&o, k);
    let os = |k: &str| opt_str(&o, k).to_string();
    // ``cfg[key] = v`` keeps an existing key in place and appends new ones, like a Python dict
    match mtype {
        "workspaces" => {
            let mut cfg = json!({"disable-scroll": !ob("scroll"), "all-outputs": false, "on-click": "activate"});
            let style = os("style");
            if style == "numbers" {
                cfg["format"] = json!("{id}");
            } else if style == "dots" {
                cfg["format"] = json!("{icon}");
                cfg["format-icons"] =
                    json!({"active": "●", "urgent": "●", "default": "○", "empty": "○", "persistent": "○"});
            } else {
                cfg["format"] = json!("{icon}");
                let icons: Map<String, Json> = workspace_labels(&style)
                    .into_iter()
                    .enumerate()
                    .map(|(n, l)| ((n + 1).to_string(), Json::from(l)))
                    .collect();
                cfg["format-icons"] = Json::Object(icons);
            }
            if oi("persistent") != 0 {
                cfg["persistent-workspaces"] = json!({"*": oi("persistent")});
            }
            cfg
        }
        "window" => json!({"format": "{title}", "max-length": oi("max_length"), "separate-outputs": true}),
        "clock" => {
            let mut cfg = json!({
                "format": format!("{{:{}}}", clock_strftime(&o)),
                "format-alt": "{:%A, %d %B %Y}",
                "interval": if ob("seconds") { 1 } else { 60 },
            });
            if ob("calendar") {
                cfg["tooltip-format"] = json!("<tt><small>{calendar}</small></tt>");
                cfg["calendar"] = json!({"mode": "month", "weeks-pos": "", "on-scroll": 1});
            } else {
                cfg["tooltip"] = json!(false);
            }
            cfg
        }
        "battery" => {
            let pct = if ob("percent") { " {capacity}%" } else { "" };
            let mut cfg = json!({
                "states": {"warning": oi("warning"), "critical": oi("critical")},
                "tooltip-format": "{capacity}% — {timeTo}",
            });
            if icons {
                cfg["format"] = json!(format!("{{icon}}{pct}"));
                cfg["format-charging"] = json!(format!("{}{pct}", g::CHARGING));
                cfg["format-plugged"] = json!(format!("{}{pct}", g::CHARGING));
                cfg["format-icons"] = json!(g::BATTERY);
            } else {
                cfg["format"] = json!(format!("BAT{}", if pct.is_empty() { " {capacity}%" } else { pct }));
                cfg["format-charging"] = json!("CHR {capacity}%");
            }
            cfg
        }
        "network" => {
            let (label, wired) = match os("label").as_str() {
                "icon" => ("", ""),
                "ip" => (" {ipaddr}", " {ipaddr}"),
                _ => (" {essid}", " {ifname}"),
            };
            if icons {
                json!({
                    "format-wifi": format!("{}{label}", g::WIFI),
                    "format-ethernet": format!("{}{wired}", g::ETHERNET),
                    "format-disconnected": g::OFFLINE,
                    "format-linked": g::ETHERNET,
                    "tooltip-format": "{ifname} — {ipaddr}\n{essid} {signalStrength}%",
                })
            } else {
                json!({
                    "format-wifi": format!("WIFI{label}"),
                    "format-ethernet": format!("ETH{wired}"),
                    "format-disconnected": "OFFLINE",
                    "tooltip-format": "{ifname} — {ipaddr}",
                })
            }
        }
        "pulseaudio" | "wireplumber" => {
            let pct = if ob("percent") { " {volume}%" } else { "" };
            let mut cfg = if mtype == "pulseaudio" {
                json!({"scroll-step": oi("step"), "on-click": "pavucontrol", "tooltip-format": "{volume}% — {desc}"})
            } else {
                json!({"on-click": "pavucontrol", "tooltip-format": "{volume}% — {node_name}"})
            };
            if mtype == "wireplumber" {
                cfg["scroll-step"] = json!(oi("step") as f64);
            }
            if icons {
                cfg["format"] = json!(format!("{{icon}}{pct}"));
                cfg["format-muted"] = json!(g::MUTED);
                cfg["format-icons"] =
                    if mtype == "pulseaudio" { json!({"default": g::VOLUME}) } else { json!(g::VOLUME) };
            } else {
                cfg["format"] = json!(format!("VOL{}", if pct.is_empty() { " {volume}%" } else { pct }));
                cfg["format-muted"] = json!("MUTE");
            }
            cfg
        }
        "cpu" => json!({
            "format": format!("{} {{usage}}%", if icons { g::CPU } else { "CPU" }),
            "interval": oi("interval"),
        }),
        "memory" => json!({
            "format": format!("{} {{percentage}}%", if icons { g::MEMORY } else { "MEM" }),
            "interval": oi("interval"),
            "tooltip-format": "{used:0.1f} GiB of {total:0.1f} GiB",
        }),
        "temperature" => json!({
            "format": format!("{} {{temperatureC}}°C", if icons { g::TEMPERATURE } else { "TEMP" }),
            "critical-threshold": oi("critical"),
        }),
        "tray" => json!({"icon-size": oi("icon_size"), "spacing": oi("spacing")}),
        "backlight" => {
            let pct = if ob("percent") { " {percent}%" } else { "" };
            let text = if icons {
                format!("{}{pct}", g::BACKLIGHT)
            } else {
                format!("BRI{}", if pct.is_empty() { " {percent}%" } else { pct })
            };
            json!({ "format": text })
        }
        "bluetooth" => {
            let dev = if ob("device") { " {device_alias}" } else { "" };
            if icons {
                json!({
                    "format": g::BLUETOOTH,
                    "format-connected": format!("{}{dev}", g::BT_CONNECTED),
                    "format-disabled": g::BT_OFF,
                    "format-off": g::BT_OFF,
                    "tooltip-format-connected": "{device_enumerate}",
                    "on-click": "blueman-manager",
                })
            } else {
                json!({
                    "format": "BT",
                    "format-connected": format!("BT{}", if dev.is_empty() { " on" } else { dev }),
                    "format-off": "BT off",
                    "format-disabled": "BT off",
                    "on-click": "blueman-manager",
                })
            }
        }
        "power" => {
            let actions: Map<String, Json> =
                power_actions(&o).into_iter().map(|(id, _, cmd)| (id.to_string(), Json::from(cmd))).collect();
            json!({
                "format": if icons { g::POWER } else { "POWER" },
                "tooltip": false,
                "menu": "on-click",
                "menu-file": menu_file,
                "menu-actions": actions,
            })
        }
        "idle_inhibitor" => json!({
            "format": "{icon}",
            "format-icons": {
                "activated": if icons { g::COFFEE } else { "AWAKE" },
                "deactivated": if icons { g::COFFEE_OFF } else { "IDLE" },
            },
            "tooltip-format-activated": "Staying awake",
            "tooltip-format-deactivated": "Normal sleep",
        }),
        "mpris" => {
            let music = if icons { format!("{} ", g::MUSIC) } else { String::new() };
            json!({
                "format": format!("{music}{{dynamic}}"),
                "dynamic-len": oi("max_length"),
                "dynamic-order": ["title", "artist"],
                "format-paused": format!("{music}<i>{{dynamic}}</i>"),
            })
        }
        "keyboard_state" => json!({
            "capslock": ob("capslock"),
            "numlock": ob("numlock"),
            "format": {"capslock": "CAPS {icon}", "numlock": "NUM {icon}"},
            "format-icons": {
                "locked": if icons { g::LOCK } else { "on" },
                "unlocked": if icons { g::UNLOCK } else { "off" },
            },
        }),
        "language" => {
            let fmt = if os("style") == "short" { "{short}" } else { "{long}" };
            let kb = if icons { format!("{} ", g::KEYBOARD) } else { String::new() };
            json!({ "format": format!("{kb}{fmt}") })
        }
        other => panic!("unknown bar module {other:?}"),
    }
}

// ---------------------------------------------------------------- preview labels

fn ellipsize(text: &str, max: i64) -> String {
    let n = text.chars().count() as i64;
    if n <= max {
        text.to_string()
    } else {
        let keep: String = text.chars().take((max - 1).max(0) as usize).collect();
        format!("{keep}…")
    }
}

/// What the module roughly shows on a real bar; used by the live preview.
pub fn sample_label(mtype: &str, opts: &Opts, icons: bool) -> String {
    let o = merged(mtype, opts);
    let ic = |glyph: &str| if icons { format!("{glyph} ") } else { String::new() };
    match mtype {
        "window" => ellipsize("Firefox — Hyprland Wiki", opt_int(&o, "max_length")),
        "clock" => {
            let t = NaiveDate::from_ymd_opt(2026, 10, 21).and_then(|d| d.and_hms_opt(14, 32, 8)).unwrap_or_default();
            let text = t.format(&clock_strftime(&o)).to_string();
            if text.starts_with('0') && opt_str(&o, "format") == "12h" {
                text[1..].to_string()
            } else {
                text
            }
        }
        "battery" => {
            if icons {
                let pct = if opt_bool(&o, "percent") { " 87%" } else { "" };
                format!("{}{pct}", g::BATTERY[8])
            } else {
                "BAT 87%".into()
            }
        }
        "network" => {
            let tail = match opt_str(&o, "label") {
                "icon" => "",
                "ip" => "192.168.1.24",
                _ => "Home Wi-Fi",
            };
            if icons {
                if tail.is_empty() {
                    g::WIFI.into()
                } else {
                    format!("{} {tail}", g::WIFI)
                }
            } else {
                format!("WIFI {tail}").trim().to_string()
            }
        }
        "pulseaudio" | "wireplumber" => {
            if icons {
                format!("{}{}", g::VOLUME[2], if opt_bool(&o, "percent") { " 64%" } else { "" })
            } else {
                "VOL 64%".into()
            }
        }
        "cpu" => ic(g::CPU) + if icons { "12%" } else { "CPU 12%" },
        "memory" => ic(g::MEMORY) + if icons { "41%" } else { "MEM 41%" },
        "temperature" => ic(g::TEMPERATURE) + if icons { "52°C" } else { "TEMP 52°C" },
        "backlight" => {
            if icons {
                format!("{}{}", g::BACKLIGHT, if opt_bool(&o, "percent") { " 70%" } else { "" })
            } else {
                "BRI 70%".into()
            }
        }
        "bluetooth" => {
            let device = opt_bool(&o, "device");
            if icons {
                format!("{}{}", g::BT_CONNECTED, if device { " Headphones" } else { "" })
            } else if device {
                "BT Headphones".into()
            } else {
                "BT on".into()
            }
        }
        "power" => (if icons { g::POWER } else { "POWER" }).into(),
        "idle_inhibitor" => (if icons { g::COFFEE_OFF } else { "IDLE" }).into(),
        "mpris" => ic(g::MUSIC) + &ellipsize("Midnight City — M83", opt_int(&o, "max_length")),
        "keyboard_state" => {
            format!("{}{}", if opt_bool(&o, "capslock") { "CAPS " } else { "NUM " }, if icons { g::LOCK } else { "on" })
        }
        "language" => ic(g::KEYBOARD) + if opt_str(&o, "style") == "short" { "EN" } else { "English (US)" },
        other => module(other).map(|m| m.label).unwrap_or(other).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_labels() {
        let mut o = Opts::new();
        o.insert("format".into(), json!("12h"));
        assert_eq!(sample_label("clock", &o, true), "2:32 PM");
        o.insert("format".into(), json!("day-24h"));
        assert_eq!(sample_label("clock", &o, true), "Wed 21 Oct  14:32");
        o.insert("seconds".into(), json!(true));
        assert_eq!(clock_strftime(&o), "%a %d %b  %H:%M:%S");
    }

    #[test]
    fn catalog_is_consistent() {
        assert_eq!(MODULES.len(), 18);
        for m in MODULES {
            assert!(module_config(m.id, &Opts::new(), true, "/x").is_object());
            assert!(!sample_label(m.id, &Opts::new(), false).is_empty());
        }
    }
}
