"""Thin hyprctl wrapper. Never invoked in dry-run mode."""

from __future__ import annotations

import os
import subprocess
from collections.abc import Callable, Iterable

Runner = Callable[[list[str]], tuple[int, str, str]]


def is_dry_run() -> bool:
    """Dry-run when explicitly requested or when not inside a Hyprland session."""
    if os.environ.get("HYPRLAND_MOD_DRY_RUN", "").strip().lower() in ("1", "true", "yes", "on"):
        return True
    return not os.environ.get("HYPRLAND_INSTANCE_SIGNATURE")


def default_runner(args: list[str]) -> tuple[int, str, str]:
    """Run ``hyprctl <args>``; returns (returncode, stdout, stderr)."""
    try:
        proc = subprocess.run(["hyprctl", *args], capture_output=True, text=True, timeout=5)
    except FileNotFoundError:
        return 127, "", "hyprctl not found"
    except subprocess.TimeoutExpired:
        return 124, "", "hyprctl timed out"
    return proc.returncode, proc.stdout, proc.stderr


def build_batch(pairs: Iterable[tuple[str, str]]) -> str:
    """``[(k, v), ...]`` -> ``"keyword k v ; keyword k2 v2"``."""
    return " ; ".join(f"keyword {k} {v}" for k, v in pairs)


def batch_errors(returncode: int, stdout: str, stderr: str) -> list[str]:
    """Extract error messages from hyprctl output (each command answers ``ok``)."""
    errors = [ln.strip() for ln in stdout.splitlines() if ln.strip() and ln.strip().lower() != "ok"]
    if stderr.strip():
        errors.append(stderr.strip())
    if returncode != 0 and not errors:
        errors.append(f"hyprctl exited with code {returncode}")
    return errors
