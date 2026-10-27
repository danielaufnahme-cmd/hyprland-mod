//! Managing the Waybar background process.

use anyhow::{bail, Result};
use std::path::Path;
use std::process::Command;

use super::generate::waybar_dir;

pub fn start_waybar<P: AsRef<Path>>(bar_dir: Option<P>) -> Result<()> {
    let dir = bar_dir
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(waybar_dir);
    let cfg = dir.join("config.jsonc");
    let css = dir.join("style.css");

    // Kill any existing waybar running this config
    stop_waybar();

    let child = Command::new("waybar")
        .arg("-c")
        .arg(&cfg)
        .arg("-s")
        .arg(&css)
        .spawn();

    match child {
        Ok(_) => Ok(()),
        Err(e) => bail!("Failed to start waybar: {}", e),
    }
}

pub fn stop_waybar() {
    let _ = Command::new("killall").arg("waybar").output();
}
