mod common;

use std::cell::RefCell;
use std::fs;
use std::rc::Rc;

use common::{args, s, strings, FakeRunner, TempConfig};
use hyprmod_core::manager::detect_config;
use hyprmod_core::parser::parse_config;
use hyprmod_core::{ApplyResult, ConfigManager, Event, ManagerOptions, Mode};
use serde_json::json;

type Seen = Rc<RefCell<Vec<(String, Option<String>)>>>;

fn recorder(cm: &mut ConfigManager) -> Seen {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    cm.subscribe(Box::new(move |e: &Event, k: Option<&str>| {
        sink.borrow_mut().push((e.name().to_string(), k.map(str::to_string)))
    }));
    seen
}

fn names(seen: &Seen) -> Vec<String> {
    seen.borrow().iter().map(|(e, _)| e.clone()).collect()
}

// ------------------------------------------------------------ defaults

#[test]
fn env_paths() {
    let t = TempConfig::conf();
    let cm = t.make();
    assert_eq!(cm.config_path(), t.config);
    assert_eq!(cm.managed_path(), t.managed);
    assert_eq!(cm.data_dir(), t.data);
    assert!(cm.dry_run());
    assert!(!cm.hyprland_running());
    assert_eq!(cm.mode(), Mode::Conf);
}

#[test]
fn default_paths_without_env() {
    // HOME points at a temp dir, so the real ~/.config/hypr is never even looked at.
    let t = TempConfig::conf();
    let home = t.tmp.path().join("home");
    fs::create_dir_all(home.join(".config/hypr")).unwrap();
    let mut env = t.env.clone();
    env.remove("HYPRLAND_MOD_CONFIG");
    env.remove("HYPRLAND_MOD_DATA");
    let make = || ConfigManager::new(ManagerOptions { env: Some(env.clone()), ..Default::default() }).unwrap();
    let cm = make();
    assert!(cm.config_path().ends_with(".config/hypr/hyprland.conf"));
    assert!(cm.managed_path().ends_with(".config/hypr/hyprland-mod.conf"));
    assert!(cm.data_dir().ends_with(".local/share/hyprland-mod"));
    assert!(cm.data_dir().starts_with(&home));
    assert_eq!(cm.mode(), Mode::Conf);
    fs::write(home.join(".config/hypr/hyprland.lua"), "").unwrap();
    let cm = make();
    assert!(cm.config_path().ends_with(".config/hypr/hyprland.lua"));
    assert!(cm.managed_path().ends_with(".config/hypr/hyprland-mod.lua"));
    assert_eq!(cm.mode(), Mode::Lua);
    let x = t.tmp.path().join("x.lua");
    assert_eq!(detect_config(Some(&x.to_string_lossy()), None).1, Mode::Lua);
    assert_eq!(detect_config(Some(&t.config.to_string_lossy()), None).1, Mode::Conf);
}

// ------------------------------------------------------------ get precedence

#[test]
fn user_config_values() {
    let t = TempConfig::conf();
    let cm = t.make();
    assert_eq!(cm.get("decoration:blur:size").as_deref(), Some("3"));
    assert_eq!(cm.get("dwindle:pseudotile").as_deref(), Some("true")); // via source include
    assert_eq!(cm.get("general:gaps_in").as_deref(), Some("5")); // via $variable
    assert_eq!(cm.get("decoration:dim_inactive"), None);
    assert!(!cm.is_modified("decoration:blur:size"));
}

#[test]
fn pending_beats_managed_beats_user() {
    let t = TempConfig::conf();
    fs::write(&t.managed, "decoration {\n  blur {\n    size = 6\n  }\n}\n").unwrap();
    let mut cm = t.make();
    assert_eq!(cm.get("decoration:blur:size").as_deref(), Some("6"));
    assert!(cm.is_modified("decoration:blur:size"));
    cm.set("decoration:blur:size", s("9")).unwrap();
    assert_eq!(cm.get("decoration:blur:size").as_deref(), Some("9"));
    cm.discard();
    assert_eq!(cm.get("decoration:blur:size").as_deref(), Some("6"));
    cm.reset("decoration:blur:size");
    assert_eq!(cm.get("decoration:blur:size").as_deref(), Some("3"));
    assert!(!cm.is_modified("decoration:blur:size"));
}

