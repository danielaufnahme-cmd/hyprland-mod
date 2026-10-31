//! Safe file helpers: lossless text reads and atomic writes that follow symlinks and keep permissions.
//!
//! Bytes that are not valid UTF-8 are mapped to the private-use characters
//! U+10FF80..U+10FFFF on read and back to the original bytes on write, so any
//! file round-trips unchanged (like Python's ``surrogateescape``).

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::util::realpath;

const ESCAPE_BASE: u32 = 0x10FF00;

pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut rest = bytes;
    loop {
        match std::str::from_utf8(rest) {
            Ok(s) => {
                out.push_str(s);
                return out;
            }
            Err(e) => {
                let (good, bad) = rest.split_at(e.valid_up_to());
                out.push_str(std::str::from_utf8(good).unwrap_or_default());
                let n = e.error_len().unwrap_or(bad.len());
                for &b in &bad[..n] {
                    out.push(char::from_u32(ESCAPE_BASE + b as u32).unwrap_or('\u{fffd}'));
                }
                rest = &bad[n..];
            }
        }
    }
}

pub fn encode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut buf = [0u8; 4];
    for c in text.chars() {
        let cp = c as u32;
        if (ESCAPE_BASE + 0x80..=ESCAPE_BASE + 0xFF).contains(&cp) {
            out.push((cp - ESCAPE_BASE) as u8);
        } else {
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    }
    out
}

pub fn read_text(path: &Path) -> io::Result<String> {
    Ok(decode(&fs::read(path)?))
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write via temp file + fsync + rename in the target's real directory.
///
/// Symlinks (e.g. dotfile managers) are resolved so the link itself survives.
/// New files are created with mode 0600, existing files keep their mode.
pub fn atomic_write(path: &Path, text: &str) -> io::Result<()> {
    atomic_write_bytes(path, &encode(text))
}

pub fn atomic_write_bytes(path: &Path, data: &[u8]) -> io::Result<()> {
    let target = realpath(path);
    let dir = target.parent().map(Path::to_path_buf).unwrap_or_else(|| "/".into());
    fs::create_dir_all(&dir)?;
    let mode = fs::metadata(&target).ok().map(|m| m.permissions().mode() & 0o7777);
    let (tmp, mut file) = loop {
        let nanos =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".hyprland-mod-{}{n}{nanos:x}.tmp", std::process::id()));
        match fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&tmp) {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(data)?;
        file.flush()?;
        file.sync_all()?;
        if let Some(mode) = mode {
            fs::set_permissions(&tmp, fs::Permissions::from_mode(mode))?;
        }
        fs::rename(&tmp, &target)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_utf8_round_trips() {
        let raw = b"a = \xff\xfe ok \xc3\xa9\n";
        assert_eq!(encode(&decode(raw)), raw);
    }

    #[test]
    fn atomic_write_keeps_mode_and_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.conf");
        fs::write(&real, "x").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("link.conf");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        atomic_write(&link, "new").unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        assert_eq!(fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640);
        let fresh = dir.path().join("sub/new.txt");
        atomic_write(&fresh, "y").unwrap();
        assert_eq!(fs::metadata(&fresh).unwrap().permissions().mode() & 0o777, 0o600);
    }
}
