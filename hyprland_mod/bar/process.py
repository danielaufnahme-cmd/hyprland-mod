"""Starting and stopping the Waybar instance this app owns, and spotting other bars.

Only the process recorded in our PID file is ever stopped, and only after
/proc confirms it is still a waybar running *our* config (PIDs get reused).
Launching, killing and /proc are injectable so tests never touch real processes.
"""

from __future__ import annotations

import os
import shlex
import shutil
import signal
import subprocess
from pathlib import Path

from .generate import paths

INSTALL_HINTS = [
    ("Arch, CachyOS, EndeavourOS, Manjaro", "sudo pacman -S waybar"),
    ("Fedora", "sudo dnf install waybar"),
    ("Debian, Ubuntu, Pop!_OS", "sudo apt install waybar"),
    ("openSUSE", "sudo zypper install waybar"),
]

# process name -> how we call it in the warning
BAR_PROCESSES = {
    "waybar": "Waybar",
    "ags": "AGS",
    "agsv1": "AGS",
    "astal": "Astal",
    "eww": "Eww",
    "hyprpanel": "HyprPanel",
    "ironbar": "Ironbar",
    "nwg-panel": "nwg-panel",
    "yambar": "Yambar",
    "sfwbar": "SFWBar",
    "gBar": "gBar",
}
QUICKSHELL = ("quickshell", "qs")
# Quickshell configs that are known to draw a bar ("caelestia", "noctalia", "dms"...).
QUICKSHELL_BARS = {"caelestia": "Caelestia", "noctalia": "Noctalia", "noctalia-shell": "Noctalia",
                   "dms": "DankMaterialShell", "dankmaterialshell": "DankMaterialShell",
                   "ii": "illogical-impulse", "illogical-impulse": "illogical-impulse",
                   "end-4": "illogical-impulse", "hyprpanel": "HyprPanel"}


class WaybarMissing(RuntimeError):
    """Waybar isn't installed; the UI shows install commands instead."""


def is_dry_run() -> bool:
    return os.environ.get("HYPRLAND_MOD_DRY_RUN", "").strip().lower() in ("1", "true", "yes", "on")


def command(folder=None, binary: str = "waybar") -> list[str]:
    p = paths(folder)
    return [binary, "-c", str(p["config"]), "-s", str(p["style"])]


def autostart_command(folder=None) -> str:
    """Shell line for Hyprland's ``exec-once`` that starts this bar on login."""
    return " ".join(shlex.quote(part) for part in command(folder))


def _default_launcher(cmd: list[str], log_path: Path) -> int:
    log_path.parent.mkdir(parents=True, exist_ok=True)
    with open(log_path, "ab") as log:
        proc = subprocess.Popen(cmd, stdin=subprocess.DEVNULL, stdout=log, stderr=subprocess.STDOUT,
                                start_new_session=True, close_fds=True)
    return proc.pid


def _read(path: Path) -> str:
    try:
        return path.read_bytes().replace(b"\0", b" ").decode("utf-8", "replace").strip()
    except OSError:
        return ""


class BarProcess:
    """Owns at most one waybar process, remembered across app restarts by a PID file."""

    def __init__(self, state_dir, folder=None, launcher=None, killer=None, which=None, proc_root="/proc"):
        self.state_dir = Path(state_dir)
        self.folder = folder
        self._launcher = launcher
        self._killer = killer or os.kill
        self._which = which or shutil.which
        self.proc_root = Path(proc_root)

    @property
    def pid_path(self) -> Path:
        return self.state_dir / "waybar.pid"

    @property
    def log_path(self) -> Path:
        return self.state_dir / "waybar.log"

    def installed(self) -> bool:
        return bool(self._which("waybar"))

    def _cmdline(self, pid: int) -> str:
        return _read(self.proc_root / str(pid) / "cmdline")

    def _is_ours(self, pid: int) -> bool:
        cmd = self._cmdline(pid)
        return "waybar" in cmd and str(paths(self.folder)["config"]) in cmd

    def running_pid(self) -> int | None:
        try:
            pid = int(self.pid_path.read_text().split()[0])
        except (OSError, ValueError, IndexError):
            return None
        return pid if pid > 0 and self._is_ours(pid) else None

    def is_running(self) -> bool:
        return self.running_pid() is not None

    def start(self) -> int | None:
        """(Re)start our bar. Returns the PID, or None in dry-run mode with the real launcher."""
        binary = self._which("waybar")
        if not binary:
            raise WaybarMissing("Waybar isn't installed.")
        self.stop()
        if self._launcher is None and is_dry_run():
            return None
        launcher = self._launcher or _default_launcher
        pid = int(launcher(command(self.folder, binary), self.log_path))
        self.state_dir.mkdir(parents=True, exist_ok=True)
        self.pid_path.write_text(f"{pid}\n")
        return pid

    def stop(self) -> bool:
        """Stop the bar we started; never anything else. True if one was stopped."""
        pid = self.running_pid()
        stopped = False
        if pid is not None and not (self._killer is os.kill and is_dry_run()):
            try:
                self._killer(pid, signal.SIGTERM)
                stopped = True
            except ProcessLookupError:
                pass
        try:
            self.pid_path.unlink()
        except FileNotFoundError:
            pass
        return stopped

    def other_bars(self) -> list[str]:
        """Names of bars running that aren't ours (Waybar, Caelestia, AGS, Eww...)."""
        own = self.running_pid()
        return other_bars(self.proc_root, ignore={own} if own else set())


def other_bars(proc_root="/proc", ignore=frozenset()) -> list[str]:
    found = []
    root = Path(proc_root)
    try:
        entries = [e for e in os.listdir(root) if e.isdigit()]
    except OSError:
        return found
    for entry in entries:
        pid = int(entry)
        if pid in ignore or pid == os.getpid():
            continue
        comm = _read(root / entry / "comm")
        name = None
        if comm in BAR_PROCESSES:
            name = BAR_PROCESSES[comm]
        elif comm in QUICKSHELL:
            args = _read(root / entry / "cmdline").split()
            cfg = None
            for flag in ("-c", "--config"):
                if flag in args and args.index(flag) + 1 < len(args):
                    cfg = args[args.index(flag) + 1]
            if cfg is None:
                name = "Quickshell"
            elif cfg.lower() in QUICKSHELL_BARS:
                name = f"{QUICKSHELL_BARS[cfg.lower()]} (Quickshell)"
        if name and name not in found:
            found.append(name)
    return found
