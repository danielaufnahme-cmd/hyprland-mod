use hyprmod_core::values::{color_to_rgba_tuple, format_float, normalize_gradient};
use hyprmod_core::{format_value, parse_color, parse_value, OptionSpec, OptionType, Value};
use serde_json::{json, Value as Json};

fn opt(kind: OptionType, default: Json) -> OptionSpec {
    let mut o = OptionSpec::new("x", kind);
    o.default = default;
    o
}

fn bool_opt() -> OptionSpec {
    opt(OptionType::Bool, json!(false))
}
fn int_opt() -> OptionSpec {
    opt(OptionType::Int, json!(2))
}
fn float_opt() -> OptionSpec {
    opt(OptionType::Float, json!(0.5))
}
fn color_opt() -> OptionSpec {
    opt(OptionType::Color, json!("0xff444444"))
}
fn gradient_opt() -> OptionSpec {
    opt(OptionType::Gradient, json!("rgba(ffffffff)"))
}
fn string_opt() -> OptionSpec {
    opt(OptionType::String, json!("dwindle"))
}
fn vec2_opt() -> OptionSpec {
    opt(OptionType::Vec2, json!("0 0"))
}
fn enum_opt() -> OptionSpec {
    OptionSpec::from_json(&json!({"key": "x", "type": "enum", "default": "dwindle",
        "choices": [{"value": "dwindle", "label": "Dwindle"}, {"value": "master", "label": "Master"}]}))
    .unwrap()
}
fn int_enum_opt() -> OptionSpec {
    OptionSpec::from_json(&json!({"key": "x", "type": "enum", "default": 0,
        "choices": [{"value": 0, "label": "Off"}, {"value": 1, "label": "On"}, {"value": 2, "label": "Loose"}]}))
    .unwrap()
}

fn t(s: &str) -> Value {
    Value::Text(s.into())
}

fn list(items: &[f64]) -> Value {
    Value::List(items.iter().map(|&f| Value::Float(f)).collect())
}

#[test]
fn bool_words() {
    for text in ["true", "yes", "on", "1", " TRUE "] {
        assert_eq!(parse_value(&bool_opt(), Some(text)), Value::Bool(true), "{text}");
    }
    for text in ["false", "no", "off", "0"] {
        assert_eq!(parse_value(&bool_opt(), Some(text)), Value::Bool(false), "{text}");
    }
}

#[test]
fn bool_format_and_fallback() {
    assert_eq!(format_value(&bool_opt(), &Value::Bool(true)).unwrap(), "true");
    assert_eq!(format_value(&bool_opt(), &Value::Bool(false)).unwrap(), "false");
    assert_eq!(format_value(&bool_opt(), &t("yes")).unwrap(), "true");
    assert!(format_value(&bool_opt(), &t("maybe")).is_err());
    assert_eq!(parse_value(&bool_opt(), Some("maybe")), Value::Bool(false));
    assert_eq!(parse_value(&bool_opt(), None), Value::Bool(false));
    assert_eq!(parse_value(&opt(OptionType::Bool, json!("true")), None), Value::Bool(true));
}

#[test]
fn int() {
    assert_eq!(parse_value(&int_opt(), Some("7")), Value::Int(7));
    assert_eq!(parse_value(&int_opt(), Some("0x10")), Value::Int(16));
    assert_eq!(parse_value(&int_opt(), Some("3.0")), Value::Int(3));
    assert_eq!(parse_value(&int_opt(), Some("abc")), Value::Int(2));
    assert_eq!(parse_value(&int_opt(), Some("20 20 20 20")), Value::Int(20)); // CSS gaps
    assert_eq!(format_value(&int_opt(), &Value::Float(4.6)).unwrap(), "5");
    assert_eq!(format_value(&int_opt(), &Value::Float(2.5)).unwrap(), "2"); // Python round()
}

