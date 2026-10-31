mod common;

use std::path::Path;

use common::{args, assert_luac_ok, fixtures, lua_bin, run_fake_hl, s, strings, FakeRunner, TempConfig};
use hyprmod_core::lua::{
    self, animation_call, curve_call, describe_value, dotted, list_calls, load_lua_types, lua_key, lua_literal,
    lua_path, read_lua_state, require_line, required_files, scan_lists, with_require_line, LuaTypes,
};
use hyprmod_core::managed::ManagedState;
use hyprmod_core::{parse_value, ConfigManager, Mode, OptionSpec, OptionType, Value};
use serde_json::json;

fn types() -> LuaTypes {
    load_lua_types(Some(&fixtures().join("hl_stubs.lua")))
}

fn lit(key: &str, value: &str) -> String {
    let t = types();
    lua_literal(key, value, t.get(&dotted(key)).map(String::as_str))
}

#[test]
fn scalars() {
    assert_eq!(lit("decoration:blur:enabled", "yes"), "true");
    assert_eq!(lit("decoration:blur:enabled", "0"), "false");
    assert_eq!(lit("misc:unknown_flag", "off"), "false");
    assert_eq!(lit("decoration:rounding", "8"), "8");
    assert_eq!(lit("decoration:blur:vibrancy", ".5"), "0.5");
    assert_eq!(lit("general:layout", "master"), "\"master\"");
    assert_eq!(lit("input:kb_layout", "us,de"), "\"us,de\"");
    assert_eq!(lit("input:kb_layout", "1"), "\"1\"");
    assert_eq!(lit("misc:x", "say \"hi\" \\ now"), "\"say \\\"hi\\\" \\\\ now\"");
}

#[test]
fn colors_and_gradients() {
    assert_eq!(lit("decoration:shadow:color", "0xee1a1a1a"), "\"rgba(1a1a1aee)\"");
    assert_eq!(
        lit("general:col.active_border", "rgba(33ccffee) rgba(00ff99ee) 45deg"),
        "{ colors = { \"rgba(33ccffee)\", \"rgba(00ff99ee)\" }, angle = 45 }"
    );
    assert_eq!(lit("general:col.inactive_border", "rgb(595959)"), "\"rgba(595959ff)\"");
    assert_eq!(lit("group:groupbar:text_color", "rgba(ffffffff)"), "\"rgba(ffffffff)\"");
}

#[test]
fn vec2_and_gaps() {
    assert_eq!(lit("decoration:shadow:offset", "2 -3"), "{ 2, -3 }");
    assert_eq!(lit("general:gaps_out", "5 10"), "{ top = 5, right = 10, bottom = 5, left = 10 }");
    assert_eq!(lit("general:gaps_out", "12"), "12");
    assert_eq!(lit("general:gaps_out", "1 2 3"), "{ top = 1, right = 2, bottom = 3, left = 2 }");
}

#[test]
fn without_stubs() {
    assert_eq!(lua_literal("x:y", "true", None), "true");
    assert_eq!(lua_literal("x:y", "1.25", None), "1.25");
    assert_eq!(lua_literal("x:y", "hello", None), "\"hello\"");
    assert!(load_lua_types(Some(&fixtures().join("nope.lua"))).is_empty());
}

#[test]
fn keys() {
    assert_eq!(lua_path("general:col.active_border"), ["general", "col", "active_border"]);
    assert_eq!(lua_key("end"), "[\"end\"]");
    assert_eq!(lua_key("my-plugin"), "[\"my-plugin\"]");
    assert_eq!(lua::lua_string("a\nb\x01\x7f"), "\"a\\nb\\001\\127\"");
}

#[test]
fn curve() {
    assert_eq!(
        curve_call("snappy, 0.05, 0.9, 0.1, 1.05").unwrap(),
        "hl.curve(\"snappy\", { type = \"bezier\", points = { {0.05, 0.9}, {0.1, 1.05} } })"
    );
    for bad in ["x, 1, 2", "x, a, b, c, d", ", 1, 2, 3, 4"] {
        assert!(curve_call(bad).is_err(), "{bad}");
    }
}

#[test]
fn animation() {
    assert_eq!(
        animation_call("windows, 1, 4.79, easeOutQuint, popin 87%").unwrap(),
        "hl.animation({ leaf = \"windows\", enabled = true, speed = 4.79, bezier = \"easeOutQuint\", style = \"popin 87%\" })"
    );
    assert_eq!(animation_call("fade, 0").unwrap(), "hl.animation({ leaf = \"fade\", enabled = false })");
    assert!(animation_call("windows, 1, 4, spring:easy").unwrap().contains("spring = \"easy\""));
    for bad in ["windows", "windows, maybe, 1", "windows, 1, fast"] {
        assert!(animation_call(bad).is_err(), "{bad}");
    }
    assert!(list_calls("monitor", &strings(&["DP-1, preferred, auto, 1"])).is_err());
}

