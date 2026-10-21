"""Shared test setup: every test works on a temp copy of the fixtures, never real files."""

from __future__ import annotations

import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

FIXTURES = Path(__file__).resolve().parent / "fixtures"
REAL_HYPR_DIR = os.path.realpath(os.path.expanduser("~/.config/hypr"))


class FakeRunner:
    """Records hyprctl invocations instead of running anything."""

    def __init__(self, reply=(0, "ok", "")) -> None:
        """``reply`` is a (code, stdout, stderr) tuple or a callable taking the args."""
        self.calls: list[list[str]] = []
        self.reply = reply

    def __call__(self, args: list[str]) -> tuple[int, str, str]:
        self.calls.append(list(args))
        return self.reply(args) if callable(self.reply) else self.reply


def forbidden_runner(args: list[str]) -> tuple[int, str, str]:
    raise AssertionError(f"hyprctl must not run in dry-run mode: {args}")


class TempConfigTest(unittest.TestCase):
    """Copies the fixture config to a temp dir and points the env at it (dry-run)."""

    def setUp(self) -> None:
        self.tmp = Path(tempfile.mkdtemp(prefix="hyprland-mod-test-"))
        self.hypr = self.tmp / "hypr"
        self.data = self.tmp / "data"
        self.hypr.mkdir()
        shutil.copy(FIXTURES / "sample_hyprland.conf", self.hypr / "hyprland.conf")
        shutil.copy(FIXTURES / "extra.conf", self.hypr / "extra.conf")
        self.config = self.hypr / "hyprland.conf"
        self.managed = self.hypr / "hyprland-mod.conf"
        env = mock.patch.dict(os.environ, {
            "HYPRLAND_MOD_DRY_RUN": "1",
            "HYPRLAND_MOD_CONFIG": str(self.config),
            "HYPRLAND_MOD_DATA": str(self.data),
        })
        env.start()
        self.addCleanup(env.stop)
        self.addCleanup(shutil.rmtree, self.tmp, ignore_errors=True)
        self.original_text = self.config.read_text()

    def make(self, runner=forbidden_runner):
        from hyprland_mod.core import ConfigManager
        cm = ConfigManager(runner=runner)
        for path in (cm.config_path, cm.managed_path, cm.data_dir):
            self.assertFalse(os.path.realpath(path).startswith(REAL_HYPR_DIR), path)
            self.assertTrue(path.startswith(str(self.tmp)), path)
        return cm

    def live_env(self):
        """Context manager simulating a running Hyprland (only with a FakeRunner!)."""
        return mock.patch.dict(os.environ, {
            "HYPRLAND_MOD_DRY_RUN": "0", "HYPRLAND_INSTANCE_SIGNATURE": "fake-test-instance"})

    def source_lines(self) -> list[str]:
        return [ln for ln in self.config.read_text().splitlines() if ln.startswith("source")
                and "hyprland-mod.conf" in ln]
