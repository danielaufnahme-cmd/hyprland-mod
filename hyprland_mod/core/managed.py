"""The managed settings file (``hyprland-mod.conf``) and the main config's source line."""

from __future__ import annotations

import os
import re
from dataclasses import dataclass, field

from .parser import ParsedConfig, escape_value, expand_path, parse_config, strip_comment

HEADER = (
    "# Hyprland Mod managed settings. Generated file: manual edits may be overwritten.\n"
    "# It is sourced at the end of hyprland.conf so these values take precedence.\n"
)
SOURCE_MARKER = "# Hyprland Mod: keep this line last so its settings take precedence"

# Keywords that may legitimately repeat; stored as ordered lists, not single values.
LIST_KEYWORDS = {
    "bezier", "animation", "monitor", "workspace", "windowrule", "windowrulev2",
    "layerrule", "exec", "exec-once", "exec-shutdown", "env", "submap", "gesture",
    "permission", "plugin",
}
LIST_ORDER = ("bezier", "animation")  # beziers must be defined before animations use them

KEY_RE = re.compile(r"^[A-Za-z0-9_.\-]+(?::[A-Za-z0-9_.\-]+)*$")


def is_list_keyword(key: str) -> bool:
    return key in LIST_KEYWORDS or key.startswith("bind")


@dataclass
class ManagedState:
    settings: dict[str, str] = field(default_factory=dict)
    lists: dict[str, list[str]] = field(default_factory=dict)

    def to_dict(self) -> dict:
        return {"version": 1, "settings": dict(self.settings),
                "lists": {k: list(v) for k, v in self.lists.items()}}

    @classmethod
    def from_dict(cls, data: dict) -> "ManagedState":
        if not isinstance(data, dict):
            raise ValueError("state must be a dict")
        settings = data.get("settings") or {}
        lists = data.get("lists") or {}
        if not isinstance(settings, dict) or not isinstance(lists, dict):
            raise ValueError("settings and lists must be dicts")
        return cls({str(k): str(v) for k, v in settings.items()},
                   {str(k): [str(x) for x in v] for k, v in lists.items() if isinstance(v, list)})

    def copy(self) -> "ManagedState":
        return ManagedState.from_dict(self.to_dict())


def state_from_parsed(parsed: ParsedConfig) -> ManagedState:
    state = ManagedState()
    counts: dict[str, int] = {}
    for e in parsed.entries:
        counts[e.key] = counts.get(e.key, 0) + 1
    for e in parsed.entries:
        if is_list_keyword(e.key) or counts[e.key] > 1:
            state.lists.setdefault(e.key, []).append(e.value)
        else:
            state.settings[e.key] = e.value
    return state


def read_managed(path: str) -> ManagedState:
    if not os.path.isfile(path):
        return ManagedState()
    return state_from_parsed(parse_config(path))


def validate_key(key: str) -> None:
    if not isinstance(key, str) or not KEY_RE.match(key):
        raise ValueError(f"invalid setting key: {key!r}")
    if key.split(":")[0] == "device" or key == "source" or key.startswith("$"):
        raise ValueError(f"unsupported setting key: {key!r}")


def validate_value(value: str) -> None:
    if not isinstance(value, str):
        raise ValueError("value must be a string")
    if "\n" in value or "\r" in value:
        raise ValueError("value must be a single line")
    if "{" in value or "}" in value:
        raise ValueError("value must not contain braces")


def render(state: ManagedState) -> str:
    """Render as nested sections (``decoration { blur { enabled = true } }``)."""
    tree: dict = {}
    for key in sorted(state.settings):
        node = tree
        *sections, leaf = key.split(":")
        for s in sections:
            node = node.setdefault(("s", s), {})
        node[("v", leaf)] = state.settings[key]

    out: list[str] = [HEADER]

    def emit(node: dict, depth: int) -> None:
        pad = "    " * depth
        for (kind, name), val in node.items():
            if kind == "v":
                out.append(f"{pad}{name} = {escape_value(val)}\n")
        for (kind, name), val in node.items():
            if kind == "s":
                out.append(f"{pad}{name} {{\n")
                emit(val, depth + 1)
                out.append(f"{pad}}}\n")

    if tree:
        out.append("\n")
        emit(tree, 0)
    ordered = [k for k in LIST_ORDER if k in state.lists] + \
              [k for k in state.lists if k not in LIST_ORDER]
    for kw in ordered:
        if state.lists[kw]:
            out.append("\n")
            out.extend(f"{kw} = {escape_value(v)}\n" for v in state.lists[kw])
    return "".join(out)


# ---------------------------------------------------------------- source line

def source_path_text(managed_path: str) -> str:
    """Path as written in the source line (``~/`` form when under $HOME)."""
    path = os.path.abspath(managed_path)
    home = os.path.expanduser("~")
    if home not in ("", "/") and path.startswith(home.rstrip("/") + "/"):
        return "~/" + os.path.relpath(path, home)
    return path


def _points_to(line: str, managed_path: str, config_dir: str) -> bool:
    m = re.match(r"^source\s*=\s*(.+)$", strip_comment(line))
    if not m:
        return False
    target = expand_path(m.group(1), config_dir)
    return os.path.realpath(target) == os.path.realpath(managed_path)


def with_source_line(text: str, config_path: str, managed_path: str) -> str | None:
    """Return the config text with exactly one source line at the end, or None if already so."""
    config_dir = os.path.dirname(os.path.abspath(config_path))
    lines = text.splitlines(keepends=True)
    ours = [i for i, ln in enumerate(lines) if _points_to(ln, managed_path, config_dir)]
    meaningful = [i for i, ln in enumerate(lines) if strip_comment(ln)]
    if len(ours) == 1 and meaningful and ours[0] == meaningful[-1]:
        return None
    drop = set(ours)
    for i in ours:  # also drop our own marker comment right above it
        if i > 0 and lines[i - 1].strip() == SOURCE_MARKER:
            drop.add(i - 1)
    kept = [ln for i, ln in enumerate(lines) if i not in drop]
    body = "".join(kept)
    if body and not body.endswith("\n"):
        body += "\n"
    if body and not body.endswith("\n\n"):
        body += "\n"
    return f"{body}{SOURCE_MARKER}\nsource = {source_path_text(managed_path)}\n"