#[test]
fn scan_user_lists() {
    let files = required_files(&fixtures().join("sample_hyprland.lua"), &[]);
    let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["sample_hyprland.lua", "colors.lua"]);
    let lists = scan_lists(&files);
    assert_eq!(lists["bezier"], ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1", "quick, 0.15, 0, 0.1, 1"]);
    assert_eq!(
        lists["animation"],
        [
            "global, 1, 10, default",
            "windows, 1, 4.79, spring:easy",
            "windowsIn, 1, 4.1, easeOutQuint, popin 87%",
            "fade, 0"
        ]
    );
}

#[test]
fn describe() {
    assert_eq!(describe_value("a.b", &json!(true)).as_deref(), Some("true"));
    assert_eq!(describe_value("decoration.shadow.color", &json!(0xee1a1a1au32)).as_deref(), Some("rgba(1a1a1aee)"));
    assert_eq!(
        describe_value(
            "general.col.active_border",
            &json!({"colors": ["rgba(33ccffee)", "rgba(00ff99ee)"], "angle": 45})
        )
        .as_deref(),
        Some("rgba(33ccffee) rgba(00ff99ee) 45deg")
    );
    assert_eq!(describe_value("x.offset", &json!({"x": 1, "y": 2.5})).as_deref(), Some("1 2.5"));
    assert_eq!(describe_value("x.f", &json!(0.30000000000000004)).as_deref(), Some("0.3"));
    assert_eq!(describe_value("x", &json!(null)), None);
    assert_eq!(describe_value("x", &json!("595959aa 0deg")).as_deref(), Some("rgba(595959aa) 0deg"));
    assert_eq!(describe_value("x", &json!("dwindle")).as_deref(), Some("dwindle"));
}

const CFG: &str = "/cfg/hypr/hyprland.lua";
const MOD: &str = "/cfg/hypr/hyprland-mod.lua";

fn with_req(text: &str) -> Option<String> {
    with_require_line(text, Path::new(CFG), Path::new(MOD))
}

#[test]
fn require_added_once_and_idempotent() {
    let text = "hl.config({ general = { gaps_in = 1 } })";
    let once = with_req(text).unwrap();
    assert!(once.starts_with(&format!("{text}\n")));
    assert!(once.ends_with("\npcall(require, \"hyprland-mod\")\n"));
    assert_eq!(with_req(&once), None);
}

#[test]
fn require_existing_variants_recognized() {
    for line in ["require(\"hyprland-mod\")", "require 'hyprland-mod'", "pcall(require, \"hyprland-mod\");"] {
        let text = format!("x = 1\n{line}  -- mine\n-- trailing comment\n");
        assert_eq!(with_req(&text), None, "{line}");
    }
}

#[test]
fn require_moved_to_end_and_deduplicated() {
    let text = "pcall(require, \"hyprland-mod\")\nx = 1\nrequire(\"hyprland-mod\")\ny = 2\n";
    let out = with_req(text).unwrap();
    assert_eq!(out.matches("hyprland-mod\"").count(), 1);
    assert!(out.contains("x = 1\ny = 2\n"));
}

#[test]
fn commented_require_does_not_count() {
    let text = "-- pcall(require, \"hyprland-mod\")\n--[[\nrequire(\"hyprland-mod\")\n]]\nx = 1\n";
    let out = with_req(text).unwrap();
    assert!(out.starts_with(text));
    assert!(out.ends_with("pcall(require, \"hyprland-mod\")\n"));
}

#[test]
fn managed_outside_config_dir_uses_dofile() {
    assert_eq!(require_line(Path::new(CFG), Path::new("/elsewhere/mod.lua")), "pcall(dofile, \"/elsewhere/mod.lua\")");
}

// ------------------------------------------------------------ manager in Lua mode

#[test]
fn mode_and_paths() {
    let t = TempConfig::lua();
    let cm = t.make();
    assert_eq!(cm.mode(), Mode::Lua);
    assert_eq!(cm.config_path(), t.config);
    assert_eq!(cm.managed_path(), t.managed);
}

#[test]
fn conf_env_with_lua_sibling_switches_to_lua() {
    let mut t = TempConfig::lua();
    t.env.insert("HYPRLAND_MOD_CONFIG".into(), t.hypr.join("hyprland.conf").to_string_lossy().into());
    let cm = t.make();
    assert_eq!((cm.mode(), cm.config_path()), (Mode::Lua, t.config.as_path()));
}