#[test]
fn managed_not_double_read_through_source() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("general:border_size", s("4")).unwrap();
    cm.apply();
    let cm2 = t.make();
    assert_eq!(cm2.user_value("general:border_size").as_deref(), Some("2"));
    assert_eq!(cm2.get("general:border_size").as_deref(), Some("4"));
}

// ------------------------------------------------------------ staging

#[test]
fn set_pending_discard() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let seen = recorder(&mut cm);
    assert!(!cm.is_dirty());
    cm.set("decoration:rounding", s("12")).unwrap();
    assert!(cm.is_dirty());
    assert_eq!(cm.pending_count(), 1);
    assert_eq!(cm.pending().into_iter().collect::<Vec<_>>(), [(s("decoration:rounding"), s("12"))]);
    assert!(cm.is_modified("decoration:rounding"));
    assert!(!t.managed.exists());
    cm.discard();
    cm.flush_events();
    assert!(!cm.is_dirty());
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10"));
    assert_eq!(*seen.borrow(), [(s("changed"), Some(s("decoration:rounding"))), (s("discarded"), None)]);
}

#[test]
fn invalid_input_rejected() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    for key in ["", "bad key", "a::b", "device:mouse:sensitivity", "source", "x{"] {
        assert!(cm.set(key, s("1")).is_err(), "{key}");
    }
    for value in ["1\n2", "a } b"] {
        assert!(cm.set("general:gaps_in", s(value)).is_err());
    }
    assert!(!cm.is_dirty());
}

#[test]
fn unsubscribe_and_bad_listener() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let seen = recorder(&mut cm);
    let id = cm.subscribe(Box::new(|_: &Event, _: Option<&str>| panic!("bad listener")));
    cm.set("general:gaps_in", s("2")).unwrap();
    cm.flush_events(); // the panicking listener is contained
    cm.unsubscribe(id);
    cm.set("general:gaps_in", s("3")).unwrap();
    cm.flush_events();
    assert_eq!(names(&seen), ["changed", "changed"]);
}

#[test]
fn listeners_can_reborrow_through_refcell() {
    let t = TempConfig::conf();
    let cm = Rc::new(RefCell::new(t.make()));
    let seen = Rc::new(RefCell::new(Vec::new()));
    let (weak, sink) = (Rc::downgrade(&cm), seen.clone());
    cm.borrow_mut().subscribe(Box::new(move |_: &Event, k: Option<&str>| {
        let cm = weak.upgrade().unwrap();
        let value = k.and_then(|k| cm.borrow().get(k)); // would panic if still borrowed
        sink.borrow_mut().push(value);
    }));
    ConfigManager::update(&cm, |cm| cm.set("decoration:rounding", s("7"))).unwrap();
    cm.borrow_mut().set("decoration:rounding", s("8")).unwrap();
    ConfigManager::dispatch_events(&cm);
    assert_eq!(*seen.borrow(), [Some(s("7")), Some(s("8"))]);
}

// ------------------------------------------------------------ apply

#[test]
fn apply_writes_managed_and_source_line() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:blur:enabled", s("false")).unwrap();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("general:col.active_border", s("rgba(ff0000ff) rgba(00ff00ff) 45deg")).unwrap();
    cm.set("misc:vfr", s("true")).unwrap();
    let result = cm.apply();
    assert_eq!(result, ApplyResult { ok: true, errors: vec![], live_applied: false });
    assert!(!cm.is_dirty());

    let text = t.managed_text();
    assert!(text.contains("decoration {\n    rounding = 8\n    blur {\n        enabled = false\n"), "{text}");
    let parsed = parse_config(&t.managed, &[]);
    assert_eq!(parsed.get("decoration:blur:enabled"), Some("false"));
    assert_eq!(parsed.get("general:col.active_border"), Some("rgba(ff0000ff) rgba(00ff00ff) 45deg"));
    assert_eq!(parsed.get("misc:vfr"), Some("true"));

    assert_eq!(t.source_lines().len(), 1);
    let main = t.config_text();
    assert!(main.starts_with(&t.original_text), "user lines must be untouched");
    assert!(main.trim_end().ends_with(&format!("source = {}", t.managed.display())));
    // the whole config (with the source line) now resolves the new value
    assert_eq!(parse_config(&t.config, &[]).get("decoration:rounding"), Some("8"));
}

