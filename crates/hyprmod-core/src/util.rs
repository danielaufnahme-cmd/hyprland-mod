//! Small Python-compatible helpers so files and strings match the Python version exactly.

use std::path::{Component, Path, PathBuf};

use serde_json::Value as Json;

/// Python ``str.isspace`` for one character.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python ``str.strip()``.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Python ``str.split()`` (no argument).
pub fn split_ws(s: &str) -> Vec<&str> {
    s.split(is_space).filter(|p| !p.is_empty()).collect()
}

/// ``" ".join(s.split())``.
pub fn squash_ws(s: &str) -> String {
    split_ws(s).join(" ")
}

fn is_line_break(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

/// Python ``str.splitlines(keepends)``.
pub fn splitlines(s: &str, keepends: bool) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut iter = s.char_indices().peekable();
    while let Some((i, c)) = iter.next() {
        if is_line_break(c) {
            let mut end = i + c.len_utf8();
            if c == '\r' {
                if let Some(&(j, '\n')) = iter.peek() {
                    end = j + 1;
                    iter.next();
                }
            }
            out.push(if keepends { &s[start..end] } else { &s[start..i] });
            start = end;
        }
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

/// First ``n`` characters (Python ``s[:n]``).
pub fn take_chars(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Python ``str.capitalize()``.
pub fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars.flat_map(char::to_lowercase)).collect(),
        None => String::new(),
    }
}

/// Python ``str.title()``.
pub fn title(s: &str) -> String {
    let mut out = String::new();
    let mut prev_cased = false;
    for c in s.chars() {
        if prev_cased {
            out.extend(c.to_lowercase());
        } else {
            out.extend(c.to_uppercase());
        }
        prev_cased = c.is_alphabetic();
    }
    out
}

