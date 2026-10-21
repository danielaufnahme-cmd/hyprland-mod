"""Load settings pages from ``data/schema/*.json``."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

DEFAULT_SCHEMA_DIR = Path(__file__).resolve().parents[2] / "data" / "schema"


def _normalize_option(opt: dict) -> dict | None:
    if not isinstance(opt, dict) or not isinstance(opt.get("key"), str):
        return None
    out = dict(opt)
    out.setdefault("label", opt["key"].rsplit(":", 1)[-1].replace("_", " ").capitalize())
    out.setdefault("description", "")
    out.setdefault("type", "string")
    out.setdefault("default", None)
    out["advanced"] = bool(opt.get("advanced", False))
    if "choices" in out:
        out["choices"] = [c if isinstance(c, dict) else {"value": c, "label": str(c)}
                          for c in out["choices"] or []]
    return out


def _normalize_page(page: dict, fallback_id: str) -> dict | None:
    if not isinstance(page, dict):
        return None
    out = dict(page)
    out["id"] = str(page.get("id") or fallback_id)
    out.setdefault("title", out["id"].replace("_", " ").title())
    out.setdefault("icon", "preferences-system-symbolic")
    out.setdefault("description", "")
    try:
        out["order"] = float(page.get("order", 1000))
    except (TypeError, ValueError):
        out["order"] = 1000.0
    if out["order"].is_integer():
        out["order"] = int(out["order"])
    groups = []
    for group in page.get("groups") or []:
        if not isinstance(group, dict):
            continue
        g = dict(group)
        g.setdefault("title", "")
        g["options"] = [o for o in map(_normalize_option, group.get("options") or []) if o]
        groups.append(g)
    out["groups"] = groups
    return out


def load_schema(schema_dir: str | os.PathLike[str] | None = None) -> list[dict]:
    """Return all pages sorted by ``order`` (then title). Bad files are skipped."""
    folder = Path(schema_dir) if schema_dir is not None else DEFAULT_SCHEMA_DIR
    pages: list[dict] = []
    if not folder.is_dir():
        return pages
    for path in sorted(folder.glob("*.json")):
        try:
            data = json.loads(path.read_text(encoding="utf-8"))
        except (OSError, ValueError) as exc:
            print(f"hyprland-mod: skipping schema {path.name}: {exc}", file=sys.stderr)
            continue
        items = data if isinstance(data, list) else [data]
        for i, item in enumerate(items):
            page = _normalize_page(item, path.stem if len(items) == 1 else f"{path.stem}-{i}")
            if page:
                pages.append(page)
    pages.sort(key=lambda p: (p["order"], p["title"]))
    return pages
