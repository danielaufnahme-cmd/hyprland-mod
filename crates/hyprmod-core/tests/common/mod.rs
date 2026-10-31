//! Shared test setup: every test works on a temp copy of the fixtures, never real files,
//! with a fake environment (HOME inside the temp dir) and a fake hyprctl runner.
#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use hyprmod_core::{ConfigManager, ManagerOptions, RunOutput, Runner};

/// The Python project's fixtures (read-only).
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Rust-only fixtures (Python-written files for the cross-compat tests).
pub fn rust_fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

pub fn app_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

pub type Calls = Rc<RefCell<Vec<Vec<String>>>>;
type Reply = Rc<dyn Fn(&[String]) -> anyhow::Result<RunOutput>>;

/// Records hyprctl invocations instead of running anything.
#[derive(Clone)]
pub struct FakeRunner {
    pub calls: Calls,
    reply: Reply,
}

impl FakeRunner {
    pub fn ok() -> Self {
        Self::replying(|_| Ok((0, "ok".into(), String::new())))
    }

    pub fn replying(f: impl Fn(&[String]) -> anyhow::Result<RunOutput> + 'static) -> Self {
        FakeRunner { calls: Rc::new(RefCell::new(Vec::new())), reply: Rc::new(f) }
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.borrow().clone()
    }

    pub fn clear(&self) {
        self.calls.borrow_mut().clear();
    }
}

impl Runner for FakeRunner {
    fn run(&self, args: &[String]) -> anyhow::Result<RunOutput> {
        self.calls.borrow_mut().push(args.to_vec());
        (self.reply)(args)
    }
}

/// Panics on any call: hyprctl must never run in dry-run mode.
pub struct ForbiddenRunner;

impl Runner for ForbiddenRunner {
    fn run(&self, args: &[String]) -> anyhow::Result<RunOutput> {
        panic!("hyprctl must not run in dry-run mode: {args:?}");
    }
}

pub fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

pub struct TempConfig {
    pub tmp: tempfile::TempDir,
    pub hypr: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
    pub managed: PathBuf,
    pub original_text: String,
    pub env: HashMap<String, String>,
}

impl TempConfig {
    /// Copy of ``sample_hyprland.conf`` + ``extra.conf`` (dry-run).
    pub fn conf() -> Self {
        let tmp = tempfile::Builder::new().prefix("hyprmod-core-test-").tempdir().unwrap();
        let hypr = tmp.path().join("hypr");
        fs::create_dir_all(&hypr).unwrap();
        fs::copy(fixtures().join("sample_hyprland.conf"), hypr.join("hyprland.conf")).unwrap();
        fs::copy(fixtures().join("extra.conf"), hypr.join("extra.conf")).unwrap();
        Self::finish(tmp, hypr, "hyprland.conf", "hyprland-mod.conf")
    }

    /// Copy of ``sample_hyprland.lua`` + ``colors.lua`` with the test stubs (dry-run).
    pub fn lua() -> Self {
        let tmp = tempfile::Builder::new().prefix("hyprmod-core-test-").tempdir().unwrap();
        let hypr = tmp.path().join("hypr");
        fs::create_dir_all(&hypr).unwrap();
        fs::copy(fixtures().join("sample_hyprland.lua"), hypr.join("hyprland.lua")).unwrap();
        fs::copy(fixtures().join("colors.lua"), hypr.join("colors.lua")).unwrap();
        let mut t = Self::finish(tmp, hypr, "hyprland.lua", "hyprland-mod.lua");
        t.env.insert("HYPRLAND_MOD_LUA_STUBS".into(), fixtures().join("hl_stubs.lua").to_string_lossy().into());
        t
    }

    fn finish(tmp: tempfile::TempDir, hypr: PathBuf, config: &str, managed: &str) -> Self {
        let data = tmp.path().join("data");
        let config = hypr.join(config);
        let mut env = HashMap::new();
        env.insert("HOME".into(), tmp.path().join("home").to_string_lossy().into());
        env.insert("HYPRLAND_MOD_DRY_RUN".into(), "1".into());
        env.insert("HYPRLAND_MOD_CONFIG".into(), config.to_string_lossy().into());
        env.insert("HYPRLAND_MOD_DATA".into(), data.to_string_lossy().into());
        let original_text = fs::read_to_string(&config).unwrap();
        TempConfig { managed: hypr.join(managed), tmp, hypr, data, config, original_text, env }
    }

    /// Simulate a running Hyprland (only ever with a fake runner!).
    pub fn live(&mut self) {
        self.env.insert("HYPRLAND_MOD_DRY_RUN".into(), "0".into());
        self.env.insert("HYPRLAND_INSTANCE_SIGNATURE".into(), "fake-test-instance".into());
    }

    pub fn make_with(&self, runner: Box<dyn Runner>) -> ConfigManager {
        let cm = ConfigManager::new(ManagerOptions {
            runner: Some(runner),
            env: Some(self.env.clone()),
            ..Default::default()
        })
        .unwrap();
        let real_hypr = dirs_real_hypr();
        for p in [cm.config_path(), cm.managed_path(), cm.data_dir()] {
            assert!(p.starts_with(self.tmp.path()), "{p:?} outside the temp dir");
            if let Some(real) = &real_hypr {
                assert!(!p.starts_with(real), "{p:?} is the real config dir");
            }
        }
        cm
    }

    pub fn make(&self) -> ConfigManager {
        self.make_with(Box::new(ForbiddenRunner))
    }

    pub fn read(&self, p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    pub fn config_text(&self) -> String {
        self.read(&self.config)
    }

    pub fn managed_text(&self) -> String {
        self.read(&self.managed)
    }

    pub fn source_lines(&self) -> Vec<String> {
        self.config_text()
            .lines()
            .filter(|l| l.starts_with("source") && l.contains("hyprland-mod.conf"))
            .map(str::to_string)
            .collect()
    }

    pub fn require_lines(&self) -> Vec<String> {
        self.config_text()
            .lines()
            .filter(|l| l.contains("hyprland-mod") && !l.starts_with("--"))
            .map(str::to_string)
            .collect()
    }
}

fn dirs_real_hypr() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/hypr"))
}

pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|d| d.join(name)).find(|p| p.is_file()))
}

pub fn lua_bin() -> Option<PathBuf> {
    which("lua").or_else(|| which("lua5.4")).or_else(|| which("luajit"))
}

/// Execute a Lua config against the fake ``hl`` table; returns its printed lines.
pub fn run_fake_hl(path: &Path) -> Vec<String> {
    let lua = lua_bin().expect("lua installed");
    let out = std::process::Command::new(lua).arg(fixtures().join("fake_hl.lua")).arg(path).output().unwrap();
    assert!(out.status.success(), "lua failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).lines().map(str::to_string).collect()
}

/// ``luac -p`` succeeds on ``path``.
pub fn assert_luac_ok(path: &Path) {
    if let Some(luac) = which("luac") {
        let out = std::process::Command::new(luac).arg("-p").arg(path).output().unwrap();
        assert!(out.status.success(), "luac: {}", String::from_utf8_lossy(&out.stderr));
    }
}

pub fn s(v: &str) -> String {
    v.to_string()
}

pub fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}
