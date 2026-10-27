//! Hyprland configuration file parser.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

static SECTION_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z0-9_.\-:]+(?:\[[^\]]*\])?)\s*\{").unwrap());
static ASSIGN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^([^=]+?)\s*=\s*(.*)$").unwrap());
static VAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$([A-Za-z0-9_]+)").unwrap());

const SKIP_SECTIONS: &[&str] = &["device"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedConfig {
    pub entries: Vec<Entry>,
    pub variables: HashMap<String, String>,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

impl ParsedConfig {
    /// Return the last value for each key (Hyprland later-wins rule).
    pub fn values(&self) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for e in &self.entries {
            map.insert(e.key.clone(), e.value.clone());
        }
        map
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        for e in self.entries.iter().rev() {
            if e.key == key {
                return Some(&e.value);
            }
        }
        None
    }

    pub fn get_all(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.key == key)
            .map(|e| e.value.as_str())
            .collect()
    }
}

pub fn strip_comment(line: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#' {
            if i + 1 < chars.len() && chars[i + 1] == '#' {
                out.push('#');
                i += 2;
                continue;
            }
            break;
        }
        out.push(chars[i]);
        i += 1;
    }
    out.trim().to_string()
}

pub fn escape_value(value: &str) -> String {
    value.replace('#', "##")
}

pub fn resolve_vars(text: &str, variables: &HashMap<String, String>) -> String {
    if !text.contains('$') || variables.is_empty() {
        return text.to_string();
    }

    VAR_RE
        .replace_all(text, |caps: &regex::Captures| {
            let name = &caps[1];
            for cut in (1..=name.len()).rev() {
                let prefix = &name[..cut];
                if let Some(val) = variables.get(prefix) {
                    let mut res = val.clone();
                    res.push_str(&name[cut..]);
                    return res;
                }
            }
            caps[0].to_string()
        })
        .to_string()
}

pub fn normalize_key(key: &str) -> String {
    let clean: String = key.chars().filter(|c| !c.is_whitespace()).collect();
    match clean.as_str() {
        "animations:bezier" => "bezier".to_string(),
        "animations:animation" => "animation".to_string(),
        _ => clean,
    }
}

pub fn expand_path<P: AsRef<Path>>(raw: &str, base_dir: P) -> PathBuf {
    let raw = raw.trim();
    let expanded = if let Some(stripped) = raw.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            home.join(stripped)
        } else {
            PathBuf::from(raw)
        }
    } else {
        PathBuf::from(raw)
    };

    if expanded.is_absolute() {
        expanded
    } else {
        base_dir.as_ref().join(expanded)
    }
}

pub struct Parser {
    pub result: ParsedConfig,
    exclude: HashSet<PathBuf>,
    follow_sources: bool,
    visited: HashSet<PathBuf>,
}

impl Parser {
    pub fn new(exclude: HashSet<PathBuf>, follow_sources: bool) -> Self {
        Self {
            result: ParsedConfig::default(),
            exclude,
            follow_sources,
            visited: HashSet::new(),
        }
    }

