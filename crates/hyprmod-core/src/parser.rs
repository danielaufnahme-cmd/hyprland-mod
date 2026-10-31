//! Read-only parser for Hyprland config files.
//!
//! Produces a flat view of the config: colon-joined keys (``decoration:blur:enabled``),
//! every occurrence of repeated keywords, ``$variables`` and the files that were read.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use indexmap::IndexMap;
use regex::Regex;

use crate::fsutil::read_text;
use crate::util::{abspath, expanduser, normpath, path_str, realpath, splitlines, strip};

/// Sections whose contents are per-device and must not leak into global keys.
pub const SKIP_SECTIONS: &[&str] = &["device"];

static SECTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([A-Za-z0-9_.\-:]+(?:\[[^\]]*\])?)\s*\{").unwrap());
static ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^([^=]+?)\s*=\s*(.*)$").unwrap());
static VAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\$([A-Za-z0-9_]+)").unwrap());
static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub value: String,
    pub file: String,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ParsedConfig {
    pub entries: Vec<Entry>,
    pub variables: IndexMap<String, String>,
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

impl ParsedConfig {
    /// Last value of every key (Hyprland semantics: later wins).
    pub fn values(&self) -> IndexMap<String, String> {
        let mut out = IndexMap::new();
        for e in &self.entries {
            out.insert(e.key.clone(), e.value.clone());
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().rev().find(|e| e.key == key).map(|e| e.value.as_str())
    }

    pub fn get_all(&self, key: &str) -> Vec<String> {
        self.entries.iter().filter(|e| e.key == key).map(|e| e.value.clone()).collect()
    }
}

/// Remove a trailing ``#`` comment; ``##`` is an escaped literal ``#``.
pub fn strip_comment(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '#' {
            if chars.peek() == Some(&'#') {
                out.push('#');
                chars.next();
                continue;
            }
            break;
        }
        out.push(c);
    }
    strip(&out).to_string()
}

/// Inverse of [`strip_comment`] for a value written to a config file.
pub fn escape_value(value: &str) -> String {
    value.replace('#', "##")
}

/// Replace ``$name`` with known variable values (longest name wins).
pub fn resolve_vars(text: &str, variables: &IndexMap<String, String>) -> String {
    if !text.contains('$') || variables.is_empty() {
        return text.to_string();
    }
    VAR_RE
        .replace_all(text, |caps: &regex::Captures| {
            let name = &caps[1];
            let cuts: Vec<usize> = name.char_indices().map(|(i, c)| i + c.len_utf8()).collect();
            for &cut in cuts.iter().rev() {
                if let Some(v) = variables.get(&name[..cut]) {
                    return format!("{}{}", v, &name[cut..]);
                }
            }
            caps[0].to_string()
        })
        .into_owned()
}

/// Remove whitespace and map ``animations:bezier``/``animations:animation`` to the global keyword.
pub fn normalize_key(key: &str) -> String {
    let key = WS_RE.replace_all(key, "");
    match key.as_ref() {
        "animations:bezier" => "bezier".into(),
        "animations:animation" => "animation".into(),
        other => other.to_string(),
    }
}

/// Expand ``~`` (with ``home``) and make ``raw`` absolute relative to ``base_dir``.
pub fn expand_path(raw: &str, base_dir: &Path, home: Option<&str>) -> PathBuf {
    let path = expanduser(strip(raw), home);
    let path = if path.is_absolute() { path } else { base_dir.join(path) };
    normpath(&path)
}

pub(crate) fn home_env() -> Option<String> {
    std::env::var("HOME").ok()
}

struct Parser {
    result: ParsedConfig,
    exclude: HashSet<PathBuf>,
    follow_sources: bool,
    visited: HashSet<PathBuf>,
    home: Option<String>,
}

impl Parser {
    fn parse_file(&mut self, path: &Path) {
        let real = realpath(path);
        if self.visited.contains(&real) {
            self.result.warnings.push(format!("skipped recursive source: {}", path_str(path)));
            return;
        }
        if self.exclude.contains(&real) {
            return;
        }
        let text = match read_text(path) {
            Ok(t) => t,
            Err(e) => {
                self.result.warnings.push(format!("cannot read {}: {}", path_str(path), e));
                return;
            }
        };
        self.visited.insert(real);
        self.result.files.push(path_str(path));
        self.parse_text(&text, path);
    }

    fn parse_text(&mut self, text: &str, path: &Path) {
        let mut stack: Vec<String> = Vec::new();
        let mut skip_depth = 0usize;
        let base_dir = abspath(path).parent().map(Path::to_path_buf).unwrap_or_else(|| "/".into());
        let name = path_str(path);
        for (idx, raw) in splitlines(text, false).into_iter().enumerate() {
            let lineno = idx + 1;
            let mut line = strip_comment(raw);
            while !line.is_empty() {
                if let Some(rest) = line.strip_prefix('}') {
                    if skip_depth > 0 {
                        skip_depth -= 1;
                    } else if stack.pop().is_none() {
                        self.result.warnings.push(format!("{name}:{lineno}: unmatched '}}'"));
                    }
                    line = strip(rest).to_string();
                    continue;
                }
                if let Some(m) = SECTION_RE.captures(&line) {
                    let section = m[1].to_string();
                    let head = section.split('[').next().unwrap_or("").split(':').next().unwrap_or("");
                    if skip_depth > 0 || (stack.is_empty() && SKIP_SECTIONS.contains(&head)) {
                        skip_depth += 1;
                    } else {
                        stack.push(section);
                    }
                    let end = m.get(0).map_or(0, |g| g.end());
                    line = strip(&line[end..]).to_string();
                    continue;
                }
                line = self.handle_statement(&line, &stack, skip_depth, &base_dir, &name, lineno);
            }
        }
        if !stack.is_empty() || skip_depth > 0 {
            let what = if stack.is_empty() { "device".to_string() } else { stack.join(", ") };
            self.result.warnings.push(format!("{name}: unclosed section(s): {what}"));
        }
    }

    /// Consume one ``key = value`` statement; returns any trailing text (a '}').
    fn handle_statement(
        &mut self,
        line: &str,
        stack: &[String],
        skip_depth: usize,
        base_dir: &Path,
        path: &str,
        lineno: usize,
    ) -> String {
        let mut line = line;
        let mut rest = String::new();
        if line.ends_with('}') && !line.contains('{') {
            rest = "}".into();
            line = line[..line.len() - 1].trim_end_matches(crate::util::is_space);
        }
        if skip_depth > 0 {
            return rest;
        }
        let Some(m) = ASSIGN_RE.captures(line) else {
            self.result.warnings.push(format!("{path}:{lineno}: unrecognized line kept as-is"));
            return rest;
        };
        let key = strip(&m[1]).to_string();
        let value = strip(&m[2]).to_string();
        if let Some(var) = key.strip_prefix('$') {
            if stack.is_empty() {
                let resolved = resolve_vars(&value, &self.result.variables);
                self.result.variables.insert(var.to_string(), resolved);
                return rest;
            }
        }
        let value = resolve_vars(&value, &self.result.variables);
        let mut parts: Vec<&str> = stack.iter().map(String::as_str).collect();
        parts.push(&key);
        let full = normalize_key(&parts.join(":"));
        if SKIP_SECTIONS.contains(&full.split(':').next().unwrap_or("")) {
            return rest;
        }
        if full == "source" && self.follow_sources {
            let pattern = expand_path(&value, base_dir, self.home.as_deref());
            let mut matches: Vec<PathBuf> = glob_sorted(&pattern);
            if matches.is_empty() {
                matches.push(pattern);
            }
            for src in matches {
                self.parse_file(&src);
            }
            return rest;
        }
        self.result.entries.push(Entry { key: full, value, file: path.to_string(), line: lineno });
        rest
    }
}

/// ``sorted(glob.glob(pattern))`` (hidden files need a literal dot).
pub(crate) fn glob_sorted(pattern: &Path) -> Vec<PathBuf> {
    let opts =
        glob::MatchOptions { case_sensitive: true, require_literal_separator: true, require_literal_leading_dot: true };
    let mut out: Vec<PathBuf> = match glob::glob_with(&path_str(pattern), opts) {
        Ok(paths) => paths.filter_map(Result::ok).collect(),
        Err(_) => {
            if pattern.symlink_metadata().is_ok() {
                vec![pattern.to_path_buf()]
            } else {
                Vec::new()
            }
        }
    };
    out.sort_by_key(|p| path_str(p));
    out
}

/// Parse ``path`` following ``source =`` includes (files in ``exclude`` are skipped).
pub fn parse_config(path: &Path, exclude: &[PathBuf]) -> ParsedConfig {
    parse_config_with_home(path, exclude, home_env().as_deref())
}

pub fn parse_config_with_home(path: &Path, exclude: &[PathBuf], home: Option<&str>) -> ParsedConfig {
    let mut p = Parser {
        result: ParsedConfig::default(),
        exclude: exclude.iter().map(|e| realpath(e)).collect(),
        follow_sources: true,
        visited: HashSet::new(),
        home: home.map(str::to_string),
    };
    p.parse_file(path);
    p.result
}

/// Parse config text without following includes.
pub fn parse_text(text: &str, path: &str) -> ParsedConfig {
    let mut p = Parser {
        result: ParsedConfig::default(),
        exclude: HashSet::new(),
        follow_sources: false,
        visited: HashSet::new(),
        home: None,
    };
    p.parse_text(text, Path::new(path));
    p.result
}
