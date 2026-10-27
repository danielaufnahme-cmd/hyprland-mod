use hyprland_mod::bar::{generate_config, generate_style, Bar};
use hyprland_mod::core::manager::{ConfigManager, ConfigMode};
use hyprland_mod::core::schema::load_schema;
use hyprland_mod::items::{Bind, Monitor, WindowRule};
use hyprland_mod::presets::{list_presets, validate_preset};
use tempfile::tempdir;

#[test]
fn test_config_manager_lifecycle_conf() {
    let tmp = tempdir().unwrap();
    let config_file = tmp.path().join("hyprland.conf");
    let managed_file = tmp.path().join("hyprland-mod.conf");
    let data_dir = tmp.path().join("data");

    std::fs::write(
        &config_file,
        r#"
general {
    gaps_in = 5
    gaps_out = 10
}
"#,
    )
    .unwrap();

    let mut mgr =
        ConfigManager::new(Some(&config_file), Some(&managed_file), Some(&data_dir), true).unwrap();

    assert_eq!(mgr.mode, ConfigMode::Conf);
    assert_eq!(mgr.get("general:gaps_in"), Some("5".to_string()));

    // Stage changes
    mgr.set("general:gaps_in", "15").unwrap();
    mgr.set("decoration:rounding", "10").unwrap();
    assert!(mgr.is_dirty());
    assert_eq!(mgr.get("general:gaps_in"), Some("15".to_string()));

    // Apply
    let res = mgr.apply().unwrap();
    assert!(res.ok);
    assert!(res.backup_id.is_some());
    assert!(!mgr.is_dirty());

    // Verify written file
    let managed_text = std::fs::read_to_string(&managed_file).unwrap();
    assert!(managed_text.contains("gaps_in = 15"));
    assert!(managed_text.contains("rounding = 10"));

    // Verify include in user config
    let main_text = std::fs::read_to_string(&config_file).unwrap();
    assert!(main_text.contains("source = "));
    assert!(main_text.contains("hyprland-mod.conf"));

    // Verify backup
    let backups = mgr.backups.list();
    assert_eq!(backups.len(), 1);

    // Reset option
    mgr.reset("general:gaps_in");
    assert!(mgr.is_dirty());
    let res2 = mgr.apply().unwrap();
    assert!(res2.ok);

    let managed_text2 = std::fs::read_to_string(&managed_file).unwrap();
    assert!(!managed_text2.contains("gaps_in = 15"));
}

#[test]
fn test_config_manager_lifecycle_lua() {
    let tmp = tempdir().unwrap();
    let config_file = tmp.path().join("hyprland.lua");
    let managed_file = tmp.path().join("hyprland-mod.lua");
    let data_dir = tmp.path().join("data");

    std::fs::write(&config_file, "-- user lua config\nreturn {}\n").unwrap();

    let mut mgr =
        ConfigManager::new(Some(&config_file), Some(&managed_file), Some(&data_dir), true).unwrap();

    assert_eq!(mgr.mode, ConfigMode::Lua);

    mgr.set("general:gaps_in", "12").unwrap();
    let res = mgr.apply().unwrap();
    assert!(res.ok);

    let main_text = std::fs::read_to_string(&config_file).unwrap();
    assert!(main_text.contains("pcall(require, \"hyprland-mod\")"));

    let mod_text = std::fs::read_to_string(&managed_file).unwrap();
    assert!(mod_text.contains("mod[\"general.gaps_in\"] = 12"));
}

#[test]
fn test_bar_builder_output() {
    let bar = Bar::default();
    let cfg = generate_config(&bar);
    assert!(cfg.contains("\"layer\": \"top\""));
    assert!(cfg.contains("\"position\": \"top\""));
    assert!(cfg.contains("\"hyprland/workspaces\""));
    assert!(cfg.contains("\"clock\""));

    let css = generate_style(&bar);
    assert!(css.contains("window#waybar"));
    assert!(css.contains("#workspaces button"));
}

#[test]
fn test_schema_coverage() {
    let pages = load_schema::<&str>(None);
    assert_eq!(pages.len(), 12);
    let mut total_opts = 0;
    for p in &pages {
        for g in &p.groups {
            total_opts += g.options.len();
        }
    }
    assert!(total_opts > 300, "expected >300 settings, found {}", total_opts);
}

#[test]
fn test_presets_validity() {
    let presets = list_presets::<&str>(None);
    assert!(!presets.is_empty());
    for p in presets {
        assert!(validate_preset(&p).is_ok(), "Preset {} failed validation", p.id);
    }
}

#[test]
fn test_items_formatting() {
    let bind = Bind {
        mods: vec!["SUPER".into(), "SHIFT".into()],
        key: "Q".into(),
        dispatcher: "killactive".into(),
        args: "".into(),
        description: "Close active".into(),
        flags: "".into(),
    };
    assert_eq!(bind.to_config_line(), "bind = SUPER SHIFT, Q, killactive, ");

    let rule = WindowRule {
        rule: "float".into(),
        pattern: "class:^(kitty)$".into(),
        is_v2: true,
    };
    assert_eq!(rule.to_config_line(), "windowrulev2 = float, class:^(kitty)$");

    let mon = Monitor {
        name: "DP-1".into(),
        resolution: "2560x1440@165".into(),
        position: "0x0".into(),
        scale: "1.0".into(),
        transform: Some(1),
        disabled: false,
    };
    assert_eq!(mon.to_config_line(), "monitor = DP-1, 2560x1440@165, 0x0, 1.0, transform, 1");
}
