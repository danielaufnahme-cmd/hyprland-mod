//! Thin hyprctl wrapper. Never invoked in dry-run mode.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// ``(returncode, stdout, stderr)``.
pub type RunOutput = (i32, String, String);

/// Runs ``hyprctl <args>``. Tests use a fake; the real one is [`HyprctlRunner`].
pub trait Runner {
    fn run(&self, args: &[String]) -> anyhow::Result<RunOutput>;
}

impl<F: Fn(&[String]) -> anyhow::Result<RunOutput>> Runner for F {
    fn run(&self, args: &[String]) -> anyhow::Result<RunOutput> {
        self(args)
    }
}

/// Runs the real ``hyprctl`` (no shell, 5 s timeout).
#[derive(Clone, Copy, Debug, Default)]
pub struct HyprctlRunner;

impl Runner for HyprctlRunner {
    fn run(&self, args: &[String]) -> anyhow::Result<RunOutput> {
        let child = Command::new("hyprctl")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok((127, String::new(), "hyprctl not found".into()))
            }
            Err(e) => return Err(e.into()),
        };
        let reader = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut p) = pipe {
                    let _ = p.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let out = reader(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let err = reader(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let (stdout, stderr) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        Ok(match status {
            Some(s) => (s.code().unwrap_or(-1), stdout, stderr),
            None => (124, String::new(), "hyprctl timed out".into()),
        })
    }
}

fn env_truthy(value: Option<String>) -> bool {
    value.is_some_and(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// Dry-run when explicitly requested or when not inside a Hyprland session.
pub fn is_dry_run_with(get: impl Fn(&str) -> Option<String>) -> bool {
    env_truthy(get("HYPRLAND_MOD_DRY_RUN")) || get("HYPRLAND_INSTANCE_SIGNATURE").is_none_or(|v| v.is_empty())
}

pub fn is_dry_run() -> bool {
    is_dry_run_with(|k| std::env::var(k).ok())
}

/// ``[(k, v), ...]`` -> ``"keyword k v ; keyword k2 v2"``.
pub fn build_batch(pairs: &[(String, String)]) -> String {
    pairs.iter().map(|(k, v)| format!("keyword {k} {v}")).collect::<Vec<_>>().join(" ; ")
}

/// Error messages from hyprctl output (each command answers ``ok``).
pub fn batch_errors(code: i32, stdout: &str, stderr: &str) -> Vec<String> {
    let mut errors: Vec<String> = crate::util::splitlines(stdout, false)
        .into_iter()
        .map(crate::util::strip)
        .filter(|l| !l.is_empty() && l.to_lowercase() != "ok")
        .map(str::to_string)
        .collect();
    let err = crate::util::strip(stderr);
    if !err.is_empty() {
        errors.push(err.to_string());
    }
    if code != 0 && errors.is_empty() {
        errors.push(format!("hyprctl exited with code {code}"));
    }
    errors
}
