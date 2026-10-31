//! Core engine of Hyprland Mod: parse, stage, write, live-apply, back up and undo
//! Hyprland settings. No UI code lives here.
//!
//! On-disk formats (managed ``hyprland-mod.conf``/``.lua``, backups, history,
//! presets, setup files) are byte-compatible with the Python version.

pub mod actions;
pub mod backup;
pub mod fsutil;
pub mod hyprctl;
pub mod items;
pub mod lua;
pub mod managed;
pub mod manager;
pub mod parser;
pub mod presets;
pub mod pyjson;
pub mod schema;
pub mod util;
pub mod values;

pub use backup::Backup;
pub use hyprctl::{HyprctlRunner, RunOutput, Runner};
pub use manager::{ApplyResult, ConfigManager, Event, ManagerOptions, Mode, SubscriptionId};
pub use presets::{Change, Preset, PresetError, PresetStore};
pub use schema::{app_data_dir, load_schema, Choice, Group, OptionSpec, OptionType, Page};
pub use values::{format_value, parse_color, parse_value, Value};