/// Python ``repr(float)``.
pub fn float_repr(v: f64) -> String {
    if v.is_nan() {
        return "nan".into();
    }
    if v.is_infinite() {
        return if v > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0.0".into() } else { "0.0".into() };
    }
    let sci = format!("{v:e}"); // shortest round-trip digits, e.g. "1.5e-7"
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let neg = mant.starts_with('-');
    let digits: String = mant.chars().filter(char::is_ascii_digit).collect();
    let sign = if neg { "-" } else { "" };
    if !(-4..16).contains(&exp) {
        let mut m = digits[..1].to_string();
        if digits.len() > 1 {
            m.push('.');
            m.push_str(&digits[1..]);
        }
        return format!("{sign}{m}e{}{:02}", if exp < 0 { '-' } else { '+' }, exp.abs());
    }
    let point = exp + 1; // digits before the decimal point
    let body = if point <= 0 {
        format!("0.{}{}", "0".repeat((-point) as usize), digits)
    } else if point as usize >= digits.len() {
        format!("{}{}.0", digits, "0".repeat(point as usize - digits.len()))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    format!("{sign}{body}")
}

/// Python ``f"{v:g}"``.
pub fn format_g(v: f64) -> String {
    if !v.is_finite() {
        return float_repr(v);
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{v:.5e}");
    let (mant, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if !(-4..6).contains(&exp) {
        format!("{}e{}{:02}", trim(mant), if exp < 0 { '-' } else { '+' }, exp.abs())
    } else {
        let decimals = (5 - exp).max(0) as usize;
        trim(&format!("{v:.decimals$}"))
    }
}

/// Python ``str(x)`` of a decoded JSON value.
pub fn py_str(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// Python ``repr`` of a str.
pub fn repr_str(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') { '"' } else { '\'' };
    let mut out = String::new();
    out.push(quote);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// Python ``repr(x)`` of a decoded JSON value.
pub fn py_repr(v: &Json) -> String {
    match v {
        Json::Null => "None".into(),
        Json::Bool(b) => if *b { "True" } else { "False" }.into(),
        Json::Number(n) => {
            if n.is_f64() {
                float_repr(n.as_f64().unwrap_or(0.0))
            } else {
                n.to_string()
            }
        }
        Json::String(s) => repr_str(s),
        Json::Array(a) => format!("[{}]", a.iter().map(py_repr).collect::<Vec<_>>().join(", ")),
        Json::Object(m) => format!(
            "{{{}}}",
            m.iter().map(|(k, v)| format!("{}: {}", repr_str(k), py_repr(v))).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// Python truthiness of a decoded JSON value.
pub fn truthy(v: &Json) -> bool {
    match v {
        Json::Null => false,
        Json::Bool(b) => *b,
        Json::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Json::String(s) => !s.is_empty(),
        Json::Array(a) => !a.is_empty(),
        Json::Object(m) => !m.is_empty(),
    }
}

/// ``data.get(key) or default`` on a JSON object.
pub fn get_or<'a>(map: &'a serde_json::Map<String, Json>, key: &str) -> Option<&'a Json> {
    map.get(key).filter(|v| truthy(v))
}

/// Python ``float(text)`` (no underscores).
pub fn parse_float(text: &str) -> Option<f64> {
    let t = strip(text);
    if t.is_empty() {
        return None;
    }
    let lower = t.to_ascii_lowercase();
    let body = lower.trim_start_matches(['+', '-']);
    if matches!(body, "inf" | "infinity" | "nan") {
        return t.parse().ok().or_else(|| {
            let neg = lower.starts_with('-');
            let v = if body == "nan" { f64::NAN } else { f64::INFINITY };
            Some(if neg { -v } else { v })
        });
    }
    if body.chars().any(|c| c.is_ascii_alphabetic() && c != 'e') {
        return None;
    }
    t.parse().ok()
}

/// Python ``int(text, 0)`` as f64 (None when Python would raise).
pub fn parse_int_base0(text: &str) -> Option<f64> {
    let t = strip(text);
    let (neg, body) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    let lower = body.to_ascii_lowercase();
    let (radix, digits) = if let Some(r) = lower.strip_prefix("0x") {
        (16, r)
    } else if let Some(r) = lower.strip_prefix("0o") {
        (8, r)
    } else if let Some(r) = lower.strip_prefix("0b") {
        (2, r)
    } else {
        if lower.is_empty() || !lower.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if lower.len() > 1 && lower.starts_with('0') && lower.bytes().any(|b| b != b'0') {
            return None;
        }
        (10, lower.as_str())
    };
    let digits = digits.strip_prefix('_').unwrap_or(digits);
    if digits.is_empty() || digits.starts_with('_') || digits.ends_with('_') || digits.contains("__") {
        return None;
    }
    let mut value = 0f64;
    for c in digits.chars().filter(|&c| c != '_') {
        value = value * radix as f64 + c.to_digit(radix)? as f64;
    }
    Some(if neg { -value } else { value })
}

/// Python ``int(text)`` (base 10, strict).
pub fn parse_int(text: &str) -> Option<i64> {
    let t = strip(text).replace('_', "");
    let body = t.strip_prefix(['+', '-']).unwrap_or(&t);
    if body.is_empty() || !body.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse().ok()
}

// ---------------------------------------------------------------- paths

/// Lexical ``os.path.normpath``.
pub fn normpath(p: &Path) -> PathBuf {
    let mut out: Vec<Component> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir) => {}
                _ => out.push(c),
            },
            other => out.push(other),
        }
    }
    let buf: PathBuf = out.iter().collect();
    if buf.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        buf
    }
}

/// ``os.path.abspath``.
pub fn abspath(p: &Path) -> PathBuf {
    if p.is_absolute() {
        normpath(p)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        normpath(&cwd.join(p))
    }
}

/// Non-strict ``os.path.realpath``: symlinks resolved as far as the path exists.
pub fn realpath(p: &Path) -> PathBuf {
    let abs = abspath(p);
    if let Ok(real) = abs.canonicalize() {
        return real;
    }
    match (abs.parent(), abs.file_name()) {
        (Some(parent), Some(name)) if parent != abs => realpath(parent).join(name),
        _ => abs,
    }
}

/// ``os.path.expanduser`` with an explicit home directory.
pub fn expanduser(raw: &str, home: Option<&str>) -> PathBuf {
    match home {
        Some(h) if raw == "~" => PathBuf::from(h),
        Some(h) if raw.starts_with("~/") => {
            let base = h.trim_end_matches('/');
            PathBuf::from(format!("{}{}", base, &raw[1..]))
        }
        _ => PathBuf::from(raw),
    }
}

/// ``os.path.relpath(path, start)`` for absolute paths.
pub fn relpath(path: &Path, start: &Path) -> PathBuf {
    let (path, start) = (abspath(path), abspath(start));
    let p: Vec<_> = path.components().collect();
    let s: Vec<_> = start.components().collect();
    let common = p.iter().zip(&s).take_while(|(a, b)| a == b).count();
    let mut out = PathBuf::new();
    for _ in common..s.len() {
        out.push("..");
    }
    for c in &p[common..] {
        out.push(c);
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_like_python() {
        assert_eq!(float_repr(1.0), "1.0");
        assert_eq!(float_repr(0.1696), "0.1696");
        assert_eq!(float_repr(1e16), "1e+16");
        assert_eq!(float_repr(1e-5), "1e-05");
        assert_eq!(float_repr(123456.5), "123456.5");
        assert_eq!(float_repr(0.0001), "0.0001");
        assert_eq!(format_g(500.0), "500");
        assert_eq!(format_g(0.5), "0.5");
        assert_eq!(format_g(1234567.0), "1.23457e+06");
    }

    #[test]
    fn strings_like_python() {
        assert_eq!(splitlines("a\r\nb\rc\n\nd", false), ["a", "b", "c", "", "d"]);
        assert_eq!(splitlines("a\nb\n", true), ["a\n", "b\n"]);
        assert_eq!(title("my_thing x2y"), "My_Thing X2Y");
        assert_eq!(capitalize("gaps IN"), "Gaps in");
        assert_eq!(parse_int_base0("0x10"), Some(16.0));
        assert_eq!(parse_int_base0("3.0"), None);
        assert_eq!(parse_int_base0("010"), None);
        assert_eq!(parse_float(" .5 "), Some(0.5));
        assert_eq!(parse_float("abc"), None);
        assert_eq!(relpath(Path::new("/a/b/c.lua"), Path::new("/a")), PathBuf::from("b/c.lua"));
        assert_eq!(relpath(Path::new("/x/c.lua"), Path::new("/a/b")), PathBuf::from("../../x/c.lua"));
    }
}