#[test]
fn source_line_uses_home_relative_path() {
    let t = TempConfig::conf();
    let mut env = t.env.clone();
    env.insert("HOME".into(), t.tmp.path().to_string_lossy().into());
    let mut cm = ConfigManager::new(ManagerOptions { env: Some(env), ..Default::default() }).unwrap();
    cm.set("decoration:rounding", s("8")).unwrap();
    assert!(cm.apply().ok);
    assert!(t.config_text().ends_with("\nsource = ~/hypr/hyprland-mod.conf\n"));
    assert!(t.config_text().contains("\n\n# Hyprland Mod: keep this line last so its settings take precedence\n"));
}

#[test]
fn apply_twice_keeps_one_source_line() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let after_first = t.config_text();
    cm.set("decoration:rounding", s("6")).unwrap();
    cm.apply();
    let mut cm2 = t.make();
    cm2.set("decoration:rounding", s("4")).unwrap();
    cm2.apply();
    assert_eq!(t.config_text(), after_first);
    assert_eq!(t.source_lines().len(), 1);
}

#[test]
fn source_line_moved_to_end_if_not_last() {
    let t = TempConfig::conf();
    fs::write(&t.config, format!("source = {}\n{}", t.managed.display(), t.original_text)).unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let text = t.config_text();
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(t.source_lines().len(), 1);
    assert_eq!(*lines.last().unwrap(), format!("source = {}", t.managed.display()));
    assert!(text.contains(&t.original_text));
}

#[test]
fn existing_relative_source_line_recognized() {
    let t = TempConfig::conf();
    fs::write(&t.config, format!("{}\nsource = ./hyprland-mod.conf # mine\n", t.original_text)).unwrap();
    let before = t.config_text();
    fs::write(&t.managed, "").unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    assert_eq!(t.config_text(), before);
}

#[test]
fn symlinked_config_stays_symlink() {
    let t = TempConfig::conf();
    let real = t.tmp.path().join("dotfiles/hyprland.conf");
    fs::create_dir_all(real.parent().unwrap()).unwrap();
    fs::rename(&t.config, &real).unwrap();
    std::os::unix::fs::symlink(&real, &t.config).unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    assert!(cm.apply().ok);
    assert!(fs::symlink_metadata(&t.config).unwrap().file_type().is_symlink());
    assert!(fs::read_to_string(&real).unwrap().contains("hyprland-mod.conf"));
}

#[test]
fn no_changes_is_noop() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    assert!(cm.apply().ok);
    assert!(!t.managed.exists());
    assert_eq!(t.config_text(), t.original_text);
    assert!(cm.list_backups().is_empty());
}

#[test]
fn missing_config_fails_safely() {
    let t = TempConfig::conf();
    fs::remove_file(&t.config).unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    let result = cm.apply();
    assert!(!result.ok);
    assert!(!t.managed.exists());
    assert!(!t.config.exists());
    assert!(cm.is_dirty());
}

#[test]
fn hash_value_round_trip() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("plugin:test:label", s("a#b")).unwrap();
    cm.apply();
    assert!(t.managed_text().contains("a##b"));
    assert_eq!(t.make().get("plugin:test:label").as_deref(), Some("a#b"));
}

#[test]
fn applied_event() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let seen = recorder(&mut cm);
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.flush_events();
    assert_eq!(names(&seen), ["changed", "applied"]);
}

#[test]
fn new_managed_file_is_private_and_config_mode_kept() {
    use std::os::unix::fs::PermissionsExt;
    let t = TempConfig::conf();
    fs::set_permissions(&t.config, fs::Permissions::from_mode(0o640)).unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    assert_eq!(fs::metadata(&t.managed).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(&t.config).unwrap().permissions().mode() & 0o777, 0o640);
}

// ------------------------------------------------------------ reset

