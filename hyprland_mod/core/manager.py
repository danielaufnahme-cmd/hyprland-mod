"""ConfigManager: read, stage, write, live-apply, back up and undo Hyprland settings."""

from __future__ import annotations

import json
import os
import sys
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from . import hyprctl, lua
from .backup import BackupStore
from .fsutil import atomic_write, read_text
from .managed import (
    LIST_ORDER, ManagedState, read_managed, render, validate_key, validate_value,
    with_source_line,
)
from .parser import ParsedConfig, normalize_key, parse_config

HISTORY_LIMIT = 50
Listener = Callable[[str, "str | None"], None]
DEFAULT_CONFIG_DIR = "~/.config/hypr"


def detect_config(explicit: str | None) -> tuple[str, str]:
    """Return (config path, mode). A ``hyprland.lua`` next to the config means Lua mode."""
    if explicit:
        path = os.path.abspath(os.path.expanduser(explicit))
    else:
        path = os.path.abspath(os.path.expanduser(os.path.join(DEFAULT_CONFIG_DIR, "hyprland.conf")))
    sibling = os.path.join(os.path.dirname(path), "hyprland.lua")
    if not path.endswith(".lua") and os.path.isfile(sibling):
        path = sibling
    return path, "lua" if path.endswith(".lua") else "conf"


class LuaUserView:
    """User values in Lua mode: live ``hyprctl -j descriptions`` (cached) plus scanned curves."""

    def __init__(self, lists: dict[str, list[str]], fetch: Callable[[], dict[str, str]] | None) -> None:
        self.lists = lists
        self.warnings: list[str] = []
        self._fetch = fetch
        self._values: dict[str, str] | None = None

    def get(self, key: str) -> str | None:
        if self._values is None:
            self._values = self._fetch() if self._fetch else {}
        return self._values.get(lua.dotted(key))

    def get_all(self, keyword: str) -> list[str]:
        return list(self.lists.get(keyword, []))


@dataclass
class ApplyResult:
    ok: bool
    errors: list[str] = field(default_factory=list)
    live_applied: bool = False


