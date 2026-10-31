//! Conversion between typed values and Hyprland config strings.

use std::fmt;
use std::sync::LazyLock;

use anyhow::{anyhow, bail, Result};
use regex::Regex;
use serde_json::Value as Json;

use crate::schema::{OptionSpec, OptionType};
use crate::util::{float_repr, parse_float, parse_int, parse_int_base0, repr_str, split_ws, strip};

pub const TRUE_WORDS: &[&str] = &["true", "yes", "on", "1"];
pub const FALSE_WORDS: &[&str] = &["false", "no", "off", "0"];

static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]+$").unwrap());
static DEG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+(?:\.\d+)?deg$").unwrap());
static FUNC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^(rgba?)\(\s*(.*?)\s*\)$").unwrap());

/// A typed option value (the Python version's dynamic value).
///
/// ``vec2`` values and colour tuples are lists of numbers.
#[derive(Clone, Debug)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<Value>),
}

impl PartialEq for Value {
    /// Numbers compare by value (``2 == 2.0``) like in Python.
    fn eq(&self, other: &Self) -> bool {
        use Value::*;
        match (self, other) {
            (Null, Null) => true,
            (Bool(a), Bool(b)) => a == b,
            (Int(a), Int(b)) => a == b,
            (Int(a), Float(b)) | (Float(b), Int(a)) => (*a as f64) == *b,
            (Float(a), Float(b)) => a == b,
            (Text(a), Text(b)) => a == b,
            (List(a), List(b)) => a == b,
            _ => false,
        }
    }
}

impl Value {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Bool(b) => Some(*b as i64 as f64),
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn from_json(v: &Json) -> Value {
        match v {
            Json::Null => Value::Null,
            Json::Bool(b) => Value::Bool(*b),
            Json::Number(n) => match n.as_i64() {
                Some(i) if !n.is_f64() => Value::Int(i),
                _ => Value::Float(n.as_f64().unwrap_or(0.0)),
            },
            Json::String(s) => Value::Text(s.clone()),
            Json::Array(a) => Value::List(a.iter().map(Value::from_json).collect()),
            Json::Object(_) => Value::Text(crate::util::py_str(v)),
        }
    }

    pub fn to_json(&self) -> Json {
        match self {
            Value::Null => Json::Null,
            Value::Bool(b) => Json::Bool(*b),
            Value::Int(i) => Json::from(*i),
            Value::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
            Value::Text(s) => Json::String(s.clone()),
            Value::List(l) => Json::Array(l.iter().map(Value::to_json).collect()),
        }
    }
}

/// Python ``str(value)``.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("None"),
            Value::Bool(b) => f.write_str(if *b { "True" } else { "False" }),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => f.write_str(&float_repr(*x)),
            Value::Text(s) => f.write_str(s),
            Value::List(items) => {
                let parts: Vec<String> = items
                    .iter()
                    .map(|v| match v {
                        Value::Text(s) => repr_str(s),
                        other => other.to_string(),
                    })
                    .collect();
                write!(f, "[{}]", parts.join(", "))
            }
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}
impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}
impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::Text(v.to_string())
    }
}
impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::Text(v)
    }
}

// ---------------------------------------------------------------- colors