#[test]
fn float() {
    assert_eq!(format_value(&float_opt(), &Value::Float(1.0)).unwrap(), "1");
    assert_eq!(format_value(&float_opt(), &Value::Float(0.1696)).unwrap(), "0.1696");
    assert_eq!(format_value(&float_opt(), &Value::Float(0.30000000000000004)).unwrap(), "0.3");
    let mut two = OptionSpec::new("x", OptionType::Float);
    two.digits = Some(2);
    assert_eq!(format_value(&two, &Value::Float(0.456)).unwrap(), "0.46");
    assert_eq!(format_value(&float_opt(), &Value::Float(-0.0)).unwrap(), "0");
    assert_eq!(parse_value(&float_opt(), Some("0.95")), Value::Float(0.95));
    assert_eq!(parse_value(&float_opt(), Some("bad")), Value::Float(0.5));
    assert_eq!(format_float(10.0, None), "10");
}

#[test]
fn color_forms() {
    let c = color_opt();
    assert_eq!(parse_value(&c, Some("rgba(33ccffee)")), t("rgba(33ccffee)"));
    assert_eq!(parse_value(&c, Some("rgba(33CCFFEE)")), t("rgba(33ccffee)"));
    assert_eq!(parse_value(&c, Some("rgb(33ccff)")), t("rgba(33ccffff)"));
    assert_eq!(parse_value(&c, Some("0xee33ccff")), t("rgba(33ccffee)"));
    assert_eq!(parse_value(&c, Some("rgba(255, 0, 128, 0.5)")), t("rgba(ff008080)"));
    assert_eq!(parse_value(&c, Some("rgb(255, 0, 128)")), t("rgba(ff0080ff)"));
    assert_eq!(parse_value(&c, Some("#112233")), t("rgba(112233ff)"));
}

#[test]
fn color_default_and_invalid() {
    assert_eq!(parse_value(&color_opt(), None), t("rgba(444444ff)"));
    assert_eq!(parse_value(&color_opt(), Some("notacolor")), t("rgba(444444ff)"));
    assert_eq!(parse_color("rgba(12345)"), None);
    assert_eq!(parse_color("rgba(1, 2)"), None);
    assert!(format_value(&color_opt(), &t("nope")).is_err());
}

#[test]
fn bare_hex_live_shapes() {
    let auto = opt(OptionType::Color, json!(""));
    assert_eq!(parse_value(&auto, Some("ffffffff 0deg")), t("rgba(ffffffff)"));
    assert_eq!(parse_value(&opt(OptionType::Gradient, Json::Null), Some("595959aa 0deg")), t("rgba(595959aa) 0deg"));
    assert_eq!(normalize_gradient("33ccffee 00ff99ee 45deg"), "rgba(33ccffee) rgba(00ff99ee) 45deg");
}

#[test]
fn empty_color_means_automatic() {
    let auto = opt(OptionType::Color, json!(""));
    assert_eq!(parse_value(&auto, None), t(""));
    assert_eq!(parse_value(&auto, Some("")), t(""));
    assert_eq!(format_value(&auto, &t("")).unwrap(), "");
    assert_eq!(format_value(&color_opt(), &Value::Null).unwrap(), "");
    assert_eq!(format_value(&gradient_opt(), &t("")).unwrap(), "");
    assert_eq!(parse_value(&opt(OptionType::Gradient, json!("")), None), t(""));
}

#[test]
fn color_format() {
    let c = color_opt();
    assert_eq!(format_value(&c, &t("0xff112233")).unwrap(), "rgba(112233ff)");
    let rgb = Value::List(vec![Value::Int(255), Value::Int(0), Value::Int(0)]);
    assert_eq!(format_value(&c, &rgb).unwrap(), "rgba(ff0000ff)");
    let rgba = Value::List(vec![Value::Int(0), Value::Int(255), Value::Int(0), Value::Float(0.5)]);
    assert_eq!(format_value(&c, &rgba).unwrap(), "rgba(00ff0080)");
    assert_eq!(color_to_rgba_tuple("rgba(01020304)"), Some((1, 2, 3, 4)));
}

