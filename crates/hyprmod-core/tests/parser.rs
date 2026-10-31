mod common;

use common::fixtures;
use hyprmod_core::parser::{parse_config, parse_text, resolve_vars, strip_comment, ParsedConfig};
use indexmap::IndexMap;

fn sample() -> ParsedConfig {
    parse_config(&fixtures().join("sample_hyprland.conf"), &[])
}

#[test]
fn comments_and_escapes() {
    assert_eq!(strip_comment("a = b # c"), "a = b");
    assert_eq!(strip_comment("# only"), "");
    assert_eq!(strip_comment("a = ##b # c"), "a = #b");
    assert_eq!(strip_comment("   key   =   v   "), "key   =   v");
}

#[test]
fn nested_sections() {
    let cfg = sample();
    assert_eq!(cfg.get("decoration:blur:enabled"), Some("true"));
    assert_eq!(cfg.get("decoration:blur:vibrancy"), Some("0.1696"));
    assert_eq!(cfg.get("decoration:shadow:range"), Some("4"));
    assert_eq!(cfg.get("input:touchpad:natural_scroll"), Some("false"));
}

#[test]
fn variables_resolved() {
    let cfg = sample();
    assert_eq!(cfg.get("general:gaps_in"), Some("5"));
    assert_eq!(cfg.get("general:col.active_border"), Some("rgba(33ccffee) rgba(00ff99ee) 45deg"));
    assert_eq!(cfg.get_all("bind")[0], "SUPER, Q, exec, kitty");
    assert_eq!(cfg.variables["mainMod"], "SUPER");
}

#[test]
fn source_include_followed() {
    let cfg = sample();
    assert_eq!(cfg.get("dwindle:pseudotile"), Some("true"));
    assert_eq!(cfg.get("cursor:no_hardware_cursors"), Some("true"));
    assert_eq!(cfg.files.len(), 2);
    // extra.conf sets gaps_out=12 but the main config sets 10 later: later wins
    assert_eq!(cfg.get("general:gaps_out"), Some("10"));
}

#[test]
fn inline_category_and_comments() {
    let cfg = sample();
    assert_eq!(cfg.get("misc:force_default_wallpaper"), Some("0"));
    assert_eq!(cfg.get("misc:disable_hyprland_logo"), Some("true"));
    assert_eq!(cfg.get("plugin:test:label"), Some("#notacomment"));
}

#[test]
fn device_block_skipped() {
    let cfg = sample();
    assert_eq!(cfg.get("input:sensitivity"), Some("0"));
    assert_eq!(cfg.get("name"), None);
    assert!(!cfg.entries.iter().any(|e| e.key.starts_with("device")));
}

#[test]
fn repeated_keywords() {
    let cfg = sample();
    assert_eq!(cfg.get_all("bezier"), ["easeOutQuint, 0.23, 1, 0.32, 1", "linear, 0, 0, 1, 1"]);
    assert_eq!(cfg.get_all("animation").len(), 2);
    assert_eq!(cfg.get_all("monitor").len(), 2);
    assert_eq!(cfg.get_all("exec-once"), ["waybar", "kitty --class scratch"]);
}

#[test]
fn unknown_lines_kept() {
    let cfg = sample();
    assert_eq!(cfg.get("some_future_keyword"), Some("kept-as-is"));
    assert!(cfg.warnings.is_empty(), "{:?}", cfg.warnings);
    assert_eq!(cfg.values()["general:gaps_out"], "10");
}

#[test]
fn one_line_and_close_on_value_line() {
    let cfg = parse_text("general { border_size = 3 }\nmisc {\n  vfr = true }\nx = 1\n", "<string>");
    assert_eq!(cfg.get("general:border_size"), Some("3"));
    assert_eq!(cfg.get("misc:vfr"), Some("true"));
    assert_eq!(cfg.get("x"), Some("1"));
}

#[test]
fn unbalanced_braces_warn_not_crash() {
    let cfg = parse_text("}\ngeneral {\n gaps_in = 1\n", "<string>");
    assert_eq!(cfg.get("general:gaps_in"), Some("1"));
    assert!(!cfg.warnings.is_empty());
}

#[test]
fn garbage_line_warns() {
    let cfg = parse_text("this is not valid\nok = 1\n", "<string>");
    assert_eq!(cfg.get("ok"), Some("1"));
    assert_eq!(cfg.warnings.len(), 1);
}

#[test]
fn nested_device_blocks_skipped() {
    let cfg = parse_text("device {\n name = a\n inner {\n x = 1\n }\n}\ny = 2\n", "<string>");
    assert_eq!(cfg.entries.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(), ["y"]);
}

#[test]
fn longest_variable_wins() {
    let vars: IndexMap<String, String> =
        [("a", "1"), ("ab", "2")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    assert_eq!(resolve_vars("$ab-$a", &vars), "2-1");
    assert_eq!(resolve_vars("$missing", &vars), "$missing");
}

#[test]
fn recursive_source_is_safe() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.conf"), "source = ./b.conf\nx = 1\n").unwrap();
    std::fs::write(d.path().join("b.conf"), "source = ./a.conf\ny = 2\n").unwrap();
    let cfg = parse_config(&d.path().join("a.conf"), &[]);
    assert_eq!((cfg.get("x"), cfg.get("y")), (Some("1"), Some("2")));
    assert!(cfg.warnings.iter().any(|w| w.contains("recursive")));
}

#[test]
fn missing_source_warns() {
    let d = tempfile::tempdir().unwrap();
    std::fs::write(d.path().join("a.conf"), "source = ./nope.conf\nx = 1\n").unwrap();
    let cfg = parse_config(&d.path().join("a.conf"), &[]);
    assert_eq!(cfg.get("x"), Some("1"));
    assert_eq!(cfg.warnings.len(), 1);
}

#[test]
fn glob_sources_sorted_and_excluded() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir(d.path().join("conf.d")).unwrap();
    std::fs::write(d.path().join("conf.d/b.conf"), "x = b\n").unwrap();
    std::fs::write(d.path().join("conf.d/a.conf"), "x = a\ny = a\n").unwrap();
    std::fs::write(d.path().join("main.conf"), "source = conf.d/*.conf\n").unwrap();
    let cfg = parse_config(&d.path().join("main.conf"), &[]);
    assert_eq!((cfg.get("x"), cfg.get("y")), (Some("b"), Some("a")));
    let cfg = parse_config(&d.path().join("main.conf"), &[d.path().join("conf.d/b.conf")]);
    assert_eq!(cfg.get("x"), Some("a"));
}

#[test]
fn animations_block_aliases() {
    let cfg = parse_text("animations {\n bezier = b, 0, 0, 1, 1\n}\nbezier = c, 1, 1, 1, 1\n", "<string>");
    assert_eq!(cfg.get_all("bezier"), ["b, 0, 0, 1, 1", "c, 1, 1, 1, 1"]);
}
