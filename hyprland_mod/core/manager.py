"""ConfigManager: read, stage, write, live-apply, back up and undo Hyprland settings."""

from __future__ import annotations

import json
import os
import re
import sys
import time
import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from . import actions, hyprctl, items, lua
from .backup import BackupStore
from .fsutil import atomic_write, read_text
from .managed import (
    EXTRA_FIELDS, LIST_ORDER, ManagedState, read_managed, render, validate_key, validate_value,
    with_source_line, without_source_line,
)
from .parser import ParsedConfig, normalize_key, parse_config
from .values import format_float

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
        # Staged binds / window_rules / monitors: the complete new value per field.
        self._pending_items: dict[str, list | dict] = {}
        self._live_binds: list[dict] | None = None
        self._live_monitors: list[dict] | None = None
        self._monitor_tokens: dict[str, dict] = {}
        self.last_apply_result: ApplyResult | None = None
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
        return bool(self._pending_set or self._pending_reset or self._pending_lists or self._pending_items)

    def discard(self) -> None:
        self._clear_pending()
        self._emit("discarded", None)

    def _clear_pending(self) -> None:
        self._pending_set.clear()
        self._pending_reset.clear()
        self._pending_lists.clear()
        self._pending_items.clear()

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
        for name, value in self._pending_items.items():
            setattr(state, name, json.loads(json.dumps(value)))
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
            text = self._render(new)  # before any write: a render error leaves files untouched
            first = self._ensure_initial_backup()
            atomic_write(self.managed_path, text)
        except (OSError, ValueError) as exc:
            return ApplyResult(False, [f"Could not write settings: {exc}"]), False
        errors: list[str] = []
        try:
            if new.is_empty:  # nothing managed any more: leave the user's config as it was
                self._remove_source_line()
            else:
                self._ensure_source_line(backup_first=not first, label=label)
        except OSError as exc:
            errors.append(f"Could not update the source line in {self.config_path}: {exc}")
        self._clear_pending()
        self.reload()
        live, live_errors = self._live_apply(old, self._managed)
        self._live_binds = self._live_monitors = None  # live state changed: re-read on demand
        errors += live_errors
        return ApplyResult(not errors, errors, live), True

    def _render(self, state: ManagedState) -> str:
        """The managed file for ``state`` in the active config language."""
        if self.mode == "lua":
            return lua.render_lua(state, self.lua_types, items.lua_calls(state))
        return render(state) + items.render_conf_extras(state)

    def _ensure_initial_backup(self) -> bool:
        """Back up the user's config the very first time anything is written."""
        state = self._read_state()
        if state.get("initial_backup"):
            return False
        # Taken before the source line is added. If the line is somehow already there
        # (e.g. the app's data folder was deleted), it is left out of the copy.
        texts = {}
        try:
            current = read_text(self.config_path)
            stripped = self._without_loader(current)
            if stripped is not None:
                texts[self.config_path] = stripped
        except OSError:
            pass
        backup_id = self.backups.create("Original config (before Hyprland Mod)", [self.config_path], texts)
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

    def _without_loader(self, text: str) -> str | None:
        remove = lua.without_require_line if self.mode == "lua" else without_source_line
        return remove(text, self.config_path, self.managed_path)

    def _remove_source_line(self) -> None:
        """Drop our source/require line (and its marker comment) from the user's config."""
        if not os.path.isfile(self.config_path):
            return
        new_text = self._without_loader(read_text(self.config_path))
        if new_text is not None:
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
        added, items_reload = _item_changes(old, new)
        need_reload = need_reload or items_reload
        commands += [line for state in added for line in items.conf_lines(state)]
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
        new_items, items_reload = _item_changes(old, new)
        if need_reload or items_reload:
            return self._run(["reload"])
        try:
            code = "; ".join(c for c in [lua.eval_code(changed, added, self.lua_types)]
                             + [call for state in new_items for call in items.lua_calls(state)] if c)
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
                # The managed file did not exist then: restore "no Hyprland Mod settings",
                # written in the config's own language (a conf header is invalid Lua).
                atomic_write(self.managed_path, self._render(ManagedState()))
        except OSError as exc:
            return ApplyResult(False, [f"Could not restore backup: {exc}"])
        self._clear_pending()
        self.reload()
        self._push_history(self._managed)
        live, errors = (False, []) if self.dry_run else self._run(["reload"])
        self._emit("restored", None)
        return ApplyResult(not errors, errors, live)

    # ------------------------------------------------------------ keybinds

    def _staged_items(self, name: str):
        """Staged copy of ``binds``/``window_rules``/``monitors`` (safe to modify)."""
        value = self._pending_items[name] if name in self._pending_items else getattr(self._managed, name)
        return json.loads(json.dumps(value))

    def _stage_items(self, name: str, value) -> None:
        if value == getattr(self._managed, name):
            self._pending_items.pop(name, None)
        else:
            self._pending_items[name] = value
        self._emit("changed", name)

    def list_binds(self) -> list[dict]:
        """The user's own binds (read-only) followed by Hyprland Mod's (staged) binds."""
        mod = [{**b, "raw": items.bind_raw(b), "source": "mod"} for b in self._staged_items("binds")]
        return self._user_binds() + mod

    def _user_binds(self) -> list[dict]:
        if not self.dry_run:
            if self._live_binds is None:
                self._live_binds = self._fetch_live_binds()
            binds = [dict(b) for b in self._live_binds]
            # Our applied binds are loaded too: drop one live match for each (they load last).
            for b in self._managed.binds:
                key = items.combo(b["mods"], b["key"])
                for i in range(len(binds) - 1, -1, -1):
                    if items.combo(binds[i]["mods"], binds[i]["key"]) == key and not binds[i]["submap"]:
                        del binds[i]
                        break
            return binds
        if self.mode == "conf":
            return _parsed_conf_binds(self._user)
        return _scanned_lua_binds(lua.required_files(self.config_path, exclude=[self.managed_path]))

    def _fetch_live_binds(self) -> list[dict]:
        """``hyprctl -j binds`` as bind dicts (read-only; Lua binds show as ``__lua``)."""
        try:
            code, out, _err = self.runner(["-j", "binds"])
            data = json.loads(out) if code == 0 else []
        except Exception:
            return []
        result: list[dict] = []
        for i, item in enumerate(data if isinstance(data, list) else []):
            if not isinstance(item, dict) or not isinstance(item.get("key"), str):
                continue
            mask = int(item.get("modmask") or 0)
            dispatcher, arg = str(item.get("dispatcher") or ""), str(item.get("arg") or "")
            action, action_arg = actions.match_conf(dispatcher, arg) if dispatcher != "__lua" else (None, None)
            flags = "".join(letter for letter, field_name in (
                ("l", "locked"), ("e", "repeat"), ("r", "release"), ("n", "non_consuming"),
                ("o", "longPress"), ("m", "mouse")) if item.get(field_name) is True)
            result.append({
                "id": f"user-{i}", "mods": [m for m in items.MOD_ORDER if mask & items.MOD_BITS[m]],
                "key": item["key"], "action": action, "arg": action_arg,
                "raw": "" if dispatcher == "__lua" else f"{dispatcher} {arg}".strip(),
                "description": str(item.get("description") or ""), "source": "user", "flags": flags,
                "submap": str(item.get("submap") or ""),
            })
        return result

    def add_bind(self, bind: dict) -> str:
        clean = items.clean_bind(bind, self.mode)
        binds = self._staged_items("binds")
        if any(b["id"] == clean["id"] for b in binds):
            clean["id"] = items.new_id("bind")
        binds.append(clean)
        self._stage_items("binds", binds)
        return clean["id"]

    def update_bind(self, bind_id: str, bind: dict) -> None:
        binds = self._staged_items("binds")
        for i, b in enumerate(binds):
            if b["id"] == bind_id:
                binds[i] = items.clean_bind({**b, **bind}, self.mode, bind_id=bind_id)
                self._stage_items("binds", binds)
                return
        raise KeyError(f"unknown bind: {bind_id}")

    def remove_bind(self, bind_id: str) -> None:
        binds = self._staged_items("binds")
        kept = [b for b in binds if b["id"] != bind_id]
        if len(kept) == len(binds):
            raise KeyError(f"unknown bind: {bind_id}")
        self._stage_items("binds", kept)

    def find_conflicts(self, mods, key: str, exclude_id: str | None = None) -> list[dict]:
        """Binds (user or ours, staged) on the same key combination, outside submaps."""
        target = items.combo(items.normalize_mods(mods), str(key).strip())
        return [b for b in self.list_binds()
                if b["id"] != exclude_id and not b.get("submap")
                and items.combo(b["mods"], b["key"]) == target]

    # ------------------------------------------------------------ window rules

    def list_window_rules(self) -> list[dict]:
        """Hyprland Mod's (staged) window rules. The user's own rules are not listed."""
        return self._staged_items("window_rules")

    def add_window_rule(self, rule: dict) -> str:
        clean = items.clean_rule(rule, self.mode)
        rules = self._staged_items("window_rules")
        if any(r["id"] == clean["id"] for r in rules):
            clean["id"] = items.new_id("rule")
        rules.append(clean)
        self._stage_items("window_rules", rules)
        return clean["id"]

    def update_window_rule(self, rule_id: str, rule: dict) -> None:
        rules = self._staged_items("window_rules")
        for i, r in enumerate(rules):
            if r["id"] == rule_id:
                rules[i] = items.clean_rule({**r, **rule}, self.mode, rule_id=rule_id)
                self._stage_items("window_rules", rules)
                return
        raise KeyError(f"unknown window rule: {rule_id}")

    def remove_window_rule(self, rule_id: str) -> None:
        rules = self._staged_items("window_rules")
        kept = [r for r in rules if r["id"] != rule_id]
        if len(kept) == len(rules):
            raise KeyError(f"unknown window rule: {rule_id}")
        self._stage_items("window_rules", kept)

    # ------------------------------------------------------------ monitors

    def _monitor_info(self) -> list[dict]:
        """Connected monitors: live (read-only hyprctl) or, in dry-run, an optional JSON fixture."""
        if self._live_monitors is None:
            data: object = []
            if not self.dry_run:
                try:
                    code, out, _err = self.runner(["-j", "monitors", "all"])
                    data = json.loads(out) if code == 0 else []
                except Exception:
                    data = []
            elif os.environ.get("HYPRLAND_MOD_MONITORS_JSON"):
                try:
                    data = json.loads(read_text(os.environ["HYPRLAND_MOD_MONITORS_JSON"]))
                except (OSError, ValueError):
                    data = []
            keys = ("name", "description", "make", "model", "width", "height", "refreshRate",
                    "x", "y", "scale", "transform", "disabled", "availableModes")
            self._live_monitors = [{k: m.get(k) for k in keys} for m in (data if isinstance(data, list) else [])
                                   if isinstance(m, dict) and isinstance(m.get("name"), str)]
        return [dict(m) for m in self._live_monitors]

    def list_monitors(self) -> list[dict]:
        """Known monitors with ``override`` (staged Hyprland Mod settings or None)."""
        staged = self._staged_items("monitors")
        out = []
        for m in self._monitor_info():
            m["override"] = staged.get(m["name"])
            m["availableModes"] = list(m.get("availableModes") or [])
            out.append(m)
        known = {m["name"] for m in out}
        for name, spec in sorted(staged.items()):
            if name not in known:  # not connected (or dry-run): show what we manage
                out.append({"name": name, "description": "", "make": "", "model": "", "width": None,
                            "height": None, "refreshRate": None, "x": None, "y": None, "scale": None,
                            "transform": None, "disabled": None, "availableModes": [], "override": spec})
        return out

    def _current_monitor_spec(self, name: str) -> dict:
        for m in self._monitor_info():
            if m["name"] == name and m.get("width"):
                rate = m.get("refreshRate")
                mode = f"{m['width']}x{m['height']}" + (f"@{rate:.2f}" if isinstance(rate, (int, float)) else "")
                scale = m.get("scale")
                return {"mode": mode, "position": f"{m.get('x') or 0}x{m.get('y') or 0}",
                        "scale": format_float(float(scale)) if isinstance(scale, (int, float)) else "auto",
                        "transform": int(m.get("transform") or 0), "enabled": not m.get("disabled")}
        return {}

    def set_monitor(self, name: str, spec: dict) -> None:
        """Stage settings for one monitor; unspecified fields keep their current values."""
        name = items.clean_monitor_name(name)
        monitors = self._staged_items("monitors")
        base = monitors.get(name) or self._current_monitor_spec(name)
        monitors[name] = items.clean_monitor({**base, **{k: v for k, v in spec.items() if v is not None}})
        self._stage_items("monitors", monitors)

    def reset_monitor(self, name: str) -> None:
        monitors = self._staged_items("monitors")
        monitors.pop(name, None)
        self._stage_items("monitors", monitors)

    def apply_monitors_with_revert(self, timeout_s: float = 15) -> str:
        """Apply only the staged monitor changes; returns a token for confirm/revert.

        The UI must call :meth:`confirm_monitors` within ``timeout_s`` or :meth:`revert_monitors`.
        The apply result is in :attr:`last_apply_result`.
        """
        token = uuid.uuid4().hex
        self._monitor_tokens[token] = {"previous": json.loads(json.dumps(self._managed.monitors)),
                                       "deadline": time.monotonic() + timeout_s}
        if "monitors" in self._pending_items:
            new = self._managed.copy()
            new.monitors = self._staged_items("monitors")
            self.last_apply_result = self._commit_only(new, "monitors", "Before changing monitors")
        else:
            self.last_apply_result = ApplyResult(True, [], False)
        return token

    def monitors_deadline(self, token: str) -> float | None:
        """``time.monotonic()`` deadline of a pending monitor change (None if unknown)."""
        entry = self._monitor_tokens.get(token)
        return entry["deadline"] if entry else None

    def confirm_monitors(self, token: str) -> bool:
        return self._monitor_tokens.pop(token, None) is not None

    def revert_monitors(self, token: str) -> ApplyResult:
        """Put back the monitor overrides from before :meth:`apply_monitors_with_revert`."""
        entry = self._monitor_tokens.pop(token, None)
        if entry is None:
            return ApplyResult(False, ["Unknown or already confirmed monitor change"])
        new = self._managed.copy()
        new.monitors = entry["previous"]
        if new.monitors == self._managed.monitors:
            return ApplyResult(True, [], False)
        result = self._commit_only(new, "monitors", "Before reverting monitors")
        self._emit("monitors_reverted", None)
        return result

    def _commit_only(self, new: ManagedState, field_name: str, label: str) -> ApplyResult:
        """Commit ``new`` while keeping every other staged change pending."""
        saved = (dict(self._pending_set), set(self._pending_reset), dict(self._pending_lists),
                 {k: v for k, v in self._pending_items.items() if k != field_name})
        result, written = self._commit(new, label=label)
        self._pending_set, self._pending_reset, self._pending_lists, self._pending_items = saved
        if written:
            self._push_history(self._managed)
            self._emit("applied", None)
        return result

    # ------------------------------------------------------------ autostart

    def get_autostart(self, entry_id: str) -> str | None:
        """Staged command started at login under ``entry_id`` (e.g. ``"bar"``), or None."""
        return self._staged_items("autostart").get(entry_id)

    def set_autostart(self, entry_id: str, command: str | None) -> None:
        """Stage a command for the next login (None removes it). Never run on apply."""
        entry_id = items.clean_autostart_id(entry_id)
        entries = self._staged_items("autostart")
        if command is None:
            entries.pop(entry_id, None)
        else:
            entries[entry_id] = items.clean_command(command)
        self._stage_items("autostart", entries)

    # ------------------------------------------------------------ current look (for presets)

    def snapshot_current_look(self, keys: list[str]) -> dict[str, str | None]:
        """The user's own look: values in effect without Hyprland Mod's overrides.

        Keys Hyprland Mod does not manage: live ``hyprctl -j descriptions`` when running,
        else the user's parsed config (conf). Managed keys: the user's parsed value (conf);
        in Lua mode that is unknowable without running their config, so None.
        None also means "not set": Hyprland's default. Staged changes are ignored.
        """
        live: dict[str, str] = {}
        if not self.dry_run and self.mode == "conf":
            live = self._fetch_descriptions()  # one read-only call
        out: dict[str, str | None] = {}
        for key in keys:
            k = normalize_key(key)
            if k in self._managed.settings:
                value = self._user.get(k) if self.mode == "conf" else None
            elif self.mode == "lua":
                value = self._user.get(k)  # live descriptions (cached); None in dry-run
            else:
                value = live.get(lua.dotted(k)) or self._user.get(k)
            out[key] = value
        return out

    def current_animations(self) -> dict[str, list[str]]:
        """The user's own curves/animations: live ``hyprctl -j animations`` when nothing is
        overridden by Hyprland Mod, else what their config files define."""
        live = None
        if not self.dry_run and not any(kw in self._managed.lists for kw in ("bezier", "animation")):
            live = self._fetch_animations()
        if live is not None:
            return live
        return {kw: self._user.get_all(kw) for kw in ("bezier", "animation")}

    def _fetch_animations(self) -> dict[str, list[str]] | None:
        try:
            code, out, _err = self.runner(["-j", "animations"])
            data = json.loads(out) if code == 0 else None
        except Exception:
            return None
        if not (isinstance(data, list) and len(data) == 2 and all(isinstance(x, list) for x in data)):
            return None
        anims, curves = data
        num = lambda v: format_float(float(v))  # noqa: E731
        beziers = []
        for c in curves:
            try:
                if isinstance(c, dict) and c.get("name") and c["name"] != "default" \
                        and not str(c["name"]).startswith("__"):
                    beziers.append(", ".join([c["name"], num(c["X0"]), num(c["Y0"]), num(c["X1"]), num(c["Y1"])]))
            except (KeyError, TypeError, ValueError):
                continue
        animations = []
        for a in anims:
            if not isinstance(a, dict) or not a.get("overridden") or not a.get("name"):
                continue
            parts = [str(a["name"]), "1" if a.get("enabled", True) else "0"]
            if a.get("enabled", True) and isinstance(a.get("speed"), (int, float)) and a.get("bezier"):
                parts += [num(a["speed"]), str(a["bezier"])]
                if a.get("style"):
                    parts.append(str(a["style"]))
            animations.append(", ".join(parts))
        return {"bezier": beziers, "animation": animations}

    # ------------------------------------------------------------ export / import

    def export_state(self) -> dict:
        """Managed setup including staged changes, as plain JSON-able data."""
        return self._staged_state().to_dict()

    def import_state(self, data: dict) -> None:
        """Stage ``data`` (from :meth:`export_state`) as the complete managed setup.

        ``binds``/``window_rules``/``monitors`` missing from ``data`` stay as they are
        (older exports and presets don't have them); present ones replace them.
        """
        state = ManagedState.from_dict(data)
        staged_items: dict[str, list | dict] = {}
        if "binds" in data:
            staged_items["binds"] = [items.clean_bind(b, self.mode) for b in state.binds]
        if "window_rules" in data:
            staged_items["window_rules"] = [items.clean_rule(r, self.mode) for r in state.window_rules]
        if "monitors" in data:
            staged_items["monitors"] = {items.clean_monitor_name(n): items.clean_monitor(m)
                                        for n, m in state.monitors.items()}
        if "autostart" in data:
            staged_items["autostart"] = {items.clean_autostart_id(k): items.clean_command(v)
                                         for k, v in state.autostart.items()}
        keep = {k: v for k, v in self._pending_items.items() if k not in data}
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
        self._pending_items.update(keep)
        for name, value in staged_items.items():
            if value != getattr(self._managed, name):
                self._pending_items[name] = value
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


