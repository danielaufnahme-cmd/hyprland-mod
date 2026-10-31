//! Cross-compatibility with the Python version: files it wrote (see
//! ``tests/fixtures/python/gen_compat.py``) are read correctly and written back byte for byte.

mod common;

use std::fs;
use std::path::PathBuf;

use common::{assert_luac_ok, fixtures, rust_fixtures, s, TempConfig};
use hyprmod_core::items::{lua_calls, render_conf_extras};
use hyprmod_core::lua::{load_lua_types, read_lua_state, render_lua};
use hyprmod_core::managed::{read_managed, render};
use hyprmod_core::{ConfigManager, PresetStore};
use serde_json::{json, Value as Json};

fn py(path: &str) -> PathBuf {
    rust_fixtures().join("python").join(path)
}

fn read(path: PathBuf) -> String {
    fs::read_to_string(path).unwrap()
}

fn export(mode: &str) -> Json {
    serde_json::from_str(&read(py(&format!("{mode}/export.json")))).unwrap()
}

#[test]
fn python_lua_file_reads_and_rerenders_identically() {
    let path = py("lua/hyprland-mod.lua");
    let state = read_lua_state(&path);
    assert_eq!(state.settings["plugin:my-plugin:label"], "a \"quoted\" #value é ü");
    assert_eq!(state.lists["animation"].len(), 3);
    assert_eq!(state.binds.len(), 3);
    assert_eq!(state.binds[0]["description"], json!("Terminal ✓"));
    assert_eq!(state.window_rules.len(), 2);
    assert_eq!(state.monitors.len(), 2);
    assert_eq!(state.autostart["bar"], "waybar --style 'a b'");
    assert_eq!(state.to_dict(), export("lua"));
    let types = load_lua_types(Some(&fixtures().join("hl_stubs.lua")));
    let text = render_lua(&state, &types, &lua_calls(&state).unwrap()).unwrap();
    assert_eq!(text, read(path));
}

#[test]
fn python_conf_file_reads_and_rerenders_identically() {
    let path = py("conf/hyprland-mod.conf");
    let state = read_managed(&path);
    assert_eq!(state.settings["plugin:my-plugin:label"], "a \"quoted\" #value é ü");
    assert_eq!(state.binds.len(), 3);
    assert_eq!(state.to_dict(), export("conf"));
    let text = render(&state) + &render_conf_extras(&state).unwrap();
    assert_eq!(text, read(path));
}

/// Rust applying the state Python exported writes the same managed file, config and history.
fn same_files_as_python(mut t: TempConfig, mode: &str) {
    let ext = if mode == "lua" { "lua" } else { "conf" };
    t.env.insert("HYPRLAND_MOD_LUA_STUBS".into(), fixtures().join("hl_stubs.lua").to_string_lossy().into());
    let mut cm = t.make();
    cm.import_state(export(mode)).unwrap();
    let result = cm.apply();
    assert!(result.ok, "{:?}", result.errors);
    assert_eq!(t.managed_text(), read(py(&format!("{mode}/hyprland-mod.{ext}"))));
    assert_eq!(read(t.data.join("history.json")), read(py(&format!("{mode}/history.json"))));
    let ours = t.config_text();
    let theirs = read(py(&format!("{mode}/hyprland.{ext}")));
    if mode == "lua" {
        assert_eq!(ours, theirs);
        assert_luac_ok(&t.managed);
    } else {
        // the source line holds the absolute temp path the file was written to
        let (a, b) = (ours.rsplit_once("source = ").unwrap(), theirs.rsplit_once("source = ").unwrap());
        assert_eq!(a.0, b.0);
        assert_eq!(a.1, format!("{}\n", t.managed.display()));
    }
}

#[test]
fn rust_writes_same_lua_files_as_python() {
    same_files_as_python(TempConfig::lua(), "lua");
}

#[test]
fn rust_writes_same_conf_files_as_python() {
    same_files_as_python(TempConfig::conf(), "conf");
}

/// Open a setup Python left behind, change one setting, keep everything else.
fn continue_python_setup(t: TempConfig, mode: &str) {
    let ext = if mode == "lua" { "lua" } else { "conf" };
    let theirs = read(py(&format!("{mode}/hyprland.{ext}")));
    let line = theirs.lines().last().unwrap();
    let config =
        if mode == "lua" { theirs.clone() } else { theirs.replace(line, &format!("source = {}", t.managed.display())) };
    fs::write(&t.config, &config).unwrap();
    fs::copy(py(&format!("{mode}/hyprland-mod.{ext}")), &t.managed).unwrap();
    fs::create_dir_all(&t.data).unwrap();
    fs::copy(py(&format!("{mode}/history.json")), t.data.join("history.json")).unwrap();
    fs::write(t.data.join("state.json"), "{\n \"initial_backup\": \"20260101-000000-000000\"\n}").unwrap();

    let mut cm: ConfigManager = t.make();
    assert_eq!(cm.get("general:layout").as_deref(), Some("master"));
    assert_eq!(cm.get("plugin:my-plugin:label").as_deref(), Some("a \"quoted\" #value é ü"));
    assert_eq!(cm.get_list("bezier"), ["snappy, 0.05, 0.9, 0.1, 1.05"]);
    assert!(cm.can_undo(), "Python's history is picked up");
    assert_eq!(cm.export_state(), export(mode));
    cm.set("decoration:rounding", s("12")).unwrap();
    assert!(cm.apply().ok);
    assert_eq!(t.config_text(), config, "loader line already in place");
    let mut expected = export(mode);
    expected["settings"]["decoration:rounding"] = json!("12");
    assert_eq!(cm.export_state(), expected);
    // undo goes back to exactly what Python wrote
    assert!(cm.undo().ok);
    assert_eq!(t.managed_text(), read(py(&format!("{mode}/hyprland-mod.{ext}"))));
}

#[test]
fn continue_python_lua_setup() {
    continue_python_setup(TempConfig::lua(), "lua");
}

#[test]
fn continue_python_conf_setup() {
    continue_python_setup(TempConfig::conf(), "conf");
}

#[test]
fn python_preset_file_round_trips() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = PresetStore::new(&common::app_data(), cm.data_dir());
    let src = py("cafe.hyprmod-preset");
    let p = st.read_preset_file(&src).unwrap();
    assert_eq!(p.name, "Café ✓ Look");
    assert_eq!(p.id, "caf-look"); // "café" is not a valid id: slug of the name, like Python
    assert_eq!(p.settings["decoration:active_opacity"], "0.9");
    assert_eq!(p.tags, ["dark", "warm"]);
    let out = t.tmp.path().join("again.hyprmod-preset");
    st.export_preset(&p, &out).unwrap();
    assert_eq!(read(out), read(src));
}