#[test]
fn gradient_and_string() {
    let g = "rgba(33ccffee)   rgba(00ff99ee) 45deg";
    assert_eq!(parse_value(&gradient_opt(), Some(g)), t("rgba(33ccffee) rgba(00ff99ee) 45deg"));
    assert_eq!(format_value(&gradient_opt(), &t(g)).unwrap(), "rgba(33ccffee) rgba(00ff99ee) 45deg");
    assert_eq!(parse_value(&string_opt(), Some(" master ")), t("master"));
    assert_eq!(parse_value(&string_opt(), None), t("dwindle"));
}

#[test]
fn vec2() {
    let v = vec2_opt();
    assert_eq!(parse_value(&v, Some("10 20")), Value::List(vec![Value::Int(10), Value::Int(20)]));
    assert_eq!(parse_value(&v, Some("1.5, -2")), list(&[1.5, -2.0]));
    assert_eq!(parse_value(&v, None), list(&[0.0, 0.0]));
    assert_eq!(parse_value(&v, Some("1 2 3")), list(&[0.0, 0.0]));
    assert_eq!(format_value(&v, &list(&[10.0, 20.5])).unwrap(), "10 20.5");
    assert_eq!(format_value(&v, &t("3,4")).unwrap(), "3 4");
}

#[test]
fn enums() {
    assert_eq!(parse_value(&enum_opt(), Some("Master")), t("master"));
    assert_eq!(parse_value(&enum_opt(), None), t("dwindle"));
    assert_eq!(parse_value(&int_enum_opt(), Some("2")), Value::Int(2));
    assert_eq!(format_value(&int_enum_opt(), &Value::Int(1)).unwrap(), "1");
    assert_eq!(parse_value(&enum_opt(), Some("custom")), t("custom"));
    let bools = OptionSpec::from_json(&json!({"key": "x", "type": "enum", "default": false,
        "choices": [{"value": false, "label": "No"}, {"value": true, "label": "Yes"}, {"value": "auto", "label": "Auto"}]}))
    .unwrap();
    assert_eq!(parse_value(&bools, Some("yes")), Value::Bool(true));
    assert_eq!(format_value(&bools, &Value::Bool(true)).unwrap(), "true");
}

#[test]
fn missing_optional_fields() {
    let bare_enum = OptionSpec::new("x", OptionType::Enum);
    assert_eq!(parse_value(&bare_enum, Some("3")), Value::Int(3));
    let bare = OptionSpec::new("x", OptionType::String);
    assert_eq!(parse_value(&bare, None), Value::Null);
    assert_eq!(format_value(&bare, &t("x")).unwrap(), "x");
}

#[test]
fn round_trips() {
    let cases: Vec<(OptionSpec, Value)> = vec![
        (bool_opt(), Value::Bool(true)),
        (bool_opt(), Value::Bool(false)),
        (int_opt(), Value::Int(0)),
        (int_opt(), Value::Int(-12)),
        (float_opt(), Value::Float(0.25)),
        (float_opt(), Value::Float(3.0)),
        (color_opt(), t("rgba(1a2b3c4d)")),
        (gradient_opt(), t("rgba(ffffffff) rgba(000000ff) 90deg")),
        (string_opt(), t("hello world")),
        (vec2_opt(), Value::List(vec![Value::Int(3), Value::Float(4.5)])),
        (enum_opt(), t("master")),
        (int_enum_opt(), Value::Int(2)),
    ];
    for (option, value) in cases {
        let text = format_value(&option, &value).unwrap();
        assert_eq!(parse_value(&option, Some(&text)), value, "{text}");
        assert_eq!(format_value(&option, &parse_value(&option, Some(&text))).unwrap(), text);
    }
}