/// Python ``round()`` (half to even) clamped to a byte.
fn clamp_byte(v: f64) -> u8 {
    v.round_ties_even().clamp(0.0, 255.0) as u8
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> String {
    format!("rgba({r:02x}{g:02x}{b:02x}{a:02x})")
}

/// Normalize any Hyprland color notation to ``rgba(rrggbbaa)``; None if invalid.
pub fn parse_color(text: &str) -> Option<String> {
    let t = strip(text);
    if let Some(m) = FUNC_RE.captures(t) {
        let func = m[1].to_lowercase();
        let body = &m[2];
        if body.contains(',') {
            let nums: Option<Vec<f64>> = body.split(',').map(parse_float).collect();
            let nums = nums?;
            let (r, g, b, a) = match (func.as_str(), nums.as_slice()) {
                ("rgb", [r, g, b]) => (*r, *g, *b, 1.0),
                ("rgba", [r, g, b, a]) => (*r, *g, *b, *a),
                _ => return None,
            };
            return Some(rgba(clamp_byte(r), clamp_byte(g), clamp_byte(b), clamp_byte(a * 255.0)));
        }
        if HEX_RE.is_match(body) {
            if func == "rgb" && body.len() == 6 {
                return Some(format!("rgba({}ff)", body.to_lowercase()));
            }
            if func == "rgba" && body.len() == 8 {
                return Some(format!("rgba({})", body.to_lowercase()));
            }
        }
        return None;
    }
    if t.len() == 10 && t.to_lowercase().starts_with("0x") && HEX_RE.is_match(&t[2..]) {
        let h = t[2..].to_lowercase();
        return Some(format!("rgba({}{})", &h[2..], &h[..2])); // 0xAARRGGBB
    }
    if t.len() == 8 && HEX_RE.is_match(t) {
        return Some(format!("rgba({})", t.to_lowercase())); // bare rrggbbaa (hyprctl descriptions)
    }
    if let Some(h) = t.strip_prefix('#') {
        if HEX_RE.is_match(h) && (h.len() == 6 || h.len() == 8) {
            let h = h.to_lowercase();
            return Some(format!("rgba({}{})", h, if h.len() == 6 { "ff" } else { "" }));
        }
    }
    None
}

pub(crate) fn is_deg(tok: &str) -> bool {
    DEG_RE.is_match(tok)
}

/// Canonical gradient text: every color token as ``rgba(rrggbbaa)``, angle kept.
pub fn normalize_gradient(text: &str) -> String {
    split_ws(text)
        .into_iter()
        .map(|tok| if is_deg(tok) { tok.to_string() } else { parse_color(tok).unwrap_or_else(|| tok.to_string()) })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A color, or a one-color gradient like ``ffffffff 0deg``.
fn parse_single_color(text: &str) -> Option<String> {
    if let Some(c) = parse_color(text) {
        return Some(c);
    }
    let tokens: Vec<&str> = split_ws(text).into_iter().filter(|t| !is_deg(t)).collect();
    if tokens.len() == 1 {
        parse_color(tokens[0])
    } else {
        None
    }
}

/// ``rgba(rrggbbaa)`` (or any accepted form) -> (r, g, b, a) bytes.
pub fn color_to_rgba_tuple(color: &str) -> Option<(u8, u8, u8, u8)> {
    let norm = parse_color(color)?;
    let h = &norm[5..norm.len() - 1];
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?, byte(6)?))
}

fn format_color(value: &Value) -> Result<String> {
    match value {
        Value::Text(s) => parse_color(s).ok_or_else(|| anyhow!("invalid color: {}", repr_str(s))),
        Value::List(items) if items.len() == 3 || items.len() == 4 => {
            let num = |v: &Value| v.as_f64().ok_or_else(|| anyhow!("invalid color: {value}"));
            let (r, g, b) = (num(&items[0])?, num(&items[1])?, num(&items[2])?);
            let a = match items.get(3) {
                None => 255.0,
                Some(Value::Float(a)) if *a <= 1.0 => a * 255.0,
                Some(v) => num(v)?,
            };
            Ok(rgba(clamp_byte(r), clamp_byte(g), clamp_byte(b), clamp_byte(a)))
        }
        other => bail!("invalid color: {other}"),
    }
}

// ---------------------------------------------------------------- numbers

/// ``f"{value:.{digits}f}"`` without trailing zeros (``-0`` becomes ``0``).
pub fn format_float(value: f64, digits: Option<usize>) -> String {
    let d = digits.unwrap_or(6);
    let mut text = format!("{value:.d$}");
    if text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if text == "-0" || text.is_empty() {
        "0".into()
    } else {
        text
    }
}

fn parse_number(text: &str) -> Option<f64> {
    let mut t = strip(text);
    if t.contains(' ') {
        t = split_ws(t).first().copied().unwrap_or(""); // CSS-style gaps "20 20 20 20"
    }
    parse_int_base0(t).or_else(|| parse_float(t))
}

fn num_out(v: f64) -> Value {
    if v.fract() == 0.0 && v.is_finite() && v.abs() < 9.0e18 {
        Value::Int(v as i64)
    } else {
        Value::Float(v)
    }
}

fn to_float(v: &Value) -> Result<f64> {
    match v {
        Value::Text(s) => parse_float(s).ok_or_else(|| anyhow!("could not convert string to float: {}", repr_str(s))),
        other => other.as_f64().ok_or_else(|| anyhow!("not a number: {other}")),
    }
}

// ---------------------------------------------------------------- public API