#[test]
fn dry_run_values() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    assert_eq!(cm.get("decoration:rounding"), None); // no live Hyprland: default
    assert_eq!(cm.get_list("bezier")[0], "easeOutQuint, 0.23, 1, 0.32, 1");
    cm.set("decoration:rounding", s("8")).unwrap();
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("8"));
}

fn apply_sample(cm: &mut ConfigManager) -> hyprmod_core::ApplyResult {
    for (k, v) in [
        ("decoration:rounding", "8"),
        ("decoration:blur:enabled", "false"),
        ("decoration:blur:vibrancy", "0.2"),
        ("decoration:shadow:color", "0xee1a1a1a"),
        ("general:col.active_border", "rgba(ff0000ff) rgba(00ff00ff) 90deg"),
        ("general:layout", "master"),
        ("misc:vfr", "true"),
        ("plugin:my-plugin:label", "a \"quoted\" #value"),
    ] {
        cm.set(k, s(v)).unwrap();
    }
    cm.set_list("bezier", strings(&["snappy, 0.05, 0.9, 0.1, 1.05"])).unwrap();
    cm.set_list("animation", strings(&["windows, 1, 3, snappy, popin 80%", "fade, 0"])).unwrap();
    cm.apply()
}

#[test]
fn apply_writes_lua_and_require_line() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    let result = apply_sample(&mut cm);
    assert!(result.ok, "{:?}", result.errors);
    let text = t.managed_text();
    assert!(text.contains("hl.config({\n    decoration = {\n"));
    assert!(text.contains("active_border = { colors = { \"rgba(ff0000ff)\", \"rgba(00ff00ff)\" }, angle = 90 },"));
    assert!(text.contains("hl.curve(\"snappy\""));
    assert!(text.find("hl.curve").unwrap() < text.find("hl.animation").unwrap());
    assert_eq!(t.require_lines().len(), 1);
    assert!(t.config_text().starts_with(&t.original_text));
    assert!(t.config_text().ends_with("pcall(require, \"hyprland-mod\")\n"));
    assert!(!t.hypr.join("hyprland-mod.conf").exists());

    let cm2 = t.make(); // state reads back from the generated file
    assert_eq!(cm2.get("general:layout").as_deref(), Some("master"));
    assert_eq!(cm2.get_list("animation"), ["windows, 1, 3, snappy, popin 80%", "fade, 0"]);
    assert!(!cm2.can_redo());
}

#[test]
fn apply_twice_one_require_line() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    apply_sample(&mut cm);
    let first = t.config_text();
    cm.set("decoration:rounding", s("2")).unwrap();
    cm.apply();
    t.make().apply();
    assert_eq!(t.config_text(), first);
    assert_eq!(t.require_lines().len(), 1);
}

#[test]
fn generated_lua_compiles() {
    let t = TempConfig::lua();
    apply_sample(&mut t.make());
    assert_luac_ok(&t.managed);
    assert_luac_ok(&t.config);
}

#[test]
fn generated_lua_executes_against_fake_hl() {
    if lua_bin().is_none() {
        eprintln!("lua not installed: skipped");
        return;
    }
    let t = TempConfig::lua();
    apply_sample(&mut t.make());
    let out = run_fake_hl(&t.managed); // no pcall here: any error fails the test
    for expected in [
        "cfg decoration.rounding=8",
        "cfg decoration.blur.enabled=false",
        "cfg decoration.blur.vibrancy=0.2",
        "cfg decoration.shadow.color=rgba(1a1a1aee)",
        "cfg general.col.active_border.angle=90",
        "cfg general.col.active_border.colors.1=rgba(ff0000ff)",
        "cfg general.layout=master",
        "cfg misc.vfr=true",
        "cfg plugin.my-plugin.label=a \"quoted\" #value",
        "curve snappy:bezier:0.05,0.9,0.1,1.05",
        "anim windows|true|3|snappy|popin 80%",
        "anim fade|false|nil|nil|nil",
    ] {
        assert!(out.iter().any(|l| l == expected), "missing {expected}: {out:?}");
    }
    // through the user's config: managed values win because they load last
    let full = run_fake_hl(&t.config);
    assert!(full.iter().any(|l| l == "cfg decoration.rounding=8"));
    assert!(full.iter().any(|l| l == "cfg general.gaps_in=5"));
    assert_eq!(full[full.len() - 2..], ["anim windows|true|3|snappy|popin 80%", "anim fade|false|nil|nil|nil"]);
}