#[test]
fn reset_removes_key() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("general:gaps_in", s("1")).unwrap();
    cm.apply();
    cm.reset("decoration:rounding");
    assert!(cm.is_dirty());
    assert_eq!(cm.pending_resets(), ["decoration:rounding"]);
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10"));
    cm.apply();
    let parsed = parse_config(&t.managed, &[]);
    assert_eq!(parsed.get("decoration:rounding"), None);
    assert_eq!(parsed.get("general:gaps_in"), Some("1"));
    assert!(!cm.is_modified("decoration:rounding"));
}

#[test]
fn empty_value_acts_as_reset() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("group:groupbar:text_color_inactive", s("rgba(ffffffff)")).unwrap();
    cm.apply();
    cm.set("group:groupbar:text_color_inactive", s("")).unwrap();
    assert_eq!(cm.pending_resets(), ["group:groupbar:text_color_inactive"]);
    assert!(cm.pending().is_empty());
    cm.apply();
    assert!(!cm.is_modified("group:groupbar:text_color_inactive"));
    assert_eq!(cm.get("group:groupbar:text_color_inactive"), None);
}

#[test]
fn reset_of_pending_only() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.reset("decoration:rounding");
    assert!(!cm.is_dirty());
}

#[test]
fn setting_managed_value_again_is_not_dirty() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.set("decoration:rounding", s("8")).unwrap();
    assert!(!cm.is_dirty());
}

#[test]
fn empty_state_removes_source_line_and_apply_readds() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    assert_eq!(t.source_lines().len(), 1);
    cm.reset("decoration:rounding");
    cm.apply();
    assert!(t.source_lines().is_empty());
    assert_eq!(t.config_text(), t.original_text);
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    assert_eq!(t.source_lines().len(), 1);
}

// ------------------------------------------------------------ lists

#[test]
fn lists_fall_back_to_user_config() {
    let t = TempConfig::conf();
    let cm = t.make();
    assert_eq!(cm.get_list("bezier"), ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"]);
    assert_eq!(cm.get_list("animations:animation"), cm.get_list("animation"));
    assert!(!cm.is_list_modified("bezier"));
}

#[test]
fn set_list_wins_wholesale() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set_list("bezier", strings(&["snappy, 0.05, 0.9, 0.1, 1.05"])).unwrap();
    cm.set_list("animation", strings(&["windows, 1, 3, snappy", "fade, 0"])).unwrap();
    assert_eq!(cm.get_list("bezier"), ["snappy, 0.05, 0.9, 0.1, 1.05"]);
    cm.apply();
    let text = t.managed_text();
    assert!(text.find("bezier =").unwrap() < text.find("animation =").unwrap());
    let mut cm2 = t.make();
    assert_eq!(cm2.get_list("animation"), ["windows, 1, 3, snappy", "fade, 0"]);
    assert!(cm2.is_list_modified("animation"));
    cm2.reset_list("animation");
    assert_eq!(cm2.get_list("animation").len(), 2);
    assert_eq!(cm2.get_list("animation")[0], "windows, 1, 4.79, easeOutQuint");
    cm2.apply();
    assert!(!t.managed_text().contains("animation ="));
    assert!(t.managed_text().contains("bezier = snappy"));
}

#[test]
fn empty_list_is_kept_as_override() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set_list("bezier", vec![]).unwrap();
    assert!(cm.get_list("bezier").is_empty());
}

// ------------------------------------------------------------ backups

#[test]
fn first_apply_backs_up_original() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let backups = cm.list_backups();
    assert_eq!(backups.len(), 1);
    let b = &backups[0];
    assert!(b.label.contains("Original"));
    assert_eq!(b.files, std::slice::from_ref(&t.config));
    assert_eq!(b.created.len(), 19);
    let copy_dir = t.data.join("backups").join(&b.id).join("files");
    let copy = fs::read_dir(&copy_dir).unwrap().next().unwrap().unwrap().path();
    assert_eq!(fs::read_to_string(copy).unwrap(), t.original_text);
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(t.data.join("backups").join(&b.id).join("manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["files"][0]["copy"], json!("000-hyprland.conf"));
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(t.data.join("state.json")).unwrap()).unwrap();
    assert_eq!(state["initial_backup"], json!(b.id));
    cm.set("decoration:rounding", s("6")).unwrap();
    cm.apply();
    assert_eq!(cm.list_backups().len(), 1, "source line already present, no new backup");
}