    pub fn parse_file<P: AsRef<Path>>(&mut self, path: P) {
        let path = path.as_ref();
        let canonical = match path.canonicalize() {
            Ok(c) => c,
            Err(_) => path.to_path_buf(),
        };

        if self.visited.contains(&canonical) {
            self.result
                .warnings
                .push(format!("skipped recursive source: {}", path.display()));
            return;
        }
        if self.exclude.contains(&canonical) {
            return;
        }

        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.result
                    .warnings
                    .push(format!("cannot read {}: {}", path.display(), e));
                return;
            }
        };

        self.visited.insert(canonical);
        self.result.files.push(path.display().to_string());
        self.parse_text(&text, path.to_string_lossy().as_ref());
    }

    pub fn parse_text(&mut self, text: &str, path: &str) {
        let mut stack: Vec<String> = Vec::new();
        let mut skip_depth: usize = 0;
        let base_dir = Path::new(path).parent().unwrap_or_else(|| Path::new("."));

        for (lineno, raw) in text.lines().enumerate() {
            let mut line = strip_comment(raw);
            while !line.is_empty() {
                if line.starts_with('}') {
                    if skip_depth > 0 {
                        skip_depth -= 1;
                    } else if !stack.is_empty() {
                        stack.pop();
                    } else {
                        self.result
                            .warnings
                            .push(format!("{}:{}: unmatched '}}'", path, lineno + 1));
                    }
                    line = line[1..].trim().to_string();
                    continue;
                }

                if let Some(caps) = SECTION_RE.captures(&line) {
                    let name = caps.get(1).unwrap().as_str().to_string();
                    let section_root = name.split('[').next().unwrap_or("").split(':').next().unwrap_or("");
                    if skip_depth > 0 || (stack.is_empty() && SKIP_SECTIONS.contains(&section_root)) {
                        skip_depth += 1;
                    } else {
                        stack.push(name);
                    }
                    line = line[caps.get(0).unwrap().end()..].trim().to_string();
                    continue;
                }

                line = self.handle_statement(line, &stack, skip_depth, base_dir, path, lineno + 1);
            }
        }

        if !stack.is_empty() || skip_depth > 0 {
            let unclosed = if stack.is_empty() {
                "device".to_string()
            } else {
                stack.join(", ")
            };
            self.result.warnings.push(format!(
                "{}: unclosed section(s): {}",
                path, unclosed
            ));
        }
    }

    fn handle_statement(
        &mut self,
        mut line: String,
        stack: &[String],
        skip_depth: usize,
        base_dir: &Path,
        path: &str,
        lineno: usize,
    ) -> String {
        let mut rest = String::new();
        if line.ends_with('}') && !line.contains('{') {
            rest = "}".to_string();
            line = line[..line.len() - 1].trim_end().to_string();
        }

        if skip_depth > 0 {
            return rest;
        }

        if let Some(caps) = ASSIGN_RE.captures(&line) {
            let key = caps.get(1).unwrap().as_str().trim();
            let value = caps.get(2).unwrap().as_str().trim();

            if key.starts_with('$') && stack.is_empty() {
                let var_name = &key[1..];
                let resolved = resolve_vars(value, &self.result.variables);
                self.result.variables.insert(var_name.to_string(), resolved);
                return rest;
            }

            let resolved_val = resolve_vars(value, &self.result.variables);
            let mut parts = stack.to_vec();
            parts.push(key.to_string());
            let full = normalize_key(&parts.join(":"));

            let section_root = full.split(':').next().unwrap_or("");
            if SKIP_SECTIONS.contains(&section_root) {
                return rest;
            }

            if full == "source" {
                if !self.follow_sources {
                    self.result.entries.push(Entry {
                        key: full,
                        value: resolved_val,
                        file: path.to_string(),
                        line: lineno,
                    });
                    return rest;
                }

                let target = expand_path(&resolved_val, base_dir);
                self.parse_file(target);
                return rest;
            }

            self.result.entries.push(Entry {
                key: full,
                value: resolved_val,
                file: path.to_string(),
                line: lineno,
            });
        } else if !line.trim().is_empty() {
            self.result
                .warnings
                .push(format!("{}:{}: unrecognized line kept as-is", path, lineno));
        }

        rest
    }
}

pub fn parse_config<P: AsRef<Path>>(path: P, exclude: &[PathBuf]) -> ParsedConfig {
    let mut exclude_set = HashSet::new();
    for p in exclude {
        if let Ok(c) = p.canonicalize() {
            exclude_set.insert(c);
        } else {
            exclude_set.insert(p.clone());
        }
    }
    let mut parser = Parser::new(exclude_set, true);
    parser.parse_file(path);
    parser.result
}

pub fn parse_text(text: &str, path: &str) -> ParsedConfig {
    let mut parser = Parser::new(HashSet::new(), false);
    parser.parse_text(text, path);
    parser.result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_comment() {
        assert_eq!(strip_comment("gaps_in = 5 # comment"), "gaps_in = 5");
        assert_eq!(strip_comment("col = ##112233 # hex"), "col = #112233");
    }

    #[test]
    fn test_parse_text_sections() {
        let conf = r#"
        $gaps = 10
        general {
            gaps_in = $gaps
            gaps_out = 20
        }
        decoration {
            blur {
                enabled = true
            }
        }
        "#;
        let parsed = parse_text(conf, "<test>");
        assert_eq!(parsed.get("general:gaps_in"), Some("10"));
        assert_eq!(parsed.get("general:gaps_out"), Some("20"));
        assert_eq!(parsed.get("decoration:blur:enabled"), Some("true"));
    }
}
