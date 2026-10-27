//! Command Line Interface definitions and dispatcher.

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use colored::Colorize;
use std::path::PathBuf;

use crate::bar::{start_waybar, write_bar_files, Bar};
use crate::core::manager::ConfigManager;
use crate::core::schema::load_schema;
use crate::presets::{export_preset, find_preset, import_preset, list_presets, validate_preset};

#[derive(Parser, Debug)]
#[command(
    name = "hyprland-mod",
    about = "Every Hyprland setting and a whole rice, one click away.",
    version
)]
pub struct Cli {
    #[arg(long, global = true, help = "Path to hyprland.conf or hyprland.lua")]
    pub config: Option<PathBuf>,

    #[arg(long, global = true, help = "Path to hyprland-mod.conf or hyprland-mod.lua")]
    pub managed: Option<PathBuf>,

    #[arg(long, global = true, help = "Data directory for backups and presets")]
    pub data_dir: Option<PathBuf>,

    #[arg(long, global = true, help = "Run without applying live changes to Hyprland")]
    pub dry_run: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    #[command(about = "Show current status, active mode, and pending changes")]
    Status,

    #[command(about = "Get a configuration value")]
    Get { key: String },

    #[command(about = "Stage a configuration setting")]
    Set { key: String, value: String },

    #[command(about = "Reset a configuration setting to default")]
    Reset { key: String },

    #[command(about = "Apply staged changes to disk and reload Hyprland")]
    Apply,

    #[command(about = "Manage and apply theme presets")]
    Preset {
        #[command(subcommand)]
        action: PresetCommands,
    },

    #[command(about = "Waybar bar builder and process controls")]
    Bar {
        #[command(subcommand)]
        action: BarCommands,
    },

    #[command(about = "Manage automatic and manual backups")]
    Backup {
        #[command(subcommand)]
        action: BackupCommands,
    },

    #[command(about = "Inspect available settings schemas")]
    Schema {
        #[command(subcommand)]
        action: SchemaCommands,
    },
}

#[derive(Subcommand, Debug)]
pub enum PresetCommands {
    #[command(about = "List all available presets")]
    List,

    #[command(about = "Apply a preset by ID or name")]
    Apply { name: String },

    #[command(about = "Export a preset to a JSON file")]
    Export { name: String, out: PathBuf },

    #[command(about = "Import a preset from a JSON file")]
    Import { file: PathBuf },
}

#[derive(Subcommand, Debug)]
pub enum BarCommands {
    #[command(about = "Generate Waybar config and launch")]
    Start,

    #[command(about = "Stop running Waybar")]
    Stop,

    #[command(about = "Generate Waybar config.jsonc and style.css files")]
    Generate {
        #[arg(short, long, help = "Output directory")]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BackupCommands {
    #[command(about = "List backups")]
    List,

    #[command(about = "Restore a backup by ID")]
    Restore { id: String },
}

#[derive(Subcommand, Debug)]
pub enum SchemaCommands {
    #[command(about = "List all schema pages")]
    Pages,

