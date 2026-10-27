pub mod generate;
pub mod model;
pub mod modules;
pub mod process;

pub use generate::{generate_config, generate_style, waybar_dir, write_bar_files};
pub use model::{Bar, BarModuleEntry, BarModules};
pub use process::{start_waybar, stop_waybar};