def _item_changes(old: ManagedState, new: ManagedState) -> tuple[list[ManagedState], bool]:
    """Items to add live (as partial states, in render order) and whether a reload is needed.

    A bind or window rule that was removed or edited needs a reload (Hyprland would keep
    the old one); a monitor override can simply be re-sent, but dropping one needs a reload.
    """
    reload = False
    add = ManagedState()
    for field_name in ("binds", "window_rules"):
        before = {i["id"]: i for i in getattr(old, field_name) if i.get("enabled", True)}
        after = {i["id"]: i for i in getattr(new, field_name) if i.get("enabled", True)}
        if any(after.get(k) != v for k, v in before.items()):
            reload = True
        setattr(add, field_name, [v for k, v in after.items() if k not in before])
    if old.monitors.keys() - new.monitors.keys():
        reload = True
    add.monitors = {k: v for k, v in new.monitors.items() if old.monitors.get(k) != v}
    return ([add] if not add.is_empty else []), reload


def _parsed_conf_binds(parsed: ParsedConfig) -> list[dict]:
    """``bind*`` lines of a parsed conf config as bind dicts (best effort, read-only)."""
    out: list[dict] = []
    for i, e in enumerate(parsed.entries):
        if not e.key.startswith("bind") or e.key in ("binds",) or ":" in e.key:
            continue
        flags = e.key[4:]
        fields = [f.strip() for f in e.value.split(",", 4 if "d" in flags else 3)]
        description = fields.pop(2) if "d" in flags and len(fields) > 3 else ""
        if len(fields) < 3:
            continue
        try:
            mods = items.normalize_mods(fields[0].replace("$", ""))
        except ValueError:
            mods = []
        dispatcher, arg = fields[2], fields[3] if len(fields) > 3 else ""
        action, action_arg = actions.match_conf(dispatcher, arg)
        out.append({"id": f"user-{i}", "mods": mods, "key": fields[1], "action": action, "arg": action_arg,
                    "raw": f"{dispatcher} {arg}".strip(), "description": description, "source": "user",
                    "flags": "".join(f for f in flags if f in items.BIND_FLAGS), "submap": ""})
    return out


def _scanned_lua_binds(paths: list[str]) -> list[dict]:
    """``hl.bind("MODS + KEY", ...)`` calls with a literal key string (light scan, read-only)."""
    out: list[dict] = []
    for path in paths:
        try:
            code = "\n".join(lua._strip_lua_comments(read_text(path)))
        except OSError:
            continue
        for args in lua._calls(code, "hl.bind"):
            m = re.match(r"\s*([\"'])([^\"']+)\1\s*,\s*(.*)$", args, re.DOTALL)
            if not m:
                continue
            *mods, key = [p.strip() for p in m.group(2).split("+")]
            try:
                mods = items.normalize_mods(mods)
            except ValueError:
                continue
            out.append({"id": f"user-{len(out)}", "mods": mods, "key": key, "action": None, "arg": None,
                        "raw": " ".join(m.group(3).split())[:200], "description": "", "source": "user",
                        "flags": "", "submap": ""})
    return out
