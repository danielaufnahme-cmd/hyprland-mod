//! Conversion between Rust values and Hyprland configuration strings.

use anyhow::{bail, Result};
use regex::Regex;
use std::sync::LazyLock;

static HEX_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9a-fA-F]+$").unwrap());
static DEG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?\d+(?:\.\d+)?deg$").unwrap());
static FUNC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(rgba?)\(\s*(.*?)\s*\)$").unwrap());

fn clamp_byte(v: f64) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

fn to_rgba_string(r: u8, g: u8, b: u8, a: u8) -> String {
    format!("rgba({:02x}{:02x}{:02x}{:02x})", r, g, b, a)
}

/// Normalize any Hyprland color notation to `rgba(rrggbbaa)`.
pub fn parse_color(text: &str) -> Option<String> {
    let t = text.trim();

    if let Some(caps) = FUNC_RE.captures(t) {
        let func = caps.get(1)?.as_str().to_lowercase();
        let body = caps.get(2)?.as_str();

        if body.contains(',') {
            let parts: Vec<&str> = body.split(',').map(|s| s.trim()).collect();
            let nums: Vec<f64> = parts.into_iter().filter_map(|p| p.parse::<f64>().ok()).collect();
            if func == "rgb" && nums.len() == 3 {
                return Some(to_rgba_string(
                    clamp_byte(nums[0]),
                    clamp_byte(nums[1]),
                    clamp_byte(nums[2]),
                    255,
                ));
            } else if func == "rgba" && nums.len() == 4 {
                let alpha = if nums[3] <= 1.0 { nums[3] * 255.0 } else { nums[3] };
                return Some(to_rgba_string(
                    clamp_byte(nums[0]),
                    clamp_byte(nums[1]),
                    clamp_byte(nums[2]),
                    clamp_byte(alpha),
                ));
            }
            return None;
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

    // 0xAARRGGBB format
    if (t.starts_with("0x") || t.starts_with("0X")) && t.len() == 10 && HEX_RE.is_match(&t[2..]) {
        let h = t[2..].to_lowercase();
        let aa = &h[0..2];
        let rrggbb = &h[2..8];
        return Some(format!("rgba({}{})", rrggbb, aa));
    }

    // Bare rrggbbaa (8 chars)
    if t.len() == 8 && HEX_RE.is_match(t) {
        return Some(format!("rgba({})", t.to_lowercase()));
    }

    // #rrggbb or #rrggbbaa
    if let Some(stripped) = t.strip_prefix('#') {
        if HEX_RE.is_match(stripped) {
            let h = stripped.to_lowercase();
            if h.len() == 6 {
                return Some(format!("rgba({}ff)", h));
            } else if h.len() == 8 {
                return Some(format!("rgba({})", h));
            }
        }
    }

    None
}

/// Convert an accepted color string to (r, g, b, a) bytes.
pub fn color_to_rgba_tuple(color: &str) -> Option<(u8, u8, u8, u8)> {
    let norm = parse_color(color)?;
    if norm.starts_with("rgba(") && norm.ends_with(')') && norm.len() == 14 {
        let hex = &norm[5..13];
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some((r, g, b, a))
    } else {
        None
    }
}

/// Canonical gradient text: every color token as `rgba(rrggbbaa)`, angles kept.
pub fn normalize_gradient(text: &str) -> String {
    let out: Vec<String> = text
        .split_whitespace()
        .map(|tok| {
            if DEG_RE.is_match(tok) {
                tok.to_string()
            } else {
                parse_color(tok).unwrap_or_else(|| tok.to_string())
            }
        })
        .collect();
    out.join(" ")
}

pub fn format_float(val: f64) -> String {
    let s = format!("{:.6}", val);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    if s.is_empty() || s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

pub fn parse_bool(text: &str) -> Option<bool> {
    let lower = text.trim().to_lowercase();
    match lower.as_str() {
        "true" | "yes" | "on" | "1" => Some(true),
        "false" | "no" | "off" | "0" => Some(false),
        _ => None,
    }
}

pub fn format_bool(val: bool) -> &'static str {
    if val { "true" } else { "false" }
}

/// Format a value based on declared schema type.
pub fn format_value(type_name: &str, raw: &str) -> Result<String> {
    match type_name {
        "boolean" => {
            let b = parse_bool(raw)
                .ok_or_else(|| anyhow::anyhow!("invalid boolean: {}", raw))?;
            Ok(format_bool(b).to_string())
        }
        "color" => {
            parse_color(raw).ok_or_else(|| anyhow::anyhow!("invalid color: {}", raw))
        }
        "gradient" => {
            let g = normalize_gradient(raw);
            if g.is_empty() {
                bail!("empty gradient");
            }
            Ok(g)
        }
        "int" | "integer" => {
            let num: i64 = raw.trim().parse()?;
            Ok(num.to_string())
        }
        "float" => {
            let num: f64 = raw.trim().parse()?;
            Ok(format_float(num))
        }
        "vec2" => {
            let parts: Vec<&str> = raw
                .split(|c| c == ' ' || c == ',')
                .filter(|s| !s.trim().is_empty())
                .collect();
            if parts.len() != 2 {
                bail!("invalid vec2: {}", raw);
            }
            let x: f64 = parts[0].parse()?;
            let y: f64 = parts[1].parse()?;
            Ok(format!("{} {}", format_float(x), format_float(y)))
        }
        _ => Ok(raw.trim().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color() {
        assert_eq!(parse_color("rgb(255, 0, 128)"), Some("rgba(ff0080ff)".into()));
        assert_eq!(parse_color("rgba(0, 255, 0, 0.5)"), Some("rgba(00ff0080)".into()));
        assert_eq!(parse_color("0xff112233"), Some("rgba(112233ff)".into()));
        assert_eq!(parse_color("#aabbcc"), Some("rgba(aabbccff)".into()));
        assert_eq!(parse_color("#aabbccdd"), Some("rgba(aabbccdd)".into()));
        assert_eq!(parse_color("11223344"), Some("rgba(11223344)".into()));
        assert_eq!(parse_color("not_a_color"), None);
    }

    #[test]
    fn test_gradient() {
        let grad = normalize_gradient("#ff0000 #0000ff 45deg");
        assert_eq!(grad, "rgba(ff0000ff) rgba(0000ffff) 45deg");
    }

    #[test]
    fn test_booleans() {
        assert_eq!(parse_bool("yes"), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("on"), Some(true));
    }
}
