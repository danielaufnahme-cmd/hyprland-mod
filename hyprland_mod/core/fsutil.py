"""Safe file helpers: atomic writes that follow symlinks and keep permissions."""

from __future__ import annotations

import os
import tempfile
from pathlib import Path

from .parser import ENCODING, ERRORS


def read_text(path: str | os.PathLike[str]) -> str:
    return Path(path).read_text(encoding=ENCODING, errors=ERRORS)


def atomic_write(path: str | os.PathLike[str], text: str) -> None:
    """Write via temp file + fsync + rename in the target's real directory.

    Symlinks (e.g. dotfile managers) are resolved so the link itself survives.
    """
    target = os.path.realpath(os.fspath(path))
    directory = os.path.dirname(target)
    os.makedirs(directory, exist_ok=True)
    mode = None
    try:
        mode = os.stat(target).st_mode & 0o7777
    except FileNotFoundError:
        pass
    fd, tmp = tempfile.mkstemp(prefix=".hyprland-mod-", suffix=".tmp", dir=directory)
    try:
        with os.fdopen(fd, "w", encoding=ENCODING, errors=ERRORS, newline="") as fh:
            fh.write(text)
            fh.flush()
            os.fsync(fh.fileno())
        if mode is not None:
            os.chmod(tmp, mode)
        os.replace(tmp, target)
    except BaseException:
        try:
            os.unlink(tmp)
        except FileNotFoundError:
            pass
        raise
