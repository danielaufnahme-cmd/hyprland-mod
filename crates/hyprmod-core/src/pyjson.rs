//! ``json.dumps`` byte-compatible with Python's default encoder.

use serde_json::Value as Json;

use crate::util::float_repr;

#[derive(Clone, Copy, Debug, Default)]
pub struct Dumps {
    pub indent: Option<usize>,
    pub sort_keys: bool,
    pub ensure_ascii: bool,
}

impl Dumps {
    /// ``json.dumps(v)`` defaults (ensure_ascii on, no indent).
    pub fn compact() -> Self {
        Dumps { indent: None, sort_keys: false, ensure_ascii: true }
    }
    pub fn sorted() -> Self {
        Dumps { sort_keys: true, ..Self::compact() }
    }
    pub fn indented(n: usize) -> Self {
        Dumps { indent: Some(n), ..Self::compact() }
    }
}

pub fn dumps(v: &Json, opts: Dumps) -> String {
    let mut out = String::new();
    write_value(&mut out, v, opts, 0);
    out
}

fn write_value(out: &mut String, v: &Json, o: Dumps, level: usize) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Number(n) => {
            if n.is_f64() {
                let f = n.as_f64().unwrap_or(0.0);
                out.push_str(&match f {
                    f if f.is_nan() => "NaN".into(),
                    f if f.is_infinite() => if f > 0.0 { "Infinity" } else { "-Infinity" }.into(),
                    f => float_repr(f),
                });
            } else {
                out.push_str(&n.to_string());
            }
        }
        Json::String(s) => write_str(out, s, o.ensure_ascii),
        Json::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                sep(out, o, level + 1, i == 0);
                write_value(out, item, o, level + 1);
            }
            close(out, o, level);
            out.push(']');
        }
        Json::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut entries: Vec<(&String, &Json)> = map.iter().collect();
            if o.sort_keys {
                entries.sort_by(|a, b| a.0.cmp(b.0));
            }
            out.push('{');
            for (i, (k, val)) in entries.into_iter().enumerate() {
                sep(out, o, level + 1, i == 0);
                write_str(out, k, o.ensure_ascii);
                out.push_str(": ");
                write_value(out, val, o, level + 1);
            }
            close(out, o, level);
            out.push('}');
        }
    }
}

fn sep(out: &mut String, o: Dumps, level: usize, first: bool) {
    match o.indent {
        Some(n) => {
            if !first {
                out.push(',');
            }
            out.push('\n');
            out.push_str(&" ".repeat(n * level));
        }
        None => {
            if !first {
                out.push_str(", ");
            }
        }
    }
}

fn close(out: &mut String, o: Dumps, level: usize) {
    if let Some(n) = o.indent {
        out.push('\n');
        out.push_str(&" ".repeat(n * level));
    }
}

fn write_str(out: &mut String, s: &str, ascii: bool) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if ascii && (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_python() {
        let v = json!({"b": [1, 2.5, "é"], "a": {}, "c": []});
        assert_eq!(dumps(&v, Dumps::sorted()), r#"{"a": {}, "b": [1, 2.5, "\u00e9"], "c": []}"#);
        assert_eq!(
            dumps(&v, Dumps::indented(1)),
            "{\n \"b\": [\n  1,\n  2.5,\n  \"\\u00e9\"\n ],\n \"a\": {},\n \"c\": []\n}"
        );
        let o = Dumps { indent: Some(2), sort_keys: false, ensure_ascii: false };
        assert_eq!(dumps(&json!({"x": "é😀"}), o), "{\n  \"x\": \"é😀\"\n}");
        assert_eq!(dumps(&json!("😀\x7f"), Dumps::compact()), r#""\ud83d\ude00\u007f""#);
    }
}