class ConfigManager:
    def __init__(self, config_path: str | None = None, managed_path: str | None = None,
                 data_dir: str | None = None, runner: hyprctl.Runner | None = None) -> None:
        self.config_path, self.mode = detect_config(config_path or os.environ.get("HYPRLAND_MOD_CONFIG"))
        ext = ".lua" if self.mode == "lua" else ".conf"
        self.managed_path = os.path.abspath(os.path.expanduser(
            managed_path or os.path.join(os.path.dirname(self.config_path), "hyprland-mod" + ext)))
        self.data_dir = os.path.abspath(os.path.expanduser(
            data_dir or os.environ.get("HYPRLAND_MOD_DATA") or "~/.local/share/hyprland-mod"))
        self.runner: hyprctl.Runner = runner or hyprctl.default_runner
        self.dry_run = hyprctl.is_dry_run()
        self.backups = BackupStore(self.data_dir)
        self._listeners: list[Listener] = []
        self._pending_set: dict[str, str] = {}
        self._pending_reset: set[str] = set()
        self._pending_lists: dict[str, list[str] | None] = {}
        self._user: ParsedConfig | LuaUserView = ParsedConfig()
        self._managed = ManagedState()
        self._lua_types: dict[str, str] | None = None
        self.reload()
        self._history, self._index = self._load_history()

    # ------------------------------------------------------------ reading

    def reload(self) -> None:
        """Re-read the user's config and the managed file from disk."""
        if self.mode == "lua":
            files = lua.required_files(self.config_path, exclude=[self.managed_path])
            fetch = None if self.dry_run else self._fetch_descriptions
            self._user = LuaUserView(lua.scan_lists(files), fetch)
            self._managed = lua.read_lua_state(self.managed_path)
        else:
            self._user = parse_config(self.config_path, exclude=[self.managed_path])
            self._managed = read_managed(self.managed_path)

    def _fetch_descriptions(self) -> dict[str, str]:
        """Current option values from the running Hyprland (one read-only call)."""
        try:
            code, out, err = self.runner(["-j", "descriptions"])
            data = json.loads(out) if code == 0 else []
        except Exception:  # unavailable: fall back to managed file / defaults
            return {}
        values: dict[str, str] = {}
        for item in data if isinstance(data, list) else []:
            if not isinstance(item, dict):
                continue
            name = item.get("value") or item.get("name") or item.get("key")
            payload = item.get("data") if isinstance(item.get("data"), dict) else item
            if isinstance(name, str) and "current" in payload:
                text = lua.describe_value(name, payload["current"])
                if text is not None:
                    values[lua.dotted(name)] = text
        return values

    @property
    def lua_types(self) -> dict[str, str]:
        if self._lua_types is None:
            self._lua_types = lua.load_lua_types(os.environ.get("HYPRLAND_MOD_LUA_STUBS"))
        return self._lua_types

    @property
    def warnings(self) -> list[str]:
        return list(self._user.warnings)

    @property
    def hyprland_running(self) -> bool:
        return bool(os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"))

    def get(self, key: str) -> str | None:
        key = normalize_key(key)
        if key in self._pending_set:
            return self._pending_set[key]
        if key not in self._pending_reset and key in self._managed.settings:
            return self._managed.settings[key]
        return self._user.get(key)

    def user_value(self, key: str) -> str | None:
        """Value from the user's own config only (ignores Hyprland Mod)."""
        return self._user.get(normalize_key(key))

    def is_modified(self, key: str) -> bool:
        key = normalize_key(key)
        if key in self._pending_set:
            return True
        return key not in self._pending_reset and key in self._managed.settings

    def get_list(self, keyword: str) -> list[str]:
        kw = normalize_key(keyword)
        if kw in self._pending_lists:
            staged = self._pending_lists[kw]
            return list(staged) if staged is not None else self._user.get_all(kw)
        if kw in self._managed.lists:
            return list(self._managed.lists[kw])
        return self._user.get_all(kw)

    def is_list_modified(self, keyword: str) -> bool:
        kw = normalize_key(keyword)
        if kw in self._pending_lists:
            return self._pending_lists[kw] is not None
        return kw in self._managed.lists

    # ------------------------------------------------------------ staging

    def set(self, key: str, value: str) -> None:
        validate_key(key)
        key = normalize_key(key)
        value = str(value).strip()
        if value == "":  # empty = unset/automatic: fall back to the user's value or default
            self.reset(key)
            return
        validate_value(value)
        self._pending_reset.discard(key)
        if self._managed.settings.get(key) == value:
            self._pending_set.pop(key, None)
        else:
            self._pending_set[key] = value
        self._emit("changed", key)

    def reset(self, key: str) -> None:
        key = normalize_key(key)
        self._pending_set.pop(key, None)
        if key in self._managed.settings:
            self._pending_reset.add(key)
        self._emit("changed", key)

    def set_list(self, keyword: str, values: list[str]) -> None:
        validate_key(keyword)
        kw = normalize_key(keyword)
        clean = [str(v).strip() for v in values]
        for v in clean:
            validate_value(v)
        if self.mode == "lua":
            lua.list_calls(kw, clean)  # raises ValueError for unsupported/invalid entries
        if self._managed.lists.get(kw) == clean:
            self._pending_lists.pop(kw, None)
        else:
            self._pending_lists[kw] = clean
        self._emit("changed", kw)

    def reset_list(self, keyword: str) -> None:
        kw = normalize_key(keyword)
        if kw in self._managed.lists:
            self._pending_lists[kw] = None
        else:
            self._pending_lists.pop(kw, None)
        self._emit("changed", kw)

    def pending(self) -> dict[str, str]:
        return dict(self._pending_set)

    def pending_resets(self) -> list[str]:
        return sorted(self._pending_reset)

    @property
    def is_dirty(self) -> bool:
        return bool(self._pending_set or self._pending_reset or self._pending_lists)

    def discard(self) -> None:
        self._clear_pending()
        self._emit("discarded", None)

    def _clear_pending(self) -> None:
        self._pending_set.clear()
        self._pending_reset.clear()
        self._pending_lists.clear()

    def _staged_state(self) -> ManagedState:
        state = self._managed.copy()
        for key in self._pending_reset:
            state.settings.pop(key, None)
        state.settings.update(self._pending_set)
        for kw, values in self._pending_lists.items():
            if values is None:
                state.lists.pop(kw, None)
            else:
                state.lists[kw] = list(values)
        return state

    # ------------------------------------------------------------ applying

    def apply(self) -> ApplyResult:
        """Write staged changes to the managed file and apply them live."""
        if not self.is_dirty:
            return ApplyResult(True, [], False)
        result, written = self._commit(self._staged_state(), label="Before applying changes")
        if written:
            self._push_history(self._managed)
            self._emit("applied", None)
        return result

    def _commit(self, new: ManagedState, label: str) -> tuple[ApplyResult, bool]:
        """Back up if needed, write ``new``, ensure the source line, live-apply.

        Returns the result and whether the managed file was written.
        """
        if not os.path.isfile(self.config_path):
            return ApplyResult(False, [f"Hyprland config not found: {self.config_path}"]), False
        old = self._managed
        try:
            first = self._ensure_initial_backup()
            text = lua.render_lua(new, self.lua_types) if self.mode == "lua" else render(new)
            atomic_write(self.managed_path, text)
        except OSError as exc:
            return ApplyResult(False, [f"Could not write settings: {exc}"]), False
        errors: list[str] = []
        try:
            self._ensure_source_line(backup_first=not first, label=label)
        except OSError as exc:
            errors.append(f"Could not add source line to {self.config_path}: {exc}")
        self._clear_pending()
        self.reload()
        live, live_errors = self._live_apply(old, self._managed)
        errors += live_errors
        return ApplyResult(not errors, errors, live), True

    def _ensure_initial_backup(self) -> bool:
        """Back up the user's config the very first time anything is written."""
        state = self._read_state()
        if state.get("initial_backup"):
            return False
        backup_id = self.create_backup("Original config (before Hyprland Mod)")
        state["initial_backup"] = backup_id
        self._write_state(state)
        return True

    def _ensure_source_line(self, backup_first: bool, label: str) -> None:
        text = read_text(self.config_path)
        add_line = lua.with_require_line if self.mode == "lua" else with_source_line
        new_text = add_line(text, self.config_path, self.managed_path)
        if new_text is None:
            return
        if backup_first:
            self.create_backup(f"{label} (adding source line)")
        atomic_write(self.config_path, new_text)

    def _live_apply(self, old: ManagedState, new: ManagedState) -> tuple[bool, list[str]]:
        if self.dry_run:
            return False, []
        if self.mode == "lua":
            return self._live_apply_lua(old, new)
        commands: list[tuple[str, str]] = []
        need_reload = False
        for key, value in sorted(new.settings.items()):
            if old.settings.get(key) != value:
                commands.append((key, value))
        for key in sorted(old.settings.keys() - new.settings.keys()):
            fallback = self._user.get(key)
            if fallback is None:
                need_reload = True  # default unknown here; let Hyprland re-read files
            else:
                commands.append((key, fallback))
        keywords = [k for k in LIST_ORDER if k in old.lists or k in new.lists]
        keywords += sorted((old.lists.keys() | new.lists.keys()) - set(keywords))
        for kw in keywords:
            before = old.lists.get(kw, self._user.get_all(kw))
            after = new.lists.get(kw, self._user.get_all(kw))
            if before == after:
                continue
            if any(v not in after for v in before):
                need_reload = True
            commands.extend((kw, v) for v in after if v not in before)
        if any(";" in v for _, v in commands):
            need_reload = True
        if need_reload:
            args = ["reload"]
        elif commands:
            args = ["--batch", hyprctl.build_batch(commands)]
        else:
            return True, []
        return self._run(args)

    def _live_apply_lua(self, old: ManagedState, new: ManagedState) -> tuple[bool, list[str]]:
        """One ``hyprctl eval`` with the changed keys; removals need a full reload."""
        changed = {k: v for k, v in new.settings.items() if old.settings.get(k) != v}
        need_reload = bool(old.settings.keys() - new.settings.keys())
        added: dict[str, list[str]] = {}
        for kw in lua.LUA_LIST_KEYWORDS:
            before = old.lists.get(kw, self._user.get_all(kw))
            after = new.lists.get(kw, self._user.get_all(kw))
            if any(v not in after for v in before):
                need_reload = True
            added[kw] = [v for v in after if v not in before]
        if need_reload:
            return self._run(["reload"])
        try:
            code = lua.eval_code(changed, added, self.lua_types)
        except ValueError as exc:
            return False, [str(exc)]
        if not code:
            return True, []
        return self._run(["eval", code])

    def _run(self, args: list[str]) -> tuple[bool, list[str]]:
        try:
            code, out, err = self.runner(args)
        except Exception as exc:  # a broken runner must never break saving
            return False, [f"hyprctl failed: {exc}"]
        errors = hyprctl.batch_errors(code, out, err)
        return not errors, errors

    # ------------------------------------------------------------ undo / redo

    @property
    def can_undo(self) -> bool:
        return self._index > 0

    @property
    def can_redo(self) -> bool:
        return self._index < len(self._history) - 1

    def undo(self) -> ApplyResult:
        return self._step(-1, "undo")

    def redo(self) -> ApplyResult:
        return self._step(1, "redo")

    def _step(self, delta: int, event: str) -> ApplyResult:
        target = self._index + delta
        if not 0 <= target < len(self._history):
            return ApplyResult(False, [f"Nothing to {event}"])
        result, written = self._commit(ManagedState.from_dict(self._history[target]),
                                       label=f"Before {event}")
        if written:
            self._index = target
            self._save_history()
            self._emit(event, None)
        return result

    def _push_history(self, state: ManagedState) -> None:
        snap = state.to_dict()
        if self._history and self._history[self._index] == snap:
            return
        self._history = self._history[: self._index + 1] + [snap]
        self._history = self._history[-HISTORY_LIMIT:]
        self._index = len(self._history) - 1
        self._save_history()

    def _load_history(self) -> tuple[list[dict], int]:
        current = self._managed.to_dict()
        try:
            data = json.loads(Path(self.data_dir, "history.json").read_text(encoding="utf-8"))
            states, index = list(data["states"]), int(data["index"])
            if 0 <= index < len(states) and states[index] == current:
                return states, index
        except (OSError, ValueError, KeyError, TypeError):
            pass
        return [current], 0

    def _save_history(self) -> None:
        self._write_json("history.json", {"states": self._history, "index": self._index})

    # ------------------------------------------------------------ backups

    def _backup_files(self) -> list[str]:
        return [self.config_path, self.managed_path]

    def list_backups(self) -> list[dict]:
        return self.backups.list()

    def create_backup(self, label: str) -> str:
        return self.backups.create(label, self._backup_files())

    def restore_backup(self, backup_id: str) -> ApplyResult:
        """Restore files from a backup (after saving a safety backup of the current state)."""
        known = {b["id"]: b for b in self.list_backups()}
        if backup_id not in known:
            return ApplyResult(False, [f"Unknown backup: {backup_id}"])
        try:
            self.create_backup(f"Before restoring \"{known[backup_id]['label']}\"")
            self.backups.restore(backup_id)
            if self.managed_path not in known[backup_id]["files"]:
                # The managed file did not exist then: restore "no Hyprland Mod settings".
                atomic_write(self.managed_path, render(ManagedState()))
        except OSError as exc:
            return ApplyResult(False, [f"Could not restore backup: {exc}"])
        self._clear_pending()
        self.reload()
        self._push_history(self._managed)
        live, errors = (False, []) if self.dry_run else self._run(["reload"])
        self._emit("restored", None)
        return ApplyResult(not errors, errors, live)

    # ------------------------------------------------------------ export / import

    def export_state(self) -> dict:
        """Managed setup including staged changes, as plain JSON-able data."""
        return self._staged_state().to_dict()

    def import_state(self, data: dict) -> None:
        """Stage ``data`` (from :meth:`export_state`) as the complete managed setup."""
        state = ManagedState.from_dict(data)
        for key in state.settings:
            validate_key(key)
            validate_value(state.settings[key])
        for kw, values in state.lists.items():
            validate_key(kw)
            for v in values:
                validate_value(v)
            if self.mode == "lua":
                lua.list_calls(kw, values)
        self._clear_pending()
        for key in self._managed.settings.keys() - state.settings.keys():
            self._pending_reset.add(key)
        for key, value in state.settings.items():
            if self._managed.settings.get(key) != value:
                self._pending_set[key] = value
        for kw in self._managed.lists.keys() - state.lists.keys():
            self._pending_lists[kw] = None
        for kw, values in state.lists.items():
            if self._managed.lists.get(kw) != values:
                self._pending_lists[kw] = values
        self._emit("changed", None)

    # ------------------------------------------------------------ events & state

    def subscribe(self, fn: Listener) -> Callable[[], None]:
        """Register ``fn(event, key)``; returns an unsubscribe callable."""
        self._listeners.append(fn)
        return lambda: self._listeners.remove(fn) if fn in self._listeners else None

    def _emit(self, event: str, key: str | None) -> None:
        for fn in list(self._listeners):
            try:
                fn(event, key)
            except Exception as exc:  # one bad listener must not break the engine
                print(f"hyprland-mod: listener error on {event}: {exc}", file=sys.stderr)

    def _read_state(self) -> dict:
        try:
            data = json.loads(Path(self.data_dir, "state.json").read_text(encoding="utf-8"))
            return data if isinstance(data, dict) else {}
        except (OSError, ValueError):
            return {}

    def _write_state(self, state: dict) -> None:
        self._write_json("state.json", state)

    def _write_json(self, name: str, data: dict) -> None:
        atomic_write(os.path.join(self.data_dir, name), json.dumps(data, indent=1))