#[test]
fn initial_backup_strips_preexisting_source_line() {
    let t = TempConfig::conf();
    fs::write(
        &t.config,
        format!(
            "{}\n# Hyprland Mod: keep this line last so its settings take precedence\nsource = {}\n",
            t.original_text,
            t.managed.display()
        ),
    )
    .unwrap();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let original = cm.list_backups().pop().unwrap();
    let copy = t.data.join("backups").join(&original.id).join("files/000-hyprland.conf");
    assert_eq!(fs::read_to_string(copy).unwrap(), t.original_text);
}

#[test]
fn create_and_restore() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let bid = cm.create_backup("my snapshot").unwrap();
    assert_eq!(cm.list_backups()[0].id, bid);
    assert_eq!(cm.list_backups()[0].label, "my snapshot");
    cm.set("decoration:rounding", s("2")).unwrap();
    cm.apply();
    cm.take_events();
    let seen = recorder(&mut cm);
    let result = cm.restore_backup(&bid);
    cm.flush_events();
    assert!(result.ok);
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("8"));
    assert_eq!(names(&seen), ["restored"]);
    assert!(cm.list_backups().iter().any(|b| b.label.starts_with("Before restoring")));
}

#[test]
fn restore_original_removes_source_line() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let original = cm.list_backups().pop().unwrap().id;
    assert!(cm.restore_backup(&original).ok);
    assert_eq!(t.config_text(), t.original_text);
    // the managed file did not exist then: it now holds an empty conf state
    assert!(t.managed_text().starts_with("# Hyprland Mod managed settings."));
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10"));
}

#[test]
fn restore_unknown_or_malicious_id() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    for bad in ["nope", "../../etc", ""] {
        assert!(!cm.restore_backup(bad).ok);
    }
}

#[test]
fn corrupt_manifest_ignored() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let bid = cm.create_backup("x").unwrap();
    fs::write(t.data.join("backups").join(&bid).join("manifest.json"), "{broken").unwrap();
    assert!(cm.list_backups().is_empty());
}

#[test]
fn many_backups_unique_ids() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let ids: std::collections::BTreeSet<String> = (0..5).map(|i| cm.create_backup(&format!("b{i}")).unwrap()).collect();
    assert_eq!(ids.len(), 5);
}

// ------------------------------------------------------------ undo / redo

#[test]
fn undo_redo_cycle() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    assert!(!cm.can_undo());
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.set("decoration:rounding", s("4")).unwrap();
    cm.apply();
    assert!(cm.can_undo());
    assert!(!cm.can_redo());
    cm.take_events();

    let seen = recorder(&mut cm);
    assert!(cm.undo().ok);
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("8"));
    assert_eq!(parse_config(&t.managed, &[]).get("decoration:rounding"), Some("8"));
    assert!(cm.can_redo());
    cm.undo();
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10")); // back to the user's value
    assert!(!cm.can_undo());
    assert!(!cm.undo().ok);
    cm.redo();
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("8"));
    cm.redo();
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("4"));
    assert!(!cm.can_redo());
    cm.flush_events();
    assert_eq!(names(&seen), ["undo", "undo", "redo", "redo"]);
}

#[test]
fn new_apply_clears_redo() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.undo();
    cm.set("decoration:rounding", s("1")).unwrap();
    cm.apply();
    assert!(!cm.can_redo());
}

#[test]
fn history_persists() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    let mut cm2 = t.make();
    assert!(cm2.can_undo());
    cm2.undo();
    assert_eq!(cm2.get("decoration:rounding").as_deref(), Some("10"));
}

#[test]
fn history_is_capped() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    for i in 0..60 {
        cm.set("decoration:rounding", format!("{i}")).unwrap();
        cm.apply();
    }
    let data: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(t.data.join("history.json")).unwrap()).unwrap();
    assert_eq!(data["states"].as_array().unwrap().len(), 50);
    assert_eq!(data["index"], json!(49));
}