/// Typed value -> Hyprland config string for the option's ``type``.
pub fn format_value(option: &OptionSpec, value: &Value) -> Result<String> {
    let kind = &option.kind;
    match kind {
        OptionType::Bool => {
            let b = match value {
                Value::Text(s) => parse_bool(s).ok_or_else(|| anyhow!("invalid bool: {}", repr_str(s)))?,
                other => truthy(other),
            };
            return Ok(if b { "true" } else { "false" }.into());
        }
        OptionType::Int => {
            let f = to_float(value)?;
            if !f.is_finite() {
                bail!("cannot convert {} to integer", float_repr(f));
            }
            return Ok(format!("{}", f.round_ties_even() as i64));
        }
        OptionType::Float => return Ok(format_float(to_float(value)?, option.digits)),
        _ => {}
    }
    if matches!(kind, OptionType::Color | OptionType::Gradient) {
        let empty = match value {
            Value::Null => true,
            other => strip(&other.to_string()).is_empty(),
        };
        if empty {
            return Ok(String::new()); // unset / automatic
        }
    }
    match kind {
        OptionType::Color => format_color(value),
        OptionType::Vec2 => {
            let pair = match value {
                Value::Text(s) => parse_vec2(s).ok_or_else(|| anyhow!("invalid vec2"))?,
                Value::List(items) if items.len() == 2 => items.clone(),
                _ => bail!("invalid vec2"),
            };
            Ok(format!("{} {}", format_float(to_float(&pair[0])?, None), format_float(to_float(&pair[1])?, None)))
        }
        OptionType::Enum => Ok(match value {
            Value::Bool(b) => if *b { "true" } else { "false" }.into(),
            other => other.to_string(),
        }),
        OptionType::Gradient => Ok(normalize_gradient(&value.to_string())),
        _ => Ok(value.to_string()),
    }
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Int(i) => *i != 0,
        Value::Float(f) => *f != 0.0,
        Value::Text(s) => !s.is_empty(),
        Value::List(l) => !l.is_empty(),
    }
}

pub(crate) fn parse_bool(text: &str) -> Option<bool> {
    let t = strip(text).to_lowercase();
    if TRUE_WORDS.contains(&t.as_str()) {
        Some(true)
    } else if FALSE_WORDS.contains(&t.as_str()) {
        Some(false)
    } else {
        None
    }
}

fn parse_vec2(text: &str) -> Option<Vec<Value>> {
    let replaced = text.replace(',', " ");
    let parts = split_ws(&replaced);
    if parts.len() != 2 {
        return None;
    }
    parts.iter().map(|p| parse_float(p).map(num_out)).collect()
}

fn default_value(option: &OptionSpec) -> Value {
    let default = &option.default;
    if let Json::String(s) = default {
        if matches!(
            option.kind,
            OptionType::Bool | OptionType::Int | OptionType::Float | OptionType::Color | OptionType::Vec2
        ) {
            return parse(option, s).unwrap_or_else(|| Value::Text(s.clone()));
        }
    }
    Value::from_json(default)
}

/// Hyprland config string -> typed value; None or unparsable -> option default.
pub fn parse_value(option: &OptionSpec, text: Option<&str>) -> Value {
    match text {
        None => default_value(option),
        Some(t) => parse(option, t).unwrap_or_else(|| default_value(option)),
    }
}

fn parse(option: &OptionSpec, text: &str) -> Option<Value> {
    let t = strip(text);
    match &option.kind {
        OptionType::Bool => parse_bool(t).map(Value::Bool),
        OptionType::Int => {
            let f = parse_number(t)?;
            if !f.is_finite() {
                return None;
            }
            Some(Value::Int(f.trunc() as i64))
        }
        OptionType::Float => parse_number(t).map(Value::Float),
        OptionType::Color => {
            if t.is_empty() {
                Some(Value::Text(String::new()))
            } else {
                parse_single_color(t).map(Value::Text)
            }
        }
        OptionType::Vec2 => parse_vec2(t).map(Value::List),
        OptionType::Enum => {
            for choice in option.choices.iter().flatten() {
                let value = &choice.value;
                let matches_text = crate::util::py_str(value).to_lowercase() == t.to_lowercase();
                let matches_bool = matches!(value, Json::Bool(b) if parse_bool(t) == Some(*b));
                if matches_text || matches_bool {
                    return Some(Value::from_json(value));
                }
            }
            Some(match parse_int(t) {
                Some(i) => Value::Int(i),
                None => Value::Text(t.to_string()),
            })
        }
        OptionType::Gradient => Some(Value::Text(normalize_gradient(t))),
        _ => Some(Value::Text(t.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn opt(kind: &str, default: Json) -> OptionSpec {
        let mut o = OptionSpec::new("x", OptionType::from_name(kind));
        o.default = default;
        o
    }

    #[test]
    fn display_like_python() {
        assert_eq!(Value::List(vec![Value::Int(1), Value::Text("a".into())]).to_string(), "[1, 'a']");
        assert_eq!(Value::Float(3.0).to_string(), "3.0");
        let _ = opt("int", json!(2));
    }
}
