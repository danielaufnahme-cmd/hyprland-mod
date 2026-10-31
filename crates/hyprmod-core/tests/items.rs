//! Binds, window rules, monitors and autostart: rendering and preservation (the
//! editors for them come with the second half of the port).

mod common;

use common::{assert_luac_ok, lua_bin, run_fake_hl, s, TempConfig};
use hyprmod_core::items::{
    bind_conf, bind_lua, clean_bind, clean_monitor, clean_rule, monitor_conf, monitor_lua, rule_conf, rule_lua,
};
use hyprmod_core::lua::read_lua_state;
use hyprmod_core::managed::read_managed;
use serde_json::{json, Value as Json};

fn sample_bind() -> Json {
    json!({"mods": ["SUPER", "SHIFT"], "key": "T", "action": "launch_app", "arg": "kitty", "description": "Terminal"})
}

fn sample_rule() -> Json {
    json!({"name": "Float calculator", "match": {"class": "^(org.gnome.Calculator)$", "floating": false},
           "effects": {"float": true, "size": "800 600", "center": true, "opacity": "0.9 0.8", "rounding": "4"}})
}

fn with(base: Json, over: Json) -> Json {
    let mut b = base;
    for (k, v) in over.as_object().unwrap() {
        b[k] = v.clone();
    }
    b
}

#[test]
fn bind_normalized() {
    let b = clean_bind(&json!({"mods": "shift super", "key": "t", "action": "workspace", "arg": " 3 "}), false, None)
        .unwrap();
    assert_eq!(
        (b["mods"].clone(), b["key"].clone(), b["arg"].clone(), b["flags"].clone()),
        (json!(["SUPER", "SHIFT"]), json!("t"), json!("3"), json!(""))
    );
    assert!(b["id"].as_str().unwrap().starts_with("bind-"));
    let m =
        clean_bind(&json!({"mods": ["SUPER"], "key": "mouse:272", "action": "drag_move_window"}), false, None).unwrap();
    assert_eq!(m["flags"], json!("m"));
}

#[test]
fn bind_rejects() {
    for bad in [
        json!({"mods": ["HYPER"], "key": "A", "action": "close_window"}),
        json!({"mods": [], "key": "A, B", "action": "close_window"}),
        json!({"mods": [], "key": "", "action": "close_window"}),
        json!({"mods": [], "key": "A", "action": null}),
        json!({"mods": [], "key": "A", "action": "close_window", "flags": "z"}),
    ] {
        assert!(clean_bind(&bad, false, None).is_err(), "{bad}");
    }
    assert!(clean_bind(&with(sample_bind(), json!({"description": "a, b"})), true, None).is_err());
}

#[test]
fn bind_renderings() {
    let b = clean_bind(&with(sample_bind(), json!({"flags": "le"})), true, None).unwrap();
    assert_eq!(bind_conf(&b).unwrap(), (s("bindled"), s("SUPER SHIFT, T, Terminal, exec, kitty")));
    assert_eq!(
        bind_lua(&b).unwrap(),
        "hl.bind(\"SUPER + SHIFT + T\", hl.dsp.exec_cmd(\"kitty\"), { description = \"Terminal\", locked = true, repeating = true })"
    );
    let c = clean_bind(&json!({"mods": [], "key": "XF86AudioMute", "action": "close_window"}), true, None).unwrap();
    assert_eq!(bind_conf(&c).unwrap(), (s("bind"), s(", XF86AudioMute, killactive,")));
}

#[test]
fn rule_renderings() {
    let r = clean_rule(&sample_rule(), true, Some("rule-1")).unwrap();
    assert_eq!(rule_conf(&r), "float on, size 800 600, center on, opacity 0.9 0.8, rounding 4, match:class ^(org.gnome.Calculator)$, match:float false");
    assert_eq!(
        rule_lua(&r),
        "hl.window_rule({ name = \"hyprland-mod-rule-1\", match = { class = \"^(org.gnome.Calculator)$\", float = false }, \
         float = true, size = \"800 600\", center = true, opacity = \"0.9 0.8\", rounding = 4 })"
    );
    for bad in [
        json!({"match": {}, "effects": {"float": true}}),
        json!({"match": {"class": "x"}, "effects": {}}),
        json!({"match": {"Class!": "x"}, "effects": {"float": true}}),
        json!({"match": {"class": "a\nb"}, "effects": {"float": true}}),
    ] {
        assert!(clean_rule(&bad, false, None).is_err(), "{bad}");
    }
    assert!(clean_rule(&json!({"match": {"class": "a,b"}, "effects": {"float": true}}), true, None).is_err());
}

