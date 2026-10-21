"""Config engine: parse, stage, write, live-apply, back up and undo Hyprland settings."""

from .manager import ApplyResult, ConfigManager
from .schema import load_schema
from .values import format_value, parse_color, parse_value

__all__ = ["ApplyResult", "ConfigManager", "format_value", "load_schema", "parse_color", "parse_value"]