#[test]
fn invalid_lua_lists_rejected() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    assert!(cm.set_list("bezier", strings(&["broken"])).is_err());
    assert!(cm.set_list("monitor", strings(&["DP-1, preferred, auto, 1"])).is_err());
    assert!(cm.import_state(json!({"settings": {}, "lists": {"animation": ["x"]}})).is_err());
    assert!(!cm.is_dirty());
}

#[test]
fn undo_reset_backup_in_lua_mode() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    cm.set("decoration:rounding", s("4")).unwrap();
    cm.apply();
    cm.undo();
    let settings = read_lua_state(&t.managed).settings;
    assert_eq!(settings.into_iter().collect::<Vec<_>>(), [(s("decoration:rounding"), s("8"))]);
    cm.reset("decoration:rounding");
    cm.apply();
    assert_eq!(read_lua_state(&t.managed), ManagedState::default());
    let original = cm.list_backups().pop().unwrap();
    assert_eq!(original.files, std::slice::from_ref(&t.config));
    cm.restore_backup(&original.id);
    assert_eq!(t.config_text(), t.original_text);
}

#[test]
fn restore_writes_lua_not_conf_header() {
    // the managed file did not exist when the original backup was taken
    let t = TempConfig::lua();
    let mut cm = t.make();
    apply_sample(&mut cm);
    let original = cm.list_backups().pop().unwrap();
    assert!(cm.restore_backup(&original.id).ok);
    assert_eq!(t.config_text(), t.original_text);
    let text = t.managed_text();
    assert!(text.starts_with("-- Hyprland Mod managed settings."), "{text}");
    assert_luac_ok(&t.managed);
    assert_eq!(read_lua_state(&t.managed), ManagedState::default());
    // a fresh manager reads the restored state as Lua and applies cleanly again
    let mut cm2 = t.make();
    cm2.set("general:gaps_in", s("3")).unwrap();
    assert!(cm2.apply().ok);
    assert_eq!(t.require_lines().len(), 1);
}

#[test]
fn initial_backup_is_taken_before_require_line() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    cm.set("general:gaps_in", s("3")).unwrap();
    cm.apply();
    let original = cm.list_backups().pop().unwrap();
    assert!(original.label.contains("Original"));
    let copy =
        std::fs::read_to_string(t.data.join("backups").join(&original.id).join("files/000-hyprland.lua")).unwrap();
    assert_eq!(copy, t.original_text);
    assert!(t.config_text().contains("hyprland-mod"));
}

#[test]
fn empty_state_removes_require_line() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    cm.set("general:gaps_in", s("3")).unwrap();
    cm.apply();
    cm.reset("general:gaps_in");
    cm.apply();
    assert_eq!(t.config_text(), t.original_text);
    assert!(t.require_lines().is_empty());
    assert_luac_ok(&t.managed);
}

#[test]
fn export_import_round_trip_lua() {
    let t = TempConfig::lua();
    let mut cm = t.make();
    apply_sample(&mut cm);
    let state = cm.export_state();
    cm.import_state(json!({"version": 1, "settings": {}, "lists": {}})).unwrap();
    cm.apply();
    cm.import_state(state.clone()).unwrap();
    cm.apply();
    assert_eq!(t.make().export_state(), state);
}

// ------------------------------------------------------------ live Lua

fn descriptions_runner(descriptions: serde_json::Value) -> FakeRunner {
    let text = descriptions.to_string();
    FakeRunner::replying(move |a| {
        if a == args(&["-j", "descriptions"]).as_slice() {
            Ok((0, text.clone(), String::new()))
        } else {
            Ok((0, "ok".into(), String::new()))
        }
    })
}

fn default_descriptions() -> serde_json::Value {
    json!([
        {"value": "decoration:rounding", "data": {"default": 0, "current": 10}},
        {"value": "general.col.active_border",
         "data": {"current": {"colors": ["rgba(33ccffee)", "rgba(00ff99ee)"], "angle": 45}}},
        {"value": "decoration:blur:enabled", "data": {"current": true}},
        {"value": "decoration:shadow:color", "data": {"current": 0xee1a1a1au32}},
    ])
}

#[test]
fn reads_current_values_once() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(default_descriptions());
    let cm = t.make_with(Box::new(runner.clone()));
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10"));
    assert_eq!(cm.get("general:col.active_border").as_deref(), Some("rgba(33ccffee) rgba(00ff99ee) 45deg"));
    assert_eq!(cm.get("decoration:blur:enabled").as_deref(), Some("true"));
    assert_eq!(cm.get("decoration:shadow:color").as_deref(), Some("rgba(1a1a1aee)"));
    assert_eq!(cm.get("misc:vfr"), None);
    assert_eq!(runner.calls(), [args(&["-j", "descriptions"])]);
}