#[test]
fn monitor_renderings() {
    let m = clean_monitor(&json!({"mode": "2560x1440@239.97Hz", "position": "0x0", "scale": "1.25"})).unwrap();
    assert_eq!(
        Json::Object(m.clone()),
        json!({"mode": "2560x1440@239.97", "position": "0x0", "scale": "1.25", "transform": 0, "enabled": true})
    );
    assert_eq!(monitor_conf("DP-1", &m), "DP-1, 2560x1440@239.97, 0x0, 1.25");
    let mut m1 = m.clone();
    m1.insert("transform".into(), json!(1));
    assert_eq!(monitor_lua("DP-1", &m1),
               "hl.monitor({ output = \"DP-1\", mode = \"2560x1440@239.97\", position = \"0x0\", scale = 1.25, transform = 1 })");
    m1.insert("transform".into(), json!(3));
    assert_eq!(monitor_conf("DP-1", &m1), "DP-1, 2560x1440@239.97, 0x0, 1.25, transform, 3");
    let off = clean_monitor(&json!({"enabled": false})).unwrap();
    assert_eq!(monitor_conf("HDMI-A-1", &off), "HDMI-A-1, disable");
    assert_eq!(monitor_lua("HDMI-A-1", &off), "hl.monitor({ output = \"HDMI-A-1\", disabled = true })");
    for bad in [json!({"mode": "huge"}), json!({"position": "left"}), json!({"scale": "0"}), json!({"transform": 9})] {
        assert!(clean_monitor(&bad).is_err(), "{bad}");
    }
}

fn items_state() -> Json {
    json!({
        "binds": [{"id": "bind-1", "mods": ["SUPER", "SHIFT"], "key": "T", "action": "launch_app", "arg": "kitty",
                   "description": "Terminal", "flags": ""}],
        "window_rules": [{"id": "rule-1", "name": "Calc", "match": {"class": "^(calc)$"}, "effects": {"float": true}, "enabled": true}],
        "monitors": {"DP-1": {"mode": "1920x1080@60", "position": "0x0", "scale": "1", "transform": 0, "enabled": true}},
        "autostart": {"bar": "waybar"},
    })
}

#[test]
fn items_survive_settings_changes_conf() {
    let t = TempConfig::conf();
    let mut cm = t.make();
    let mut state = cm.export_state();
    for (k, v) in items_state().as_object().unwrap() {
        state[k] = v.clone();
    }
    cm.import_state(state).unwrap();
    assert!(cm.apply().ok);
    let text = t.managed_text();
    assert!(text.contains("bindd = SUPER SHIFT, T, Terminal, exec, kitty\n"), "{text}");
    assert!(text.contains("exec-once = waybar\n"));
    // a later settings change keeps every item exactly as it was
    let mut cm2 = t.make();
    cm2.set("decoration:rounding", s("3")).unwrap();
    assert!(cm2.apply().ok);
    let back = read_managed(&t.managed);
    assert_eq!(back.binds.len(), 1);
    assert_eq!(back.binds[0]["description"], json!("Terminal"));
    assert_eq!(back.autostart["bar"], "waybar");
    assert!(t.managed_text().ends_with(text.split("\n# ---- Hyprland Mod").nth(1).unwrap()));
    // an import without item fields keeps them
    cm2.import_state(json!({"settings": {}, "lists": {}})).unwrap();
    cm2.apply();
    assert_eq!(read_managed(&t.managed).window_rules.len(), 1);
    assert_eq!(t.source_lines().len(), 1, "items alone keep the source line");
}

#[test]
fn items_survive_and_execute_lua() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    let mut state = cm.export_state();
    for (k, v) in items_state().as_object().unwrap() {
        state[k] = v.clone();
    }
    cm.import_state(state).unwrap();
    assert!(cm.apply().ok);
    let mut cm2 = t.make();
    cm2.set("general:gaps_in", s("3")).unwrap();
    assert!(cm2.apply().ok);
    let back = read_lua_state(&t.managed);
    assert_eq!(back.monitors.len(), 1);
    assert_luac_ok(&t.managed);
    if lua_bin().is_some() {
        let full = run_fake_hl(&t.config);
        assert!(full.iter().any(|l| l == "cfg general.gaps_in=3"), "{full:?}");
        assert!(
            full.iter().any(|l| l == "bind SUPER + SHIFT + T -> exec_cmd(kitty) {description=Terminal}"),
            "{full:?}"
        );
    }
}

#[test]
fn unknown_state_keys_round_trip() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    cm.import_state(json!({"settings": {"general:gaps_in": "3"}, "lists": {}, "future_field": {"x": [1, 2]}})).unwrap();
    // import stages known fields only; an unknown key written by a newer version is kept on disk
    cm.apply();
    let text = t.managed_text().replace("\"lists\": {}", "\"future_field\": {\"x\": [1, 2]}, \"lists\": {}");
    std::fs::write(&t.managed, text).unwrap();
    let mut cm2 = t.make();
    cm2.set("general:gaps_in", s("4")).unwrap();
    assert!(cm2.apply().ok);
    assert_eq!(read_lua_state(&t.managed).other["future_field"], json!({"x": [1, 2]}));
}
