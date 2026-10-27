//! Filesystem utility functions: safe atomic writing and reading.

use anyhow::{Context, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

/// Write text to a temporary file in the same directory, sync to disk, and atomic rename.
pub fn atomic_write<P: AsRef<Path>>(path: P, content: &str) -> Result<()> {
    let path = path.as_ref();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .with_context(|| format!("failed to create parent directories for {}", path.display()))?;

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_path = parent.join(format!(".tmp-{}-{}", std::process::id(), nanos));
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("failed to create temp file {}", tmp_path.display()))?;
        file.write_all(content.as_bytes())
            .with_context(|| format!("failed to write temp file {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to sync temp file {}", tmp_path.display()))?;
    }

    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to rename {} to {}", tmp_path.display(), path.display()))?;
    Ok(())
}

/// Read text from file with error context.
pub fn read_text<P: AsRef<Path>>(path: P) -> Result<String> {
    let path = path.as_ref();
    fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))
}
