"""Single import point for the core API (real core, or dev fake when requested)."""

import os

if os.environ.get("HYPRLAND_MOD_FAKE_CORE") == "1":
    from .dev_fake import ConfigManager, load_schema, format_value, parse_value, ApplyResult  # noqa: F401
    USING_FAKE = True
else:
    from hyprland_mod.core import ConfigManager, load_schema, format_value, parse_value, ApplyResult  # noqa: F401
    USING_FAKE = False


def is_dry_run(cm=None):
    """Explicit dry-run only (core also treats "not on Hyprland" as dry-run)."""
    return os.environ.get("HYPRLAND_MOD_DRY_RUN", "").strip().lower() in ("1", "true", "yes", "on")


def current_text(cm, opt):
    """Current value of an option as config text (falls back to the schema default)."""
    text = cm.get(opt["key"])
    if text is None:
        try:
            text = format_value(opt, parse_value(opt, None))
        except Exception:
            text = str(opt.get("default", "") or "")
    return str(text)


def current_value(cm, opt):
    try:
        return parse_value(opt, cm.get(opt["key"]))
    except Exception:
        return parse_value(opt, None)


def pending_count(cm):
    try:
        n = len(cm.pending())
        if hasattr(cm, "pending_resets"):
            n += len(cm.pending_resets())
        return n or (1 if cm.is_dirty else 0)
    except Exception:
        return 1 if cm.is_dirty else 0
