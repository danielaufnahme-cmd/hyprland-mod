"""Read-only parser for Hyprland config files.

Produces a flat view of the config: colon-joined keys (``decoration:blur:enabled``),
every occurrence of repeated keywords, ``$variables`` and the files that were read.
It never writes anything.
"""

from __future__ import annotations

import glob
import os
import re
from dataclasses import dataclass, field
from pathlib import Path

ENCODING = "utf-8"
ERRORS = "surrogateescape"  # round-trips any bytes unchanged

# Sections whose contents are per-device and must not leak into global keys.
SKIP_SECTIONS = ("device",)
# Keywords that live inside `animations { }` but are global list keywords.
LIST_ALIASES = {"animations:bezier": "bezier", "animations:animation": "animation"}

_SECTION_RE = re.compile(r"^([A-Za-z0-9_.\-:]+(?:\[[^\]]*\])?)\s*\{")
_ASSIGN_RE = re.compile(r"^([^=]+?)\s*=\s*(.*)$")
_VAR_RE = re.compile(r"\$([A-Za-z0-9_]+)")


@dataclass
class Entry:
    key: str
    value: str
    file: str
    line: int


@dataclass
class ParsedConfig:
    entries: list[Entry] = field(default_factory=list)
    variables: dict[str, str] = field(default_factory=dict)
    files: list[str] = field(default_factory=list)
    warnings: list[str] = field(default_factory=list)

    def values(self) -> dict[str, str]:
        """Last value of every key (Hyprland semantics: later wins)."""
        out: dict[str, str] = {}
        for e in self.entries:
            out[e.key] = e.value
        return out

    def get(self, key: str) -> str | None:
        for e in reversed(self.entries):
            if e.key == key:
                return e.value
        return None

    def get_all(self, key: str) -> list[str]:
        return [e.value for e in self.entries if e.key == key]


def strip_comment(line: str) -> str:
    """Remove a trailing ``#`` comment; ``##`` is an escaped literal ``#``."""
    out: list[str] = []
    i = 0
    while i < len(line):
        ch = line[i]
        if ch == "#":
            if i + 1 < len(line) and line[i + 1] == "#":
                out.append("#")
                i += 2
                continue
            break
        out.append(ch)
        i += 1
    return "".join(out).strip()


def escape_value(value: str) -> str:
    """Inverse of :func:`strip_comment` for a value written to a config file."""
    return value.replace("#", "##")


def resolve_vars(text: str, variables: dict[str, str]) -> str:
    """Replace ``$name`` with known variable values (longest name wins)."""
    if "$" not in text or not variables:
        return text

    def sub(m: re.Match[str]) -> str:
        name = m.group(1)
        # Hyprland matches the longest defined variable that prefixes the token.
        for cut in range(len(name), 0, -1):
            if name[:cut] in variables:
                return variables[name[:cut]] + name[cut:]
        return m.group(0)

    return _VAR_RE.sub(sub, text)


def normalize_key(key: str) -> str:
    key = re.sub(r"\s+", "", key)
    return LIST_ALIASES.get(key, key)


def expand_path(raw: str, base_dir: str) -> str:
    path = os.path.expanduser(raw.strip())
    if not os.path.isabs(path):
        path = os.path.join(base_dir, path)
    return os.path.normpath(path)


class _Parser:
    def __init__(self, exclude: set[str], follow_sources: bool = True) -> None:
        self.result = ParsedConfig()
        self.exclude = exclude
        self.follow_sources = follow_sources
        self.visited: set[str] = set()

    def parse_file(self, path: str) -> None:
        real = os.path.realpath(path)
        if real in self.visited:
            self.result.warnings.append(f"skipped recursive source: {path}")
            return
        if real in self.exclude:
            return
        try:
            text = Path(path).read_text(encoding=ENCODING, errors=ERRORS)
        except OSError as exc:
            self.result.warnings.append(f"cannot read {path}: {exc.strerror or exc}")
            return
        self.visited.add(real)
        self.result.files.append(path)
        self.parse_text(text, path)

    def parse_text(self, text: str, path: str) -> None:
        stack: list[str] = []
        skip_depth = 0  # >0 while inside a skipped (device) block
        base_dir = os.path.dirname(os.path.abspath(path))
        for lineno, raw in enumerate(text.splitlines(), 1):
            line = strip_comment(raw)
            while line:
                if line.startswith("}"):
                    if skip_depth:
                        skip_depth -= 1
                    elif stack:
                        stack.pop()
                    else:
                        self.result.warnings.append(f"{path}:{lineno}: unmatched '}}'")
                    line = line[1:].strip()
                    continue
                m = _SECTION_RE.match(line)
                if m:
                    name = m.group(1)
                    if skip_depth or (not stack and name.split("[")[0].split(":")[0] in SKIP_SECTIONS):
                        skip_depth += 1
                    else:
                        stack.append(name)
                    line = line[m.end():].strip()
                    continue
                line = self._handle_statement(line, stack, skip_depth, base_dir, path, lineno)
        if stack or skip_depth:
            self.result.warnings.append(f"{path}: unclosed section(s): {', '.join(stack) or 'device'}")

    def _handle_statement(self, line: str, stack: list[str], skip_depth: int,
                          base_dir: str, path: str, lineno: int) -> str:
        """Consume one ``key = value`` statement; returns any trailing text (a '}')."""
        rest = ""
        if line.endswith("}") and "{" not in line:
            rest = "}"
            line = line[:-1].rstrip()
        if skip_depth:
            return rest
        m = _ASSIGN_RE.match(line)
        if not m:
            self.result.warnings.append(f"{path}:{lineno}: unrecognized line kept as-is")
            return rest
        key, value = m.group(1).strip(), m.group(2).strip()
        if key.startswith("$") and not stack:
            self.result.variables[key[1:]] = resolve_vars(value, self.result.variables)
            return rest
        value = resolve_vars(value, self.result.variables)
        full = normalize_key(":".join([*stack, key]))
        if full.split(":")[0] in SKIP_SECTIONS:
            return rest
        if full == "source":
            if not self.follow_sources:
                self.result.entries.append(Entry(full, value, path, lineno))
                return rest
            for src in sorted(glob.glob(expand_path(value, base_dir))) or [expand_path(value, base_dir)]:
                self.parse_file(src)
            return rest
        self.result.entries.append(Entry(full, value, path, lineno))
        return rest


def parse_config(path: str | os.PathLike[str], exclude: list[str] | None = None) -> ParsedConfig:
    """Parse ``path`` following ``source =`` includes (files in ``exclude`` are skipped)."""
    parser = _Parser({os.path.realpath(p) for p in (exclude or [])})
    parser.parse_file(os.fspath(path))
    return parser.result


def parse_text(text: str, path: str = "<string>") -> ParsedConfig:
    """Parse config text without following includes."""
    parser = _Parser(set(), follow_sources=False)
    parser.parse_text(text, path)
    return parser.result