#[test]
fn external_edit_resets_history() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    fs::write(&t.managed, "decoration {\n rounding = 3\n}\n").unwrap();
    let cm2 = t.make();
    assert!(!cm2.can_undo());
    assert_eq!(cm2.get("decoration:rounding").as_deref(), Some("3"));
}

#[test]
fn undo_discards_pending() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.set("general:gaps_in", s("0")).unwrap();
    cm.undo();
    assert!(!cm.is_dirty());
}

// ------------------------------------------------------------ export / import

#[test]
fn export_import_round_trip() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("decoration:blur:enabled", s("false")).unwrap();
    cm.set_list("bezier", strings(&["b1, 0, 0, 1, 1"])).unwrap();
    cm.apply();
    cm.set("general:gaps_in", s("2")).unwrap(); // pending is part of the export
    let state = cm.export_state();
    assert_eq!(state["settings"]["general:gaps_in"], json!("2"));
    assert_eq!(state["lists"], json!({"bezier": ["b1, 0, 0, 1, 1"]}));

    cm.discard();
    cm.set("misc:vfr", s("false")).unwrap();
    cm.apply();
    cm.import_state(state.clone()).unwrap();
    assert!(cm.is_dirty());
    assert_eq!(cm.get("general:gaps_in").as_deref(), Some("2"));
    assert!(!cm.is_modified("misc:vfr"));
    cm.apply();
    assert_eq!(t.make().export_state(), state);
}

#[test]
fn import_rejects_bad_data() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    assert!(cm.import_state(json!({"settings": {"bad key": "1"}})).is_err());
    assert!(cm.import_state(json!("nope")).is_err());
    assert!(cm.import_state(json!({"settings": [1]})).is_err());
    assert!(!cm.is_dirty());
}

#[test]
fn import_into_empty_replaces_lists() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set_list("animation", strings(&["fade, 0"])).unwrap();
    cm.apply();
    cm.import_state(json!({"version": 1, "settings": {"decoration:rounding": "0"}, "lists": {}})).unwrap();
    cm.apply();
    assert_eq!(cm.export_state(), json!({"version": 1, "settings": {"decoration:rounding": "0"}, "lists": {}}));
}

// ------------------------------------------------------------ live apply

#[test]
fn dry_run_never_calls_runner() {
    let t = TempConfig::conf();
    let runner = FakeRunner::ok();
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("decoration:rounding", s("8")).unwrap();
    let result = cm.apply();
    assert!(!result.live_applied);
    assert!(runner.calls().is_empty());
}

#[test]
fn batch_string_when_live() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::replying(|_| Ok((0, "ok\nok\n".into(), String::new())));
    let mut cm = t.make_with(Box::new(runner.clone()));
    assert!(cm.hyprland_running());
    assert!(!cm.dry_run());
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("decoration:blur:enabled", s("false")).unwrap();
    let result = cm.apply();
    assert_eq!(
        runner.calls(),
        [args(&["--batch", "keyword decoration:blur:enabled false ; keyword decoration:rounding 8"])]
    );
    assert!(result.ok);
    assert!(result.live_applied);
}

#[test]
fn lists_live_bezier_first() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::ok();
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set_list(
        "animation",
        strings(&["windows, 1, 4.79, easeOutQuint", "fade, 1, 3.03, linear", "border, 1, 2, snap"]),
    )
    .unwrap();
    cm.set_list("bezier", strings(&["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1", "snap, 0, 1, 0, 1"]))
        .unwrap();
    cm.apply();
    assert_eq!(
        runner.calls(),
        [args(&["--batch", "keyword bezier snap, 0, 1, 0, 1 ; keyword animation border, 1, 2, snap"])]
    );
}

#[test]
fn semicolon_values_reload() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::ok();
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("plugin:x:cmd", s("a ; b")).unwrap();
    cm.apply();
    assert_eq!(runner.calls(), [args(&["reload"])]);
}