    #[command(about = "List options under a page or all pages")]
    Options { page: Option<String> },
}

pub fn run_cli(cli: Cli) -> Result<()> {
    let mut mgr = ConfigManager::new(
        cli.config.as_deref(),
        cli.managed.as_deref(),
        cli.data_dir.as_deref(),
        cli.dry_run,
    )?;

    let command = match cli.command {
        Some(cmd) => cmd,
        None => Commands::Status,
    };

    match command {
        Commands::Status => {
            println!("{}", "=== Hyprland Mod ===".bold().cyan());
            println!("Config Path:  {}", mgr.config_path.display());
            println!("Managed Path: {}", mgr.managed_path.display());
            println!("Mode:         {:?}", mgr.mode);
            println!("Data Dir:     {}", mgr.data_dir.display());

            let changes = mgr.pending_changes();
            if changes.is_empty() {
                println!("Staged:       {}", "none".green());
            } else {
                println!("Staged Changes ({}):", changes.len());
                for (k, v) in changes {
                    if let Some(val) = v {
                        println!("  {} = {}", k.yellow(), val.green());
                    } else {
                        println!("  {} (reset)", k.red());
                    }
                }
            }

            let backups = mgr.backups.list();
            println!("Backups:      {}", backups.len());
        }

        Commands::Get { key } => {
            if let Some(val) = mgr.get(&key) {
                println!("{}", val);
            } else {
                eprintln!("{}: key not found", "error".red());
                std::process::exit(1);
            }
        }

        Commands::Set { key, value } => {
            mgr.set(&key, &value)?;
            println!("Staged: {} = {}", key.yellow(), value.green());
            println!("Run {} to write changes and reload compositor.", "hyprland-mod apply".bold());
        }

        Commands::Reset { key } => {
            mgr.reset(&key);
            println!("Staged reset for {}", key.yellow());
        }

        Commands::Apply => {
            let res = mgr.apply()?;
            if res.ok {
                println!("{}", "Successfully applied changes!".green().bold());
                if let Some(bid) = res.backup_id {
                    println!("Backup created: {}", bid);
                }
                if res.live_applied {
                    println!("Compositor reloaded instantly via hyprctl.");
                }
            } else {
                eprintln!("{}", "Apply completed with errors:".red().bold());
                for err in res.errors {
                    eprintln!("  - {}", err);
                }
            }
        }

        Commands::Preset { action } => match action {
            PresetCommands::List => {
                println!("{}", "Available Presets:".bold().cyan());
                for p in list_presets::<&str>(None) {
                    println!("  - {:<18} {}", p.id.yellow(), p.name.bold());
                    if !p.description.is_empty() {
                        println!("    {}", p.description.dimmed());
                    }
                }
            }

            PresetCommands::Apply { name } => {
                let preset = find_preset(&name)
                    .ok_or_else(|| anyhow!("Preset {:?} not found", name))?;
                validate_preset(&preset)?;

                for (k, v) in &preset.settings {
                    mgr.set(k, v)?;
                }

                let res = mgr.apply()?;
                if res.ok {
                    println!("Applied preset: {}", preset.name.green().bold());
                } else {
                    eprintln!("Failed to apply preset: {:?}", res.errors);
                }
            }

            PresetCommands::Export { name, out } => {
                let preset = find_preset(&name)
                    .ok_or_else(|| anyhow!("Preset {:?} not found", name))?;
                export_preset(&preset, &out)?;
                println!("Exported preset {} to {}", name, out.display());
            }

            PresetCommands::Import { file } => {
                let preset = import_preset(&file)?;
                println!("Successfully verified and imported preset: {}", preset.name.green());
            }
        },

        Commands::Bar { action } => match action {
            BarCommands::Start => {
                let bar = Bar::default();
                write_bar_files(&bar, None::<&str>)?;
                start_waybar(None::<&str>)?;
                println!("{}", "Waybar started with Hyprland Mod config!".green());
            }

            BarCommands::Stop => {
                crate::bar::stop_waybar();
                println!("Stopped Waybar.");
            }

            BarCommands::Generate { out } => {
                let bar = Bar::default();
                let dir = write_bar_files(&bar, out.as_deref())?;
                println!("Waybar files written to {}", dir.display().to_string().green());
            }
        },

        Commands::Backup { action } => match action {
            BackupCommands::List => {
                let list = mgr.backups.list();
                if list.is_empty() {
                    println!("No backups found.");
                } else {
                    println!("{}", "Backups:".bold().cyan());
                    for b in list {
                        println!("  {}  [{}] ({})", b.id.yellow(), b.created, b.label);
                    }
                }
            }

            BackupCommands::Restore { id } => {
                let files = mgr.backups.restore(&id)?;
                println!("{}", format!("Restored backup {} successfully:", id).green());
                for f in files {
                    println!("  - {}", f);
                }
                let _ = mgr.hyprctl.reload();
            }
        },

        Commands::Schema { action } => match action {
            SchemaCommands::Pages => {
                let pages = load_schema::<&str>(None);
                println!("{}", "Schema Pages:".bold().cyan());
                for p in pages {
                    println!("  [{:>2}] {:<14} {}", p.order as i64, p.id.yellow(), p.title.bold());
                }
            }

            SchemaCommands::Options { page } => {
                let pages = load_schema::<&str>(None);
                for p in pages {
                    if let Some(ref target) = page {
                        if !p.id.eq_ignore_ascii_case(target) {
                            continue;
                        }
                    }
                    println!("\n=== {} ({}) ===", p.title.bold(), p.id.cyan());
                    for g in p.groups {
                        if !g.title.is_empty() {
                            println!("  [Group: {}]", g.title.dimmed());
                        }
                        for opt in g.options {
                            println!("    {:<35} {:<10} {}", opt.key.yellow(), opt.r#type.dimmed(), opt.label);
                        }
                    }
                }
            }
        },
    }

    Ok(())
}