#[test]
fn real_descriptions_shape() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(json!([
        {"name": "general:col.active_border", "description": "", "default": "ffffffff 0deg",
         "current": "33ccffee 00ff99ee 45deg"},
        {"name": "general:col.inactive_border", "current": "595959aa 0deg"},
        {"name": "group:groupbar:text_color", "current": "ffffffff 0deg"},
        {"name": "general:gaps_out", "current": "20 20 20 20"},
        {"name": "general:layout", "current": "dwindle"},
        {"name": "decoration:rounding", "current": 10, "min": 0, "max": 20},
    ]));
    let cm = t.make_with(Box::new(runner.clone()));
    assert_eq!(cm.get("general:col.active_border").as_deref(), Some("rgba(33ccffee) rgba(00ff99ee) 45deg"));
    assert_eq!(cm.get("general:col.inactive_border").as_deref(), Some("rgba(595959aa) 0deg"));
    assert_eq!(cm.get("general:gaps_out").as_deref(), Some("20 20 20 20"));
    assert_eq!(cm.get("general:layout").as_deref(), Some("dwindle"));
    assert_eq!(cm.get("decoration:rounding").as_deref(), Some("10"));
    let mut color = OptionSpec::new("x", OptionType::Color);
    color.default = json!("");
    assert_eq!(parse_value(&color, cm.get("group:groupbar:text_color").as_deref()), Value::Text(s("rgba(ffffffff)")));
    let mut int = OptionSpec::new("x", OptionType::Int);
    int.default = json!(0);
    assert_eq!(parse_value(&int, cm.get("general:gaps_out").as_deref()), Value::Int(20));
    assert_eq!(runner.calls(), [args(&["-j", "descriptions"])]);
}

#[test]
fn eval_string() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(default_descriptions());
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.set("general:col.active_border", s("rgba(ff0000ff) rgba(00ff00ff) 90deg")).unwrap();
    let mut beziers = cm.get_list("bezier");
    beziers.push(s("snap, 0, 1, 0, 1"));
    cm.set_list("bezier", beziers).unwrap();
    let result = cm.apply();
    assert!(result.ok, "{:?}", result.errors);
    assert!(result.live_applied);
    assert_eq!(
        runner.calls(),
        [args(&[
            "eval",
            "hl.config({ decoration = { rounding = 8 }, general = { col = { active_border = \
             { colors = { \"rgba(ff0000ff)\", \"rgba(00ff00ff)\" }, angle = 90 } } } }); \
             hl.curve(\"snap\", { type = \"bezier\", points = { {0, 1}, {0, 1} } })"
        ])]
    );
}

#[test]
fn eval_string_compiles() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(default_descriptions());
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("misc:vfr", s("true")).unwrap();
    cm.set("plugin:x:label", s("it's \"fine\"")).unwrap();
    let mut anims = cm.get_list("animation");
    anims.push(s("border, 1, 2, default"));
    cm.set_list("animation", anims).unwrap();
    cm.apply();
    let code = runner.calls().last().unwrap()[1].clone();
    let path = t.tmp.path().join("eval.lua");
    std::fs::write(&path, code).unwrap();
    assert_luac_ok(&path);
}

#[test]
fn removal_reloads() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(default_descriptions());
    let mut cm = t.make_with(Box::new(runner.clone()));
    cm.set("decoration:rounding", s("8")).unwrap();
    cm.apply();
    runner.clear();
    cm.reset("decoration:rounding");
    cm.apply();
    assert_eq!(runner.calls(), [args(&["reload"])]);
}

#[test]
fn eval_errors_reported() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = FakeRunner::replying(|_| Ok((1, String::new(), "lua error: bad".into())));
    let mut cm = t.make_with(Box::new(runner));
    cm.set("decoration:rounding", s("8")).unwrap();
    let result = cm.apply();
    assert!(!result.ok);
    assert_eq!(result.errors, ["lua error: bad"]);
    assert!(t.managed.exists());
}

#[test]
fn snapshot_in_lua_mode() {
    let mut t = TempConfig::lua();
    t.live();
    let runner = descriptions_runner(default_descriptions());
    let mut cm = t.make_with(Box::new(runner));
    cm.set("decoration:blur:enabled", s("false")).unwrap();
    cm.apply();
    let snap = cm.snapshot_current_look(&["decoration:rounding", "decoration:blur:enabled"]);
    assert_eq!(snap["decoration:rounding"].as_deref(), Some("10"));
    assert_eq!(snap["decoration:blur:enabled"], None); // managed: the user's own value is unknowable
}