#[test]
fn errors_collected() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::replying(|_| {
        Ok((0, "ok\nconfig option <decoration:nope> does not exist.\n".into(), String::new()))
    });
    let mut cm = t.make_with(Box::new(runner));
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("decoration:nope", s("1")).unwrap();
    let result = cm.apply();
    assert!(!result.ok);
    assert!(!result.live_applied);
    assert_eq!(result.errors, ["config option <decoration:nope> does not exist."]);
    assert!(t.managed.exists(), "file is still written");
}

#[test]
fn runner_error_does_not_break_save() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::replying(|_| Err(anyhow::anyhow!("socket gone")));
    let mut cm = t.make_with(Box::new(runner));
    cm.set("decoration:rounding", s("8")).unwrap();
    let result = cm.apply();
    assert!(!result.ok);
    assert!(result.errors[0].contains("socket gone"));
    assert_eq!(parse_config(&t.managed, &[]).get("decoration:rounding"), Some("8"));
}

#[test]
fn reset_live_uses_user_value_or_reload() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::ok();
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("decoration:dim_inactive", s("true")).unwrap();
    cm.apply();
    runner.clear();
    cm.reset("decoration:rounding");
    cm.apply();
    assert_eq!(runner.calls(), [args(&["--batch", "keyword decoration:rounding 10"])]);
    runner.clear();
    cm.reset("decoration:dim_inactive"); // no user value: needs a reload
    cm.apply();
    assert_eq!(runner.calls(), [args(&["reload"])]);
    runner.clear();
    cm.undo();
    assert_eq!(runner.calls(), [args(&["--batch", "keyword decoration:dim_inactive true"])]);
}

#[test]
fn restore_reloads_when_live() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::ok();
    let mut cm = t.make_with(Box::new(runner.clone()));
    let bid = cm.create_backup("x").unwrap();
    runner.clear();
    cm.restore_backup(&bid);
    assert_eq!(runner.calls(), [args(&["reload"])]);
}

#[test]
fn snapshot_and_animations_conf() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    cm.set("general:gaps_out", s("40")).unwrap();
    cm.apply();
    let snap = cm.snapshot_current_look(&["general:gaps_out", "decoration:rounding", "decoration:dim_inactive"]);
    assert_eq!(snap["general:gaps_out"].as_deref(), Some("10")); // the user's own, not ours
    assert_eq!(snap["decoration:rounding"].as_deref(), Some("10"));
    assert_eq!(snap["decoration:dim_inactive"], None);
    let (beziers, animations) = cm.current_animations();
    assert_eq!(beziers.len(), 2);
    assert_eq!(animations[0], "windows, 1, 4.79, easeOutQuint");
}

#[test]
fn live_snapshot_uses_descriptions_and_animations() {
    let mut t = TempConfig::conf();
    t.live();
    let runner = FakeRunner::replying(|a| {
        Ok(match a.join(" ").as_str() {
            "-j descriptions" => (0, json!([{"name": "general:gaps_out", "current": "20 20 20 20"}]).to_string(), String::new()),
            "-j animations" => (0, json!([
                [{"name": "windows", "overridden": true, "enabled": true, "speed": 4.0, "bezier": "snap", "style": "popin"},
                 {"name": "fade", "overridden": true, "enabled": false},
                 {"name": "border", "overridden": false}],
                [{"name": "default", "X0": 0, "Y0": 0, "X1": 1, "Y1": 1},
                 {"name": "snap", "X0": 0.1, "Y0": 1, "X1": 0.2, "Y1": 1.0},
                 {"name": "__internal", "X0": 0, "Y0": 0, "X1": 1, "Y1": 1}]]).to_string(), String::new()),
            _ => (0, "ok".into(), String::new()),
        })
    });
    let cm = t.make_with(Box::new(runner.clone()));
    let snap = cm.snapshot_current_look(&["general:gaps_out", "general:gaps_in"]);
    assert_eq!(snap["general:gaps_out"].as_deref(), Some("20 20 20 20"));
    assert_eq!(snap["general:gaps_in"].as_deref(), Some("5"));
    let (b, a) = cm.current_animations();
    assert_eq!(b, ["snap, 0.1, 1, 0.2, 1"]);
    assert_eq!(a, ["windows, 1, 4, snap, popin", "fade, 0"]);
}
