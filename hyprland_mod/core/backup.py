"""Timestamped file backups stored under ``<data_dir>/backups/<id>/``."""

from __future__ import annotations

import json
import os
import re
import shutil
from datetime import datetime
from pathlib import Path

from .fsutil import atomic_write, read_text

_ID_RE = re.compile(r"^[0-9]{8}-[0-9]{6}-[0-9]{6}(?:-[0-9]+)?$")


class BackupStore:
    def __init__(self, data_dir: str | os.PathLike[str]) -> None:
        self.root = Path(data_dir) / "backups"

    def create(self, label: str, files: list[str]) -> str:
        """Copy every existing file in ``files``; returns the new backup id."""
        now = datetime.now()
        base = now.strftime("%Y%m%d-%H%M%S-%f")
        backup_id, n = base, 1
        while (self.root / backup_id).exists():
            backup_id, n = f"{base}-{n}", n + 1
        folder = self.root / backup_id
        (folder / "files").mkdir(parents=True)
        saved: list[dict[str, str]] = []
        for i, path in enumerate(dict.fromkeys(os.path.abspath(f) for f in files)):
            if not os.path.isfile(path):
                continue
            name = f"{i:03d}-{os.path.basename(path)}"
            shutil.copy2(os.path.realpath(path), folder / "files" / name)
            saved.append({"path": path, "copy": name})
        manifest = {
            "id": backup_id,
            "created": now.isoformat(timespec="seconds"),
            "label": label,
            "files": saved,
        }
        (folder / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
        return backup_id

    def list(self) -> list[dict]:
        """Newest first: ``{id, created, label, files}`` (files = original paths)."""
        out: list[dict] = []
        if not self.root.is_dir():
            return out
        for folder in self.root.iterdir():
            manifest = self._manifest(folder.name)
            if manifest:
                out.append({**manifest, "files": [f["path"] for f in manifest["files"]]})
        out.sort(key=lambda m: m["id"], reverse=True)
        return out

    def restore(self, backup_id: str) -> list[str]:
        """Write every backed-up file back to its original path; returns those paths."""
        manifest = self._manifest(backup_id)
        if manifest is None:
            raise KeyError(f"unknown backup: {backup_id}")
        folder = self.root / backup_id / "files"
        restored: list[str] = []
        for item in manifest["files"]:
            atomic_write(item["path"], read_text(folder / item["copy"]))
            restored.append(item["path"])
        return restored

    def _manifest(self, backup_id: str) -> dict | None:
        if not _ID_RE.match(backup_id):
            return None
        try:
            data = json.loads((self.root / backup_id / "manifest.json").read_text(encoding="utf-8"))
        except (OSError, ValueError):
            return None
        if not isinstance(data, dict) or not isinstance(data.get("files"), list):
            return None
        files = [f for f in data["files"]
                 if isinstance(f, dict) and isinstance(f.get("path"), str)
                 and isinstance(f.get("copy"), str) and "/" not in f["copy"]]
        return {"id": backup_id, "created": str(data.get("created", "")),
                "label": str(data.get("label", "")), "files": files}
