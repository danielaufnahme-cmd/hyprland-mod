//! Timestamped file backups.

use anyhow::{anyhow, Context, Result};
use chrono::Local;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use super::fsutil::{atomic_write, read_text};

static ID_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[0-9]{8}-[0-9]{6}-[0-9]{6}(?:-[0-9]+)?$").unwrap());

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileEntry {
    pub path: String,
    pub copy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub id: String,
    pub created: String,
    pub label: String,
    pub files: Vec<BackupFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
    pub id: String,
    pub created: String,
    pub label: String,
    pub files: Vec<String>,
}

pub struct BackupStore {
    root: PathBuf,
}

impl BackupStore {
    pub fn new<P: AsRef<Path>>(data_dir: P) -> Self {
        Self {
            root: data_dir.as_ref().join("backups"),
        }
    }

    pub fn create(
        &self,
        label: &str,
        files: &[PathBuf],
        texts: Option<&HashMap<PathBuf, String>>,
    ) -> Result<String> {
        let now = Local::now();
        let base = now.format("%Y%m%d-%H%M%S-%6f").to_string();
        let mut backup_id = base.clone();
        let mut n = 1;

        while self.root.join(&backup_id).exists() {
            backup_id = format!("{}-{}", base, n);
            n += 1;
        }

        let folder = self.root.join(&backup_id);
        let files_dir = folder.join("files");
        fs::create_dir_all(&files_dir)
            .with_context(|| format!("failed to create backup dir {}", files_dir.display()))?;

        let mut saved = Vec::new();
        for (i, path) in files.iter().enumerate() {
            if !path.is_file() {
                continue;
            }
            let filename = path.file_name().unwrap_or_default().to_string_lossy();
            let copy_name = format!("{:03}-{}", i, filename);
            let copy_path = files_dir.join(&copy_name);

            if let Some(override_content) = texts.and_then(|m| m.get(path)) {
                atomic_write(&copy_path, override_content)?;
            } else {
                fs::copy(path, &copy_path).with_context(|| {
                    format!("failed to copy {} to {}", path.display(), copy_path.display())
                })?;
            }

            saved.push(BackupFileEntry {
                path: path.to_string_lossy().to_string(),
                copy: copy_name,
            });
        }

        let manifest = BackupManifest {
            id: backup_id.clone(),
            created: now.to_rfc3339(),
            label: label.to_string(),
            files: saved,
        };

        let json = serde_json::to_string_pretty(&manifest)?;
        atomic_write(folder.join("manifest.json"), &json)?;

        Ok(backup_id)
    }

    pub fn list(&self) -> Vec<BackupSummary> {
        let mut out = Vec::new();
        if !self.root.is_dir() {
            return out;
        }

        if let Ok(entries) = fs::read_dir(&self.root) {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().to_string();
                if let Some(manifest) = self.get_manifest(&id) {
                    out.push(BackupSummary {
                        id: manifest.id,
                        created: manifest.created,
                        label: manifest.label,
                        files: manifest.files.into_iter().map(|f| f.path).collect(),
                    });
                }
            }
        }

        out.sort_by(|a, b| b.id.cmp(&a.id));
        out
    }

    pub fn restore(&self, backup_id: &str) -> Result<Vec<String>> {
        let manifest = self
            .get_manifest(backup_id)
            .ok_or_else(|| anyhow!("unknown backup: {}", backup_id))?;
        let folder = self.root.join(backup_id).join("files");
        let mut restored = Vec::new();

        for item in manifest.files {
            let content = read_text(folder.join(&item.copy))?;
            atomic_write(&item.path, &content)?;
            restored.push(item.path);
        }

        Ok(restored)
    }

    pub fn get_manifest(&self, backup_id: &str) -> Option<BackupManifest> {
        if !ID_RE.is_match(backup_id) {
            return None;
        }
        let manifest_path = self.root.join(backup_id).join("manifest.json");
        let text = fs::read_to_string(manifest_path).ok()?;
        serde_json::from_str(&text).ok()
    }
}
