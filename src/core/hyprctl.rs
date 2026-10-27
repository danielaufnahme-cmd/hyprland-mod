//! Interface to Hyprland's IPC (`hyprctl`).

use anyhow::{bail, Result};
use std::process::Command;

pub trait HyprctlRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> Result<String>;
}

pub struct SystemRunner {
    pub dry_run: bool,
}

impl HyprctlRunner for SystemRunner {
    fn run(&self, args: &[&str]) -> Result<String> {
        if self.dry_run || std::env::var("HYPRLAND_MOD_DRY_RUN").is_ok() {
            return Ok("ok".to_string());
        }

        let output = Command::new("hyprctl")
            .args(args)
            .output()?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            bail!("hyprctl failed: {}", err);
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }
}

pub struct Hyprctl {
    runner: Box<dyn HyprctlRunner>,
}

impl Hyprctl {
    pub fn new(dry_run: bool) -> Self {
        Self {
            runner: Box::new(SystemRunner { dry_run }),
        }
    }

    pub fn with_runner(runner: Box<dyn HyprctlRunner>) -> Self {
        Self { runner }
    }

    pub fn keyword(&self, key: &str, value: &str) -> Result<()> {
        self.runner.run(&["keyword", key, value])?;
        Ok(())
    }

    pub fn reload(&self) -> Result<()> {
        self.runner.run(&["reload"])?;
        Ok(())
    }

    pub fn monitors(&self) -> Result<serde_json::Value> {
        let out = self.runner.run(&["-j", "monitors"])?;
        let json = serde_json::from_str(&out)?;
        Ok(json)
    }

    pub fn binds(&self) -> Result<serde_json::Value> {
        let out = self.runner.run(&["-j", "binds"])?;
        let json = serde_json::from_str(&out)?;
        Ok(json)
    }

    pub fn descriptions(&self) -> Result<serde_json::Value> {
        let out = self.runner.run(&["-j", "descriptions"])?;
        let json = serde_json::from_str(&out)?;
        Ok(json)
    }
}
