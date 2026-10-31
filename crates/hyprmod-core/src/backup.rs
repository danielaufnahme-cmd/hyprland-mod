//! Timestamped file backups stored under ``<data_dir>/backups/<id>/``.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{anyhow, Result};
use regex::Regex;
use serde_json::{json, Value as Json};

use crate::fsutil::{atomic_write, read_text};
use crate::pyjson::{dumps, Dumps};
use crate::util::{abspath, path_str, realpath};

static ID_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[0-9]{8}-[0-9]{6}-[0-9]{6}(?:-[0-9]+)?$").unwrap());

/// One backup as listed (``files`` are the original paths).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backup {
    pub id: String,
    /// Local time, ``YYYY-MM-DDTHH:MM:SS``.
    pub created: String,
    pub label: String,
    pub files: Vec<PathBuf>,
}

struct Manifest {
    backup: Backup,
    copies: Vec<(PathBuf, String)>,
}

pub struct BackupStore {
    pub root: PathBuf,
}

impl BackupStore {
    pub fn new(data_dir: &Path) -> Self {
        BackupStore { root: data_dir.join("backups") }
    }

    /// Copy every existing file in ``files``; returns the new backup id.
    ///
    /// ``texts`` maps a path to the content to store instead of the file's current one.
    pub fn create(&self, label: &str, files: &[PathBuf], texts: &HashMap<PathBuf, String>) -> Result<String> {
        let now = chrono::Local::now().naive_local();
        let base = now.format("%Y%m%d-%H%M%S-%6f").to_string();
        let mut backup_id = base.clone();
        let mut n = 1;
        while self.root.join(&backup_id).exists() {
            backup_id = format!("{base}-{n}");
            n += 1;
        }
        let folder = self.root.join(&backup_id);
        fs::create_dir_all(&self.root)?;
        fs::create_dir(&folder)?;
        fs::create_dir(folder.join("files"))?;
        let overrides: HashMap<PathBuf, &String> = texts.iter().map(|(k, v)| (abspath(k), v)).collect();
        let mut unique: Vec<PathBuf> = Vec::new();
        for f in files {
            let p = abspath(f);
            if !unique.contains(&p) {
                unique.push(p);
            }
        }
        let mut saved = Vec::new();
        for (i, path) in unique.iter().enumerate() {
            if !path.is_file() {
                continue;
            }
            let name = format!("{i:03}-{}", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default());
            let dest = folder.join("files").join(&name);
            match overrides.get(path) {
                Some(text) => atomic_write(&dest, text)?,
                None => {
                    fs::copy(realpath(path), &dest)?;
                }
            }
            saved.push(json!({"path": path_str(path), "copy": name}));
        }
        let manifest = json!({
            "id": backup_id,
            "created": now.format("%Y-%m-%dT%H:%M:%S").to_string(),
            "label": label,
            "files": saved,
        });
        fs::write(folder.join("manifest.json"), dumps(&manifest, Dumps::indented(2)))?;
        Ok(backup_id)
    }

    /// Newest first.
    pub fn list(&self) -> Vec<Backup> {
        let Ok(dir) = fs::read_dir(&self.root) else { return Vec::new() };
        let mut out: Vec<Backup> = dir
            .filter_map(|e| e.ok())
            .filter_map(|e| self.manifest(&e.file_name().to_string_lossy()))
            .map(|m| m.backup)
            .collect();
        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }

    /// Write every backed-up file back to its original path; returns those paths.
    pub fn restore(&self, backup_id: &str) -> Result<Vec<PathBuf>> {
        let manifest = self.manifest(backup_id).ok_or_else(|| anyhow!("unknown backup: {backup_id}"))?;
        let folder = self.root.join(backup_id).join("files");
        let mut restored = Vec::new();
        for (path, copy) in manifest.copies {
            atomic_write(&path, &read_text(&folder.join(copy))?)?;
            restored.push(path);
        }
        Ok(restored)
    }

    fn manifest(&self, backup_id: &str) -> Option<Manifest> {
        if !ID_RE.is_match(backup_id) {
            return None;
        }
        let text = fs::read_to_string(self.root.join(backup_id).join("manifest.json")).ok()?;
        let data: Json = serde_json::from_str(&text).ok()?;
        let obj = data.as_object()?;
        let files = obj.get("files")?.as_array()?;
        let copies: Vec<(PathBuf, String)> = files
            .iter()
            .filter_map(|f| {
                let path = f.get("path")?.as_str()?;
                let copy = f.get("copy")?.as_str()?;
                (!copy.contains('/')).then(|| (PathBuf::from(path), copy.to_string()))
            })
            .collect();
        let text_of = |k: &str| obj.get(k).map(crate::util::py_str).unwrap_or_default();
        Some(Manifest {
            backup: Backup {
                id: backup_id.to_string(),
                created: text_of("created"),
                label: text_of("label"),
                files: copies.iter().map(|(p, _)| p.clone()).collect(),
            },
            copies,
        })
    }
}
